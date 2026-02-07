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
//! use std::path::PathBuf;
//!
//! let config_path = PathBuf::from("~/.yolog/config.toml");
//! let config = Config::from_file(&config_path).unwrap();
//! let core = Core::new(config, config_path).unwrap();
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
pub mod scheduler;
pub mod watcher;

// Re-export main types for convenience
pub use config::Config;
pub use db::Database;
pub use error::{CoreError, Result};

use ai::queue::AiTaskQueue;
use ai::types::AiEvent;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Core service that coordinates all Yolog functionality
pub struct Core {
    /// Configuration
    pub config: Config,

    /// Path to config file (for config API)
    pub config_path: PathBuf,

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
    pub fn new(config: Config, config_path: PathBuf) -> Result<Self> {
        let db_path = config.data_dir().join("yolog.db");
        let db = Database::new(db_path)?;
        let (event_tx, _) = broadcast::channel(256);
        let (ai_event_tx, _) = broadcast::channel(256);
        let ai_task_queue = AiTaskQueue::new(3);

        Ok(Core {
            config,
            config_path,
            db: Arc::new(db),
            watcher_handle: RwLock::new(None),
            event_tx,
            ai_event_tx,
            ai_task_queue,
        })
    }

    /// Create a Core instance with an existing database (for Desktop embedding)
    pub fn with_database(config: Config, config_path: PathBuf, db: Arc<Database>) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let (ai_event_tx, _) = broadcast::channel(256);
        let ai_task_queue = AiTaskQueue::new(3);
        Core {
            config,
            config_path,
            db,
            watcher_handle: RwLock::new(None),
            event_tx,
            ai_event_tx,
            ai_task_queue,
        }
    }

    /// Sync projects from config — auto-create or update DB projects
    ///
    /// For each `[[projects]]` entry, scans subdirectories and ensures a
    /// matching project record exists in the database. Updates names if changed.
    pub async fn sync_projects_from_config(&self) {
        let db = self.db.clone();

        for project_config in &self.config.projects {
            if !project_config.enabled {
                continue;
            }

            let path = config::expand_path(&project_config.path);
            if !path.exists() || !path.is_dir() {
                tracing::warn!(
                    "Project path does not exist: {}",
                    path.display()
                );
                continue;
            }

            // Scan subdirectories — each is a potential project
            let subdirs = match std::fs::read_dir(&path) {
                Ok(entries) => entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .collect::<Vec<_>>(),
                Err(e) => {
                    tracing::warn!("Failed to read project path {}: {}", path.display(), e);
                    continue;
                }
            };

            for entry in subdirs {
                let folder_path = entry.path();
                let folder_str = folder_path.to_string_lossy().to_string();
                let name = derive_project_name(&folder_path);

                let folder_for_query = folder_str.clone();
                let name_clone = name.clone();

                let result: std::result::Result<Option<String>, rusqlite::Error> = db
                    .with_conn(move |conn| {
                        // Check if project exists
                        let existing: Option<(String, String)> = conn
                            .query_row(
                                "SELECT id, name FROM projects WHERE folder_path = ?",
                                [&folder_for_query],
                                |row| Ok((row.get(0)?, row.get(1)?)),
                            )
                            .ok();

                        match existing {
                            Some((id, existing_name)) => {
                                if existing_name != name_clone {
                                    let _ = conn.execute(
                                        "UPDATE projects SET name = ?, updated_at = datetime('now') WHERE id = ?",
                                        rusqlite::params![name_clone, id],
                                    );
                                    Ok(Some(format!("updated:{}", name_clone)))
                                } else {
                                    Ok(None) // No change
                                }
                            }
                            None => {
                                let id = uuid::Uuid::new_v4().to_string();
                                conn.execute(
                                    "INSERT INTO projects (id, name, folder_path, auto_sync, created_at, updated_at)
                                     VALUES (?, ?, ?, 1, datetime('now'), datetime('now'))",
                                    rusqlite::params![id, name_clone, folder_for_query],
                                )?;
                                Ok(Some(format!("created:{}", name_clone)))
                            }
                        }
                    })
                    .await;

                match result {
                    Ok(Some(action)) => {
                        tracing::info!("Project sync: {} ({})", action, folder_str);
                    }
                    Ok(None) => {} // Already exists, no change
                    Err(e) => {
                        tracing::warn!("Failed to sync project {}: {}", folder_str, e);
                    }
                }
            }
        }
    }

    /// Start the file watcher for configured project paths
    pub async fn start_watching(&self) -> Result<()> {
        let handle = watcher::start_watcher(
            &self.config,
            self.config_path.clone(),
            self.db.clone(),
            self.event_tx.clone(),
            self.ai_event_tx.clone(),
            self.ai_task_queue.clone(),
        )
        .await?;
        *self.watcher_handle.write().await = Some(handle);
        Ok(())
    }

    /// Recover pending AI tasks on startup
    ///
    /// Checks for sessions that need title generation, memory extraction, or skill extraction
    /// and triggers them based on config feature flags.
    pub async fn recover_pending_ai_tasks(&self) {
        // Check if AI is enabled
        if !self.config.ai.enabled || self.config.ai.provider.is_none() {
            return;
        }

        let features = &self.config.ai.features;
        let db = self.db.clone();

        // Query pending sessions
        let sessions = match db
            .with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT s.id, s.message_count,
                        (COALESCE(s.title_ai_generated, 0) = 0 AND COALESCE(s.title_edited, 0) = 0) as needs_title,
                        (s.memories_extracted_at IS NULL) as needs_memory,
                        (s.skills_extracted_at IS NULL) as needs_skills
                    FROM sessions s
                    INNER JOIN projects p ON s.project_id = p.id
                    WHERE COALESCE(s.import_status, 'success') = 'success'
                      AND s.message_count >= 25
                      AND (
                        (COALESCE(s.title_ai_generated, 0) = 0 AND COALESCE(s.title_edited, 0) = 0)
                        OR s.memories_extracted_at IS NULL
                        OR s.skills_extracted_at IS NULL
                      )
                    ORDER BY s.created_at DESC
                    LIMIT 50",
                )?;

                let results: Vec<(String, usize, bool, bool, bool)> = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, usize>(1)?,
                            row.get::<_, i32>(2)? != 0,
                            row.get::<_, i32>(3)? != 0,
                            row.get::<_, i32>(4)? != 0,
                        ))
                    })?
                    .filter_map(|r| r.ok())
                    .collect();

                Ok::<_, rusqlite::Error>(results)
            })
            .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to query pending AI sessions: {}", e);
                return;
            }
        };

        if sessions.is_empty() {
            return;
        }

        tracing::info!(
            "AI recovery: found {} session(s) needing AI processing",
            sessions.len()
        );

        let mut trigger = ai::AiAutoTrigger::new(
            self.config_path.clone(),
            self.db.clone(),
            self.ai_event_tx.clone(),
            self.ai_task_queue.clone(),
        );

        for (session_id, message_count, needs_title, needs_memory, needs_skills) in sessions {
            let sid = &session_id[..8.min(session_id.len())];

            if needs_title && features.title_generation {
                tracing::info!("AI recovery: triggering title for {}", sid);
                trigger.on_session_parsed(&session_id, message_count).await;
            } else if needs_memory && features.memory_extraction {
                tracing::info!("AI recovery: triggering memory extraction for {}", sid);
                trigger.on_session_parsed(&session_id, message_count).await;
            } else if needs_skills && features.skills_discovery {
                tracing::info!("AI recovery: triggering skill extraction for {}", sid);
                trigger.on_session_parsed(&session_id, message_count).await;
            }
        }
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
            self.config_path.clone(),
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

    /// Start all enabled periodic background tasks (ranking, duplicate cleanup, embedding refresh)
    pub fn start_periodic_tasks(&self) {
        scheduler::start_scheduler(
            self.config.clone(),
            self.db.clone(),
            self.event_tx.clone(),
        );
    }

    /// Get the event sender for broadcasting events
    pub fn event_sender(&self) -> broadcast::Sender<watcher::WatcherEvent> {
        self.event_tx.clone()
    }
}

/// Derive a human-readable project name from a folder path.
///
/// Claude Code uses folder names like `-Users-yuanhao-vibedev-yolog` which is
/// the original path with `/` replaced by `-`. This function extracts the last
/// meaningful component (e.g., `yolog`).
fn derive_project_name(folder_path: &std::path::Path) -> String {
    let dir_name = folder_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Claude Code format: -Users-username-path-to-project
    // Reverse the transformation: replace `-` with `/`, parse as path, take last component
    if dir_name.starts_with('-') {
        let as_path = dir_name.replace('-', "/");
        if let Some(last) = std::path::Path::new(&as_path)
            .file_name()
            .and_then(|n| n.to_str())
        {
            return last.to_string();
        }
    }

    dir_name.to_string()
}
