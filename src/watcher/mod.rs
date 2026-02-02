//! File watcher module for monitoring session file changes
//!
//! Watches configured directories for JSONL session files,
//! parses them with the appropriate parser, and stores results in the database.

use crate::config::Config;
use crate::db::Database;
use crate::error::Result;
use crate::parser::{get_parser, SessionParser};
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};

/// Events emitted by the file watcher
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

/// Tracked file info within a watched directory
#[derive(Debug, Clone)]
struct TrackedFile {
    path: PathBuf,
    last_known_size: u64,
}

/// State for tracking a watched project directory
struct WatchedDirectory {
    project_id: String,
    folder_path: PathBuf,
    parser_type: String,
    /// Map of file stem -> tracked file info
    tracked_files: HashMap<String, TrackedFile>,
}

/// Internal watcher state
struct WatcherState {
    /// Watched directories by project_id
    watched: HashMap<String, WatchedDirectory>,
    /// Database connection
    db: Arc<Database>,
    /// Broadcast event sender (for SSE)
    event_tx: broadcast::Sender<WatcherEvent>,
}

/// Start watching configured paths for session files
pub async fn start_watcher(
    config: &Config,
    db: Arc<Database>,
    event_tx: broadcast::Sender<WatcherEvent>,
) -> Result<WatcherHandle> {
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    let watch_paths = config.watch_paths();

    if watch_paths.is_empty() {
        tracing::info!("No watch paths configured, file watcher idle");
        return Ok(WatcherHandle { shutdown_tx });
    }

    // Initialize watched directories
    let mut watched = HashMap::new();
    for (i, (path, parser_type)) in watch_paths.iter().enumerate() {
        if !path.exists() || !path.is_dir() {
            tracing::warn!("Watch path does not exist: {}", path.display());
            continue;
        }

        let project_id = format!("watch_{}", i);
        let mut tracked_files = HashMap::new();

        // Scan existing files
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if is_session_file(&file_path) {
                    if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
                        let size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
                        tracked_files.insert(
                            stem.to_string(),
                            TrackedFile {
                                path: file_path,
                                last_known_size: size,
                            },
                        );
                    }
                }
            }
        }

        let file_count = tracked_files.len();
        tracing::info!(
            "Watching {} ({} existing files): {}",
            parser_type,
            file_count,
            path.display()
        );

        watched.insert(
            project_id.clone(),
            WatchedDirectory {
                project_id,
                folder_path: path.clone(),
                parser_type: parser_type.clone(),
                tracked_files,
            },
        );
    }

    let state = Arc::new(tokio::sync::RwLock::new(WatcherState {
        watched,
        db,
        event_tx,
    }));

    // Create the debouncer
    let state_clone = Arc::clone(&state);
    let debouncer_result = new_debouncer(
        Duration::from_millis(200),
        move |res: std::result::Result<
            Vec<notify_debouncer_mini::DebouncedEvent>,
            notify::Error,
        >| {
            if let Ok(events) = res {
                let state = Arc::clone(&state_clone);
                // Spawn async handler
                tokio::spawn(async move {
                    for event in events {
                        if event.kind == DebouncedEventKind::Any {
                            handle_file_event(&state, &event.path).await;
                        }
                    }
                });
            }
        },
    );

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

    // Skip agent files for now
    if !is_session_file(path) {
        return;
    }

    let path_str = path.to_string_lossy().to_string();

    // Get current file size
    let new_size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return, // File might have been deleted
    };

    let file_stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => stem.to_string(),
        None => return,
    };

    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name.to_string(),
        None => return,
    };

    let parent_path = path.parent();

    let mut state_guard = state.write().await;

    // Find which watched directory this file belongs to
    let watched_dir = state_guard
        .watched
        .values_mut()
        .find(|d| parent_path == Some(&d.folder_path));

    let watched_dir = match watched_dir {
        Some(d) => d,
        None => return,
    };

    let project_id = watched_dir.project_id.clone();
    let parser_type = watched_dir.parser_type.clone();

    // Check if this is an existing tracked file or a new one
    if let Some(tracked) = watched_dir.tracked_files.get_mut(&file_stem) {
        let previous_size = tracked.last_known_size;

        if new_size > previous_size {
            // File grew - update tracking and emit event
            tracked.last_known_size = new_size;

            let event = WatcherEvent::SessionChanged {
                session_id: file_stem.clone(),
                file_path: path_str.clone(),
                previous_size,
                new_size,
            };

            let _ = state_guard.event_tx.send(event);
            tracing::debug!(
                "Session file changed: {} ({} -> {} bytes)",
                file_stem,
                previous_size,
                new_size
            );
        }
    } else {
        // New file - track it
        watched_dir.tracked_files.insert(
            file_stem.clone(),
            TrackedFile {
                path: path.clone(),
                last_known_size: new_size,
            },
        );

        let event = WatcherEvent::NewSession {
            project_id: project_id.clone(),
            file_path: path_str.clone(),
            file_name: file_name.clone(),
        };

        let _ = state_guard.event_tx.send(event);
        tracing::info!("New session detected: {} in {}", file_name, project_id);

        // Parse the new file
        let db = Arc::clone(&state_guard.db);
        let event_tx = state_guard.event_tx.clone();

        // Drop the lock before parsing
        drop(state_guard);

        parse_session_file(&db, &event_tx, &path_str, &file_stem, &parser_type).await;
    }
}

/// Parse a session file and store in database
async fn parse_session_file(
    _db: &Arc<Database>,
    event_tx: &broadcast::Sender<WatcherEvent>,
    file_path: &str,
    session_id: &str,
    parser_type: &str,
) {
    let path = PathBuf::from(file_path);

    // Read file content
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            let _ = event_tx.send(WatcherEvent::Error {
                file_path: file_path.to_string(),
                error: format!("Failed to read file: {}", e),
            });
            return;
        }
    };

    // Get parser for this type
    let parser = match get_parser(parser_type) {
        Some(p) => p,
        None => {
            tracing::warn!("Unknown parser type: {}", parser_type);
            return;
        }
    };

    // Split content into lines
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    // Parse the file
    let result = parser.parse(&lines);

    let message_count = result.events.len();
    tracing::info!("Parsed session {}: {} messages", session_id, message_count);

    // Store in database (TODO: implement session storage)
    // For now, just emit the event
    let _ = event_tx.send(WatcherEvent::SessionParsed {
        session_id: session_id.to_string(),
        message_count,
    });
}
