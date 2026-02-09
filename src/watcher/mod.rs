//! File watcher module for monitoring session file changes
//!
//! Watches configured directories for JSONL session files,
//! parses them with the appropriate parser, and stores results in the database.

mod storage;

use crate::ai::auto_trigger::AiAutoTrigger;
use crate::ai::types::AiEvent;
use crate::ai::AiTaskQueue;
use crate::config::Config;
use crate::db::Database;
use crate::error::Result;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use storage::{incremental_parse, parse_session_file};
use tokio::sync::{broadcast, mpsc};

/// Events emitted by the file watcher and other core services
#[derive(Debug, Clone)]
pub enum WatcherEvent {
    /// New session file detected
    NewSession {
        project_id: String,
        file_path: String,
        file_name: String,
    },
    /// Existing session file changed (grew)
    SessionChanged {
        session_id: String,
        file_path: String,
        previous_size: u64,
        new_size: u64,
    },
    /// Session parsing completed
    SessionParsed {
        session_id: String,
        message_count: usize,
    },
    /// Error during processing
    Error { file_path: String, error: String },
    /// Memory ranking started
    RankingStart { project_id: String },
    /// Memory ranking completed
    RankingComplete {
        project_id: String,
        promoted: usize,
        demoted: usize,
        removed: usize,
    },
    /// Memory ranking error
    RankingError { project_id: String, error: String },
    /// Generic scheduler task started
    SchedulerTaskStart {
        task_name: String,
        project_id: String,
    },
    /// Generic scheduler task completed
    SchedulerTaskComplete {
        task_name: String,
        project_id: String,
        detail: String,
    },
    /// Generic scheduler task error
    SchedulerTaskError {
        task_name: String,
        project_id: String,
        error: String,
    },
}

/// Handle for controlling the file watcher
pub struct WatcherHandle {
    /// Shutdown signal sender
    shutdown_tx: mpsc::Sender<()>,
}

impl WatcherHandle {
    /// Stop the file watcher
    pub async fn stop(self) -> Result<()> {
        let _ = self.shutdown_tx.send(()).await;
        Ok(())
    }
}

/// A watched directory configuration
struct WatchedDirectory {
    folder_path: PathBuf,
    parser_type: String,
}

/// Internal watcher state
struct WatcherState {
    /// Watched directories by folder path
    watched: HashMap<String, WatchedDirectory>,
    /// Database connection
    db: Arc<Database>,
    /// Broadcast event sender (for SSE)
    event_tx: broadcast::Sender<WatcherEvent>,
    /// AI auto-trigger — separate from state lock to avoid blocking event processing
    ai_trigger: Arc<tokio::sync::Mutex<AiAutoTrigger>>,
}

/// Start watching configured paths for session files
pub async fn start_watcher(
    config: &Config,
    config_path: PathBuf,
    db: Arc<Database>,
    event_tx: broadcast::Sender<WatcherEvent>,
    ai_event_tx: broadcast::Sender<AiEvent>,
    ai_task_queue: AiTaskQueue,
) -> Result<WatcherHandle> {
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    let watch_paths = config.watch_paths();

    if watch_paths.is_empty() {
        tracing::info!("No project paths configured, file watcher idle");
        return Ok(WatcherHandle { shutdown_tx });
    }

    // Initialize watched directories (no pre-scan — DB stores file positions)
    let mut watched = HashMap::new();
    for (path, parser_type) in watch_paths.iter() {
        if !path.exists() || !path.is_dir() {
            tracing::warn!("Watch path does not exist: {}", path.display());
            continue;
        }

        tracing::info!("Watching {}: {}", parser_type, path.display());

        watched.insert(
            path.to_string_lossy().to_string(),
            WatchedDirectory {
                folder_path: path.clone(),
                parser_type: parser_type.clone(),
            },
        );
    }

    let ai_trigger = Arc::new(tokio::sync::Mutex::new(AiAutoTrigger::new(
        config_path,
        db.clone(),
        ai_event_tx,
        ai_task_queue,
    )));

    let state = Arc::new(tokio::sync::RwLock::new(WatcherState {
        watched,
        db,
        event_tx,
        ai_trigger: Arc::clone(&ai_trigger),
    }));

    // Create a channel to send events from notify thread to tokio runtime
    let (notify_tx, mut notify_rx) = mpsc::channel::<PathBuf>(100);

    // Create the debouncer - the callback runs in notify's thread, not tokio
    let debouncer_result = new_debouncer(
        Duration::from_millis(200),
        move |res: std::result::Result<
            Vec<notify_debouncer_mini::DebouncedEvent>,
            notify::Error,
        >| {
            if let Ok(events) = res {
                for event in events {
                    if event.kind == DebouncedEventKind::Any {
                        // Send path to tokio task via channel (non-blocking)
                        let _ = notify_tx.try_send(event.path.clone());
                    }
                }
            }
        },
    );

    // Spawn tokio task to handle events from the channel
    // Each event is spawned as its own task to prevent starvation
    // (a long-running parse of one file shouldn't block processing of other files)
    let state_for_handler = Arc::clone(&state);
    tokio::spawn(async move {
        while let Some(path) = notify_rx.recv().await {
            let state_clone = Arc::clone(&state_for_handler);
            tokio::spawn(async move {
                handle_file_event(&state_clone, &path).await;
            });
        }
    });

    let mut debouncer = match debouncer_result {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to create file watcher: {}", e);
            return Ok(WatcherHandle { shutdown_tx });
        }
    };

    // Start watching all directories
    {
        let state_guard = state.read().await;
        for dir in state_guard.watched.values() {
            if let Err(e) = debouncer
                .watcher()
                .watch(&dir.folder_path, RecursiveMode::Recursive)
            {
                tracing::error!(
                    "Failed to watch directory {}: {}",
                    dir.folder_path.display(),
                    e
                );
            }
        }
    }

    tracing::info!("File watcher started");

    // Spawn shutdown handler
    tokio::spawn(async move {
        let _ = shutdown_rx.recv().await;
        // Debouncer will be dropped when this task ends
        drop(debouncer);
        tracing::info!("File watcher stopped");
    });

    Ok(WatcherHandle { shutdown_tx })
}

