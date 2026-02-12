//! File watcher module for monitoring session file changes
//!
//! Watches configured directories for JSONL session files,
//! parses them with the appropriate parser, and stores results via SessionStore.

pub(crate) mod storage;
pub mod store;

use crate::ai::auto_trigger::AiAutoTrigger;
use crate::ai::types::AiEvent;
use crate::ai::AiTaskQueue;
use crate::config::Config;
use crate::db::Database;
use crate::error::Result;
use crate::parser::get_parser;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use store::SessionStore;
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
    /// Storage backend (DB or ephemeral)
    store: Arc<SessionStore>,
    /// Broadcast event sender (for SSE)
    event_tx: broadcast::Sender<WatcherEvent>,
    /// AI auto-trigger (None in ephemeral mode — no DB for AI tasks)
    ai_trigger: Option<Arc<tokio::sync::Mutex<AiAutoTrigger>>>,
    /// Config path (for ephemeral title generation)
    config_path: PathBuf,
    /// AI event sender (for ephemeral title generation SSE)
    ai_event_tx: broadcast::Sender<AiEvent>,
    /// AI task queue (for ephemeral title concurrency)
    ai_task_queue: AiTaskQueue,
}

/// Start watching configured paths for session files
pub async fn start_watcher(
    config: &Config,
    config_path: PathBuf,
    store: Arc<SessionStore>,
    db: Option<Arc<Database>>,
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

    // Clone before potential move into AiAutoTrigger
    let config_path_for_state = config_path.clone();
    let ai_event_tx_for_state = ai_event_tx.clone();
    let ai_task_queue_for_state = ai_task_queue.clone();

    let ai_trigger = db.map(|db| {
        Arc::new(tokio::sync::Mutex::new(AiAutoTrigger::new(
            config_path,
            db,
            ai_event_tx,
            ai_task_queue,
        )))
    });

    let state = Arc::new(tokio::sync::RwLock::new(WatcherState {
        watched,
        store,
        event_tx,
        ai_trigger: ai_trigger.clone(),
        config_path: config_path_for_state,
        ai_event_tx: ai_event_tx_for_state,
        ai_task_queue: ai_task_queue_for_state,
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
fn is_session_file(path: &Path) -> bool {
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
async fn handle_file_event(state: &Arc<tokio::sync::RwLock<WatcherState>>, path: &Path) {
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
    let path_for_stat = path.to_path_buf();
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
    let store = Arc::clone(&state_guard.store);
    let event_tx = state_guard.event_tx.clone();
    let ai_trigger = state_guard.ai_trigger.clone();
    let config_path = state_guard.config_path.clone();
    let ai_event_tx = state_guard.ai_event_tx.clone();
    let ai_task_queue = state_guard.ai_task_queue.clone();

    // Drop read lock before store queries and parsing
    drop(state_guard);

    // Query store for this session's last known state
    let session_state = store.get_session_state(&file_stem).await;
    let db_file_size = session_state.file_size;
    let db_message_count = session_state.message_count;
    let db_max_sequence = session_state.max_sequence;

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

    // Choose parse strategy and execute
    let message_count = if new_size < db_file_size as u64 {
        // File was truncated — full re-parse
        tracing::info!("File truncated for {}, full re-parse", file_stem);
        full_parse(&store, &event_tx, &path_str, &file_stem, &parser_type).await
    } else if db_file_size > 0 && db_message_count > 0 {
        // Existing session with data — incremental parse (delta only)
        incremental_parse(
            &store,
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
        full_parse(&store, &event_tx, &path_str, &file_stem, &parser_type).await
    };

    if let (Some(count), Some(trigger)) = (message_count, &ai_trigger) {
        trigger
            .lock()
            .await
            .on_session_parsed(&file_stem, count)
            .await;
    }

    // Ephemeral-mode title generation (no DB needed)
    if let (Some(count), None) = (message_count, &ai_trigger) {
        if count >= 49 {
            if let SessionStore::Ephemeral(idx) = store.as_ref() {
                maybe_trigger_ephemeral_title(
                    idx,
                    &file_stem,
                    &config_path,
                    &ai_event_tx,
                    &ai_task_queue,
                )
                .await;
            }
        }
    }
}

/// Trigger title generation for an ephemeral session if conditions are met.
async fn maybe_trigger_ephemeral_title(
    idx: &Arc<crate::ephemeral::EphemeralIndex>,
    session_id: &str,
    config_path: &Path,
    ai_event_tx: &broadcast::Sender<AiEvent>,
    ai_task_queue: &AiTaskQueue,
) {
    if idx.has_title(session_id) {
        return;
    }

    let config = match Config::from_file(config_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("Ephemeral title: config read failed: {}", e);
            return;
        }
    };
    if !config.is_ai_active()
        || !config.is_feature_active(crate::config::AiFeature::TitleGeneration)
    {
        tracing::debug!("Ephemeral title: AI not active or title feature disabled");
        return;
    }

    let first_messages = match idx.get_first_user_messages(session_id, 10, 4000) {
        Some(m) => m,
        None => {
            tracing::debug!(
                "Ephemeral title: no user messages found for {}",
                &session_id[..8]
            );
            return;
        }
    };
    tracing::info!(
        "Ephemeral title: triggering for {} ({} chars)",
        &session_id[..8],
        first_messages.len()
    );

    let permit = match ai_task_queue.acquire().await {
        Ok(p) => p,
        Err(_) => return,
    };

    // Mark as generated before spawning to prevent duplicate triggers
    idx.set_title_generated(session_id);

    let idx = idx.clone();
    let sid = session_id.to_string();
    let tx = ai_event_tx.clone();

    tokio::spawn(async move {
        let _permit = permit;
        let _ = tx.send(AiEvent::TitleStart {
            session_id: sid.clone(),
        });

        let result = crate::ai::title::generate_title_from_text(&sid, &first_messages, None).await;

        if let Some(ref title) = result.title {
            idx.update_session(&sid, Some(title.clone()), None);
            idx.set_title_generated(&sid);
            tracing::info!("Ephemeral title generated for {}", &sid[..8]);
            let _ = tx.send(AiEvent::TitleComplete {
                session_id: sid,
                title: title.clone(),
            });
        } else if let Some(error) = result.error {
            tracing::warn!("Ephemeral title failed for {}: {}", &sid[..8], error);
            let _ = tx.send(AiEvent::TitleError {
                session_id: sid,
                error,
            });
        }
    });
}

/// Read and parse a full session file, then store via SessionStore.
/// Returns Some(message_count) on success, None on failure.
async fn full_parse(
    store: &SessionStore,
    event_tx: &broadcast::Sender<WatcherEvent>,
    file_path: &str,
    session_id: &str,
    parser_type: &str,
) -> Option<usize> {
    let file_path_owned = file_path.to_string();

    // Read file content
    let path_for_read = PathBuf::from(file_path);
    let content =
        match tokio::task::spawn_blocking(move || std::fs::read_to_string(&path_for_read)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                let _ = event_tx.send(WatcherEvent::Error {
                    file_path: file_path_owned,
                    error: format!("Failed to read file: {}", e),
                });
                return None;
            }
            Err(_) => {
                let _ = event_tx.send(WatcherEvent::Error {
                    file_path: file_path_owned,
                    error: "spawn_blocking task panicked".to_string(),
                });
                return None;
            }
        };

    // Parse
    let parser = match get_parser(parser_type) {
        Some(p) => p,
        None => {
            tracing::warn!("Unknown parser type: {}", parser_type);
            return None;
        }
    };

    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let result = parser.parse(&lines);
    let message_count = result.events.len();

    tracing::info!("Parsed session {}: {} messages", session_id, message_count);

    // Store via SessionStore
    match store
        .store_full_parse(file_path, session_id, parser_type, &result)
        .await
    {
        Ok(true) => {
            let _ = event_tx.send(WatcherEvent::SessionParsed {
                session_id: session_id.to_string(),
                message_count,
            });
            Some(message_count)
        }
        Ok(false) => {
            tracing::debug!("Skipped session {} - no matching project", session_id);
            None
        }
        Err(e) => {
            tracing::error!("Failed to store session {}: {}", session_id, e);
            let _ = event_tx.send(WatcherEvent::Error {
                file_path: file_path.to_string(),
                error: format!("Failed to store session: {}", e),
            });
            None
        }
    }
}

/// Read and parse only new bytes appended to a session file, then store via SessionStore.
/// Returns Some(total_message_count) on success, None on failure.
#[allow(clippy::too_many_arguments)]
async fn incremental_parse(
    store: &SessionStore,
    event_tx: &broadcast::Sender<WatcherEvent>,
    file_path: &str,
    session_id: &str,
    parser_type: &str,
    last_offset: i64,
    last_message_count: i64,
    last_max_sequence: i64,
) -> Option<usize> {
    use std::io::{Read, Seek, SeekFrom};

    let file_path_owned = file_path.to_string();
    let offset = last_offset as u64;

    // Read only new bytes from the file
    let path_for_read = PathBuf::from(file_path);
    let new_content = match tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path_for_read)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        Ok::<String, std::io::Error>(buf)
    })
    .await
    {
        Ok(Ok(c)) if !c.is_empty() => c,
        Ok(Ok(_)) => return None,
        Ok(Err(e)) => {
            let _ = event_tx.send(WatcherEvent::Error {
                file_path: file_path_owned,
                error: format!("Failed to read file delta: {}", e),
            });
            return None;
        }
        Err(_) => return None,
    };

    // Parse new lines
    let parser = match get_parser(parser_type) {
        Some(p) => p,
        None => return None,
    };

    let lines: Vec<String> = new_content.lines().map(|l| l.to_string()).collect();
    let result = parser.parse(&lines);

    if result.events.is_empty() {
        return None;
    }

    tracing::info!(
        "Incremental: parsed {} new messages for {} (total: {})",
        result.events.len(),
        session_id,
        last_message_count as usize + result.events.len()
    );

    // Store via SessionStore
    match store
        .store_incremental_parse(
            file_path,
            session_id,
            &result.events,
            &result.stats,
            last_offset,
            last_message_count,
            last_max_sequence,
        )
        .await
    {
        Ok(total) => {
            let _ = event_tx.send(WatcherEvent::SessionParsed {
                session_id: session_id.to_string(),
                message_count: total,
            });
            Some(total)
        }
        Err(e) => {
            tracing::error!(
                "Failed to store incremental parse for {}: {}",
                session_id,
                e
            );
            None
        }
    }
}
