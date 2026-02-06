//! Database module for Yolog Core
//!
//! Provides SQLite storage for projects, sessions, memories, and skills.
//!
//! IMPORTANT: Use `with_conn()` or `with_conn_result()` for all database operations
//! in async code. These methods run database operations in a blocking thread pool
//! to avoid blocking the tokio runtime.

pub mod schema;

use crate::error::{CoreError, Result};
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Database manager with async-safe connection access
///
/// Uses `std::sync::Mutex` internally but provides async-safe access via
/// `with_conn()` which runs operations in `spawn_blocking`.
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl Database {
    /// Create a new database connection
    pub fn new(db_path: PathBuf) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;

        // Enable foreign keys
        conn.execute("PRAGMA foreign_keys = ON", [])?;

        // WAL mode for better concurrent read performance
        // Use query_row since PRAGMA journal_mode returns a result
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;

        // Prevent WAL file from growing unbounded
        // Autocheckpoint every 100 pages (~400KB) instead of default 1000
        let _: i64 = conn.query_row("PRAGMA wal_autocheckpoint = 100", [], |row| row.get(0))?;
        // Cap WAL file at 200MB — forces checkpoint even under heavy write load
        let _: i64 = conn.query_row("PRAGMA journal_size_limit = 209715200", [], |row| row.get(0))?;

        // Initialize schema
        schema::init_db(&conn)?;

        Ok(Database {
            conn: Arc::new(Mutex::new(conn)),
            path: db_path,
        })
    }

    /// Run a database operation asynchronously without blocking the tokio runtime.
    ///
    /// This is the preferred way to access the database in async code.
    /// The closure runs in a blocking thread pool via `spawn_blocking`.
    ///
    /// # Example
    /// ```ignore
    /// let count = db.with_conn(|conn| {
    ///     conn.query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
    /// }).await.unwrap_or(0);
    /// ```
    pub async fn with_conn<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&Connection) -> T + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap();
            f(&guard)
        })
        .await
        .expect("spawn_blocking task panicked")
    }

    /// Run a database operation that returns a Result asynchronously.
    ///
    /// Convenience wrapper for operations that return `rusqlite::Result`.
    pub async fn with_conn_result<F, T>(&self, f: F) -> rusqlite::Result<T>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.with_conn(f).await
    }

    /// Get a synchronous connection guard (for use in non-async contexts only)
    ///
    /// WARNING: Do NOT use this in async code - it will block the tokio runtime.
    /// Use `with_conn()` instead.
    #[deprecated(note = "Use with_conn() in async code to avoid blocking the runtime")]
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    /// Get the database file path
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

// Re-export schema for convenience
pub use schema::init_db;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_creation() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_yocore.db");

        let db = Database::new(db_path.clone());
        assert!(db.is_ok());

        // Cleanup
        let _ = std::fs::remove_file(db_path);
    }
}