/// Check if a file is a main session file (not an agent file)
fn is_session_file(path: &PathBuf) -> bool {
    let extension = path.extension().and_then(|e| e.to_str());
    let file_name = path.file_name().and_then(|n| n.to_str());

    // Must have .jsonl extension
    if extension != Some("jsonl") {
        return false;
    }

    // Skip agent files
    if let Some(name) = file_name {
        if name.starts_with("agent-") || name.contains("-agent-") {
            return false;
        }
    }

    true
}

/// Handle a file system event
async fn handle_file_event(state: &Arc<tokio::sync::RwLock<WatcherState>>, path: &PathBuf) {
    // Must be a .jsonl file
    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return;
    }

    // Skip agent files
    if !is_session_file(path) {
        return;
    }

    tracing::debug!("Processing file event: {}", path.display());

    let path_str = path.to_string_lossy().to_string();

    let file_stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => stem.to_string(),
        None => return,
    };

    // Get current file size
    let path_for_stat = path.clone();
    let new_size =
        match tokio::task::spawn_blocking(move || std::fs::metadata(&path_for_stat)).await {
            Ok(Ok(m)) => m.len(),
            _ => return, // File might have been deleted
        };

    // Read lock only — no mutation needed
    let state_guard = state.read().await;

    let watched_dir = state_guard
        .watched
        .values()
        .find(|d| path.starts_with(&d.folder_path));

    let watched_dir = match watched_dir {
        Some(d) => d,
        None => return,
    };

    let parser_type = watched_dir.parser_type.clone();
    let db = Arc::clone(&state_guard.db);
    let event_tx = state_guard.event_tx.clone();
    let ai_trigger = state_guard.ai_trigger.clone();

    // Drop read lock before DB queries and parsing
    drop(state_guard);

    // Query DB for this session's last known file_size, message_count, and max sequence
    let session_id_for_query = file_stem.clone();
    let (db_file_size, db_message_count, db_max_sequence) = db
        .with_conn(move |conn| {
            let session_info = conn
                .query_row(
                    "SELECT COALESCE(file_size, 0), COALESCE(message_count, 0) FROM sessions WHERE id = ?",
                    [&session_id_for_query],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .unwrap_or((0, 0));

            let max_seq: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(sequence_num), -1) FROM session_messages WHERE session_id = ?",
                    [&session_id_for_query],
                    |row| row.get(0),
                )
                .unwrap_or(-1);

            (session_info.0, session_info.1, max_seq)
        })
        .await;

    if new_size == db_file_size as u64 {
        return; // No change
    }

    // Emit SessionChanged for existing sessions that grew
    if db_file_size > 0 && new_size > db_file_size as u64 {
        let _ = event_tx.send(WatcherEvent::SessionChanged {
            session_id: file_stem.clone(),
            file_path: path_str.clone(),
            previous_size: db_file_size as u64,
            new_size,
        });
    }

    // Choose parse strategy
    let message_count = if new_size < db_file_size as u64 {
        // File was truncated — full re-parse
        tracing::info!("File truncated for {}, full re-parse", file_stem);
        parse_session_file(&db, &event_tx, &path_str, &file_stem, &parser_type).await
    } else if db_file_size > 0 && db_message_count > 0 {
        // Existing session with data — incremental parse (delta only)
        incremental_parse(
            &db,
            &event_tx,
            &path_str,
            &file_stem,
            &parser_type,
            db_file_size,
            db_message_count,
            db_max_sequence,
        )
        .await
    } else {
        // New session or empty — full parse
        parse_session_file(&db, &event_tx, &path_str, &file_stem, &parser_type).await
    };

    if let Some(count) = message_count {
        ai_trigger
            .lock()
            .await
            .on_session_parsed(&file_stem, count)
            .await;
    }
}
