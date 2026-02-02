//! Yocore - headless service for watching, parsing, storing, and serving AI coding sessions
//!
//! This crate provides the core functionality for Yolog:
//! - Session file watching and parsing
//! - SQLite storage with FTS5 search
//! - HTTP API for remote access
//! - MCP server integration for AI assistants
//!
//! # Usage
//!
//! As a library (embedded in Desktop):
//! ```ignore
//! use yocore::{Config, Core};
//!
//! let config = Config::from_file("~/.yolog/config.toml").unwrap();
//! let core = Core::new(config).unwrap();
//! // core.start_watching().await.unwrap();
//! ```
//!
//! As a standalone server (CLI):
//! ```text
//! yocore --config ~/.yolog/config.toml
//! ```

pub mod api;
pub mod config;
pub mod db;
pub mod embeddings;
pub mod error;
pub mod handlers;
pub mod mcp;
pub mod parser;
pub mod watcher;

// Re-export main types for convenience
pub use config::Config;
pub use db::Database;
pub use error::{CoreError, Result};

use std::sync::Arc;
use tokio::sync::RwLock;

/// Core service that coordinates all Yolog functionality
pub struct Core {
    /// Configuration
    pub config: Config,

    /// Database connection
    pub db: Arc<Database>,

    /// File watcher state (optional, only when watching is active)
    watcher_handle: RwLock<Option<watcher::WatcherHandle>>,
}

impl Core {
    /// Create a new Core instance with the given configuration
    pub fn new(config: Config) -> Result<Self> {
        let db_path = config.data_dir().join("yolog.db");
        let db = Database::new(db_path)?;

        Ok(Core {
            config,
            db: Arc::new(db),
            watcher_handle: RwLock::new(None),
        })
    }

    /// Create a Core instance with an existing database (for Desktop embedding)
    pub fn with_database(config: Config, db: Arc<Database>) -> Self {
        Core {
            config,
            db,
            watcher_handle: RwLock::new(None),
        }
    }

    /// Start the file watcher for configured watch paths
    pub async fn start_watching(&self) -> Result<()> {
        let handle = watcher::start_watcher(&self.config, self.db.clone()).await?;
        *self.watcher_handle.write().await = Some(handle);
        Ok(())
    }

    /// Stop the file watcher
    pub async fn stop_watching(&self) -> Result<()> {
        if let Some(handle) = self.watcher_handle.write().await.take() {
            handle.stop().await?;
        }
        Ok(())
    }

    /// Start the HTTP API server
    pub async fn start_api_server(&self) -> Result<()> {
        let addr = self.config.server_addr();
        tracing::info!("Starting API server on {}", addr);
        api::serve(addr, self.db.clone(), &self.config).await
    }

    /// Get a reference to the database
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }
}
