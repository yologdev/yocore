//! Yocore - headless service for watching, parsing, storing, and serving AI coding sessions
//!
//! This crate provides the core functionality for Yolog:
//! - Session file watching and parsing
//! - SQLite storage with FTS5 search
//! - HTTP API for remote access
//! - MCP server integration for AI assistants
//! - AI features (title generation, memory/skill extraction)
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

pub mod ai;
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

use ai::queue::AiTaskQueue;
use ai::types::AiEvent;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Core service that coordinates all Yolog functionality
pub struct Core {
    /// Configuration
    pub config: Config,

    /// Database connection
    pub db: Arc<Database>,

    /// File watcher state (optional, only when watching is active)
    watcher_handle: RwLock<Option<watcher::WatcherHandle>>,

    /// Broadcast channel for SSE events (from watcher to API clients)
    event_tx: broadcast::Sender<watcher::WatcherEvent>,

    /// Broadcast channel for AI-related SSE events
    ai_event_tx: broadcast::Sender<AiEvent>,

    /// AI task queue for concurrency control
    ai_task_queue: AiTaskQueue,
}

impl Core {
    /// Create a new Core instance with the given configuration
    pub fn new(config: Config) -> Result<Self> {
        let db_path = config.data_dir().join("yolog.db");
        let db = Database::new(db_path)?;
        let (event_tx, _) = broadcast::channel(256);
        let (ai_event_tx, _) = broadcast::channel(256);
        let ai_task_queue = AiTaskQueue::new(3);

        Ok(Core {
            config,
            db: Arc::new(db),
            watcher_handle: RwLock::new(None),
            event_tx,
            ai_event_tx,
            ai_task_queue,
        })
    }

    /// Create a Core instance with an existing database (for Desktop embedding)
    pub fn with_database(config: Config, db: Arc<Database>) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let (ai_event_tx, _) = broadcast::channel(256);
        let ai_task_queue = AiTaskQueue::new(3);
        Core {
            config,
            db,
            watcher_handle: RwLock::new(None),
            event_tx,
            ai_event_tx,
            ai_task_queue,
        }
    }

    /// Start the file watcher for configured watch paths
    pub async fn start_watching(&self) -> Result<()> {
        let handle = watcher::start_watcher(
            &self.config,
            self.db.clone(),
            self.event_tx.clone(),
        )
        .await?;
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
        api::serve(
            addr,
            self.db.clone(),
            &self.config,
            self.event_tx.clone(),
            self.ai_event_tx.clone(),
            self.ai_task_queue.clone(),
        )
        .await
    }

    /// Get the AI event broadcaster (for emitting AI events)
    pub fn ai_event_tx(&self) -> &broadcast::Sender<AiEvent> {
        &self.ai_event_tx
    }

    /// Get the AI task queue
    pub fn ai_task_queue(&self) -> &AiTaskQueue {
        &self.ai_task_queue
    }

    /// Get a reference to the database
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// Start periodic memory ranking background task
    ///
    /// This spawns a background task that runs memory ranking on all projects
    /// at the configured interval (default: every 6 hours).
    pub fn start_periodic_ranking(&self) {
        let ranking_config = &self.config.ai.features.ranking;

        if !ranking_config.enabled {
            tracing::info!("Memory ranking is disabled");
            return;
        }

        let interval_hours = ranking_config.interval_hours;
        let batch_size = ranking_config.batch_size;
        let db = self.db.clone();
        let event_tx = self.event_tx.clone();

        tracing::info!(
            "Starting periodic memory ranking (every {} hours, batch size {})",
            interval_hours,
            batch_size
        );

        tokio::spawn(async move {
            use std::time::Duration;

            let interval = Duration::from_secs(interval_hours as u64 * 3600);
            let mut ticker = tokio::time::interval(interval);

            // Skip the first immediate tick
            ticker.tick().await;

            loop {
                ticker.tick().await;
                tracing::info!("Running periodic memory ranking sweep");

                // Get all project IDs using spawn_blocking to avoid blocking async runtime
                let db_clone = db.clone();
                let project_ids: Vec<String> =
                    match tokio::task::spawn_blocking(move || {
                        let conn = db_clone.conn();
                        conn.prepare("SELECT id FROM projects")
                            .and_then(|mut stmt| {
                                stmt.query_map([], |row| row.get(0))
                                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                            })
                            .unwrap_or_default()
                    })
                    .await
                    {
                        Ok(ids) => ids,
                        Err(e) => {
                            tracing::error!("Failed to get project IDs: {}", e);
                            continue;
                        }
                    };

                for project_id in project_ids {
                    // Emit start event
                    let _ = event_tx.send(watcher::WatcherEvent::RankingStart {
                        project_id: project_id.clone(),
                    });

                    // Run ranking in spawn_blocking with timeout
                    let db_clone = db.clone();
                    let pid = project_id.clone();
                    let ranking_future = tokio::task::spawn_blocking(move || {
                        ai::ranking::rank_project_memories(&db_clone, &pid, batch_size)
                    });

                    // Timeout after 60 seconds per project
                    let result = tokio::time::timeout(Duration::from_secs(60), ranking_future).await;

                    match result {
                        Ok(Ok(Ok(ranking_result))) => {
                            if ranking_result.memories_evaluated > 0 {
                                tracing::info!(
                                    "Ranked project {}: {} evaluated, {} promoted, {} demoted, {} removed",
                                    project_id,
                                    ranking_result.memories_evaluated,
                                    ranking_result.promoted,
                                    ranking_result.demoted,
                                    ranking_result.removed
                                );
                            }

                            // Emit complete event
                            let _ = event_tx.send(watcher::WatcherEvent::RankingComplete {
                                project_id: project_id.clone(),
                                promoted: ranking_result.promoted,
                                demoted: ranking_result.demoted,
                                removed: ranking_result.removed,
                            });
                        }
                        Ok(Ok(Err(e))) => {
                            tracing::error!("Failed to rank project {}: {}", project_id, e);
                            let _ = event_tx.send(watcher::WatcherEvent::RankingError {
                                project_id: project_id.clone(),
                                error: e,
                            });
                        }
                        Ok(Err(e)) => {
                            tracing::error!("Ranking task panicked for project {}: {}", project_id, e);
                            let _ = event_tx.send(watcher::WatcherEvent::RankingError {
                                project_id: project_id.clone(),
                                error: format!("Task panicked: {}", e),
                            });
                        }
                        Err(_) => {
                            tracing::error!("Ranking timed out for project {}", project_id);
                            let _ = event_tx.send(watcher::WatcherEvent::RankingError {
                                project_id: project_id.clone(),
                                error: "Ranking timed out after 60 seconds".to_string(),
                            });
                        }
                    }

                    // Yield to other tasks between projects
                    tokio::task::yield_now().await;
                }

                tracing::info!("Periodic memory ranking sweep complete");
            }
        });
    }

    /// Get the event sender for broadcasting events
    pub fn event_sender(&self) -> broadcast::Sender<watcher::WatcherEvent> {
        self.event_tx.clone()
    }
}
