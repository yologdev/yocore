//! Database module for Yolog Core
//!
//! Provides SQLite storage for projects, sessions, memories, and skills.

pub mod schema;

use crate::error::{CoreError, Result};
use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::Mutex;

/// Database manager with connection pooling
pub struct Database {
    conn: Mutex<Connection>,
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

        // Initialize schema
        schema::init_db(&conn)?;

        Ok(Database {
            conn: Mutex::new(conn),
            path: db_path,
        })
    }

    /// Get a connection from the pool
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
