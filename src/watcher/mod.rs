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

        // Scan existing files recursively
        fn scan_dir_recursive(dir: &PathBuf, tracked: &mut HashMap<String, TrackedFile>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let file_path = entry.path();
                    if file_path.is_dir() {
                        // Recursively scan subdirectories
                        scan_dir_recursive(&file_path, tracked);
                    } else if is_session_file(&file_path) {
                        if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
                            let size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
                            tracked.insert(
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
        }
        scan_dir_recursive(path, &mut tracked_files);

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
    let state_for_handler = Arc::clone(&state);
    tokio::spawn(async move {
        while let Some(path) = notify_rx.recv().await {
            handle_file_event(&state_for_handler, &path).await;
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

    // Skip agent files for now
    if !is_session_file(path) {
        return;
    }

    let path_str = path.to_string_lossy().to_string();

    let file_stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => stem.to_string(),
        None => return,
    };

    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name.to_string(),
        None => return,
    };

    // Get current file size - run blocking I/O in spawn_blocking
    let path_for_stat = path.clone();
    let new_size = match tokio::task::spawn_blocking(move || std::fs::metadata(&path_for_stat))
        .await
    {
        Ok(Ok(m)) => m.len(),
        _ => return, // File might have been deleted or task panicked
    };

    let mut state_guard = state.write().await;

    // Find which watched directory this file belongs to (file must be within the watched folder)
    let watched_dir = state_guard
        .watched
        .values_mut()
        .find(|d| path.starts_with(&d.folder_path));

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

            // Re-parse the file to update message count, duration, etc.
            let db = Arc::clone(&state_guard.db);
            let event_tx = state_guard.event_tx.clone();

            // Drop the lock before parsing
            drop(state_guard);

            parse_session_file(&db, &event_tx, &path_str, &file_stem, &parser_type).await;
            return; // Exit early since we dropped the lock
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
    db: &Arc<Database>,
    event_tx: &broadcast::Sender<WatcherEvent>,
    file_path: &str,
    session_id: &str,
    parser_type: &str,
) {
    let path = PathBuf::from(file_path);
    let file_path_owned = file_path.to_string();

    // Read file content - run blocking I/O in spawn_blocking
    let path_for_read = path.clone();
    let content = match tokio::task::spawn_blocking(move || std::fs::read_to_string(&path_for_read))
        .await
    {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            let _ = event_tx.send(WatcherEvent::Error {
                file_path: file_path_owned,
                error: format!("Failed to read file: {}", e),
            });
            return;
        }
        Err(_) => {
            let _ = event_tx.send(WatcherEvent::Error {
                file_path: file_path_owned,
                error: "spawn_blocking task panicked".to_string(),
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

    // Store session in database
    if let Err(e) = store_session(db, file_path, session_id, parser_type, &result).await {
        tracing::error!("Failed to store session {}: {}", session_id, e);
        let _ = event_tx.send(WatcherEvent::Error {
            file_path: file_path.to_string(),
            error: format!("Failed to store session: {}", e),
        });
        return;
    }

    // Emit success event
    let _ = event_tx.send(WatcherEvent::SessionParsed {
        session_id: session_id.to_string(),
        message_count,
    });
}

/// Store a parsed session in the database
async fn store_session(
    db: &Arc<Database>,
    file_path: &str,
    session_id: &str,
    parser_type: &str,
    result: &crate::parser::ParseResult,
) -> std::result::Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let path = PathBuf::from(file_path);

    // Get file metadata - run blocking I/O in spawn_blocking
    let path_for_stat = path.clone();
    let (file_size, file_modified) = tokio::task::spawn_blocking(move || {
        let meta = std::fs::metadata(&path_for_stat).ok();
        let size = meta.as_ref().map(|m| m.len() as i64);
        let modified = meta
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
        (size, modified)
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {}", e))?;

    // Determine AI tool name
    let ai_tool = match parser_type {
        "claude_code" | "claude-code" => "Claude Code",
        "openclaw" => "OpenClaw",
        "cursor" => "Cursor",
        _ => parser_type,
    }
    .to_string();

    // Prepare data for database operation
    let session_id = session_id.to_string();
    let session_id_for_log = session_id.clone();
    let file_path = file_path.to_string();
    let title = result.metadata.title.clone();
    let events_len = result.events.len() as i64;
    let duration_ms = result.metadata.duration_ms;
    let has_code = result.stats.has_code;
    let has_errors = result.stats.has_errors;
    let start_time = result.metadata.start_time.clone().unwrap_or_else(|| now.clone());
    let events = result.events.clone();

    // Run all database operations in spawn_blocking via with_conn
    let project_id = db
        .with_conn(move |conn| {
            use rusqlite::params;

            // Get existing project for this folder (don't auto-create)
            // If no project exists, skip storing this session
            let project_id = match get_project_for_path_sync(conn, &path) {
                Some(id) => id,
                None => {
                    // No project exists for this folder - skip silently
                    // Users must explicitly add projects they want to track
                    return Ok(None);
                }
            };

            // Insert or update session
            conn.execute(
                "INSERT INTO sessions (
                    id, project_id, file_path, title, ai_tool, message_count,
                    duration_ms, has_code, has_errors, file_size, file_modified,
                    created_at, indexed_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                ON CONFLICT(id) DO UPDATE SET
                    message_count = ?6,
                    duration_ms = ?7,
                    has_code = ?8,
                    has_errors = ?9,
                    file_size = ?10,
                    file_modified = ?11,
                    indexed_at = ?13",
                params![
                    session_id,
                    project_id,
                    file_path,
                    title,
                    ai_tool,
                    events_len,
                    duration_ms,
                    has_code,
                    has_errors,
                    file_size,
                    file_modified,
                    start_time,
                    now,
                ],
            )
            .map_err(|e| format!("Failed to insert session: {}", e))?;

            // Delete existing messages for this session (for re-indexing)
            conn.execute(
                "DELETE FROM session_messages WHERE session_id = ?",
                params![session_id],
            )
            .map_err(|e| format!("Failed to delete old messages: {}", e))?;

            // Insert messages
            for event in &events {
                conn.execute(
                    "INSERT INTO session_messages (
                        session_id, sequence_num, role, content_preview, search_content,
                        has_code, has_error, has_file_changes, tool_name, tool_type, tool_summary,
                        byte_offset, byte_length, input_tokens, output_tokens,
                        cache_read_tokens, cache_creation_tokens, model, timestamp
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
                    params![
                        session_id,
                        event.sequence as i64,
                        event.role,
                        event.content_preview,
                        event.search_content,
                        event.has_code,
                        event.has_error,
                        event.has_file_changes,
                        event.tool_name,
                        event.tool_type,
                        event.tool_summary,
                        event.byte_offset,
                        event.byte_length,
                        event.input_tokens,
                        event.output_tokens,
                        event.cache_read_tokens,
                        event.cache_creation_tokens,
                        event.model,
                        event.timestamp,
                    ],
                )
                .map_err(|e| format!("Failed to insert message {}: {}", event.sequence, e))?;
            }

            Ok::<Option<String>, String>(Some(project_id))
        })
        .await?;

    // If no project was found, the session was skipped
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(()),
    };

    tracing::info!(
        "Stored session {} with {} messages in project {}",
        session_id_for_log,
        events_len,
        project_id
    );

    Ok(())
}

/// Get an existing project for the given session file path (does NOT auto-create)
/// Returns None if no project exists for this folder - session will be skipped
fn get_project_for_path_sync(
    conn: &rusqlite::Connection,
    session_path: &PathBuf,
) -> Option<String> {
    use rusqlite::params;

    // Get parent directory as project folder
    let folder_path = session_path
        .parent()?
        .to_string_lossy()
        .to_string();

    // Check if project already exists for this folder
    conn.query_row(
        "SELECT id FROM projects WHERE folder_path = ?",
        params![folder_path],
        |row| row.get(0),
    )
    .ok()
}
