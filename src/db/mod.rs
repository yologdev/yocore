//! Database module for Yolog Core
//!
//! Provides SQLite storage for projects, sessions, memories, and skills.
//!
//! Uses two connections with SQLite WAL mode for concurrent read/write access:
//! - **Write connection** (`with_conn`): for watcher, AI tasks, and any INSERT/UPDATE/DELETE
//! - **Read connection** (`with_read_conn`): for API queries — never blocked by writes

pub mod schema;

use crate::error::Result;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Database manager with separate read/write connections.
///
/// SQLite WAL mode allows concurrent reads while a write is in progress.
/// Two connections exploit this: API reads proceed even during long writes.
pub struct Database {
    write_conn: Arc<Mutex<Connection>>,
    read_conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

/// Configure common PRAGMAs on a connection
fn configure_connection(conn: &Connection) -> std::result::Result<(), rusqlite::Error> {
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    let _: i64 = conn.query_row("PRAGMA wal_autocheckpoint = 100", [], |row| row.get(0))?;
    let _: i64 = conn.query_row("PRAGMA journal_size_limit = 209715200", [], |row| {
        row.get(0)
    })?;
    Ok(())
}

impl Database {
    /// Create a new database with separate read and write connections
    pub fn new(db_path: PathBuf) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write connection — used by watcher, AI tasks, any mutations
        let write_conn = Connection::open(&db_path)?;
        configure_connection(&write_conn)?;

        // Initialize schema on write connection
        schema::init_db(&write_conn)?;

        // Read connection — used by API queries, never blocked by writes
        let read_conn = Connection::open(&db_path)?;
        configure_connection(&read_conn)?;

        Ok(Database {
            write_conn: Arc::new(Mutex::new(write_conn)),
            read_conn: Arc::new(Mutex::new(read_conn)),
            path: db_path,
        })
    }

    /// Run a write operation asynchronously (watcher, AI, mutations).
    ///
    /// Uses the write connection. Other write operations will wait for the lock,
    /// but read operations via `with_read_conn` proceed concurrently.
    ///
    /// # Example
    /// ```ignore
    /// db.with_conn(|conn| {
    ///     conn.execute("INSERT INTO projects ...", params![...])
    /// }).await;
    /// ```
    pub async fn with_conn<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&Connection) -> T + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.write_conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap();
            f(&guard)
        })
        .await
        .expect("spawn_blocking task panicked")
    }

    /// Run a read-only operation asynchronously (API queries).
    ///
    /// Uses a separate read connection that is never blocked by writes.
    /// Thanks to SQLite WAL mode, reads see a consistent snapshot even
    /// while the write connection is mid-transaction.
    pub async fn with_read_conn<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&Connection) -> T + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.read_conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap();
            f(&guard)
        })
        .await
        .expect("spawn_blocking task panicked")
    }

    /// Run a write operation that returns a Result asynchronously.
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
    #[deprecated(note = "Use with_conn() in async code to avoid blocking the runtime")]
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.write_conn.lock().unwrap()
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
