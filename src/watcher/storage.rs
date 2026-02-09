//! Session parsing and storage functions for the file watcher.
//!
//! Handles full parsing, incremental (delta) parsing, and database storage.

use crate::db::Database;
use crate::parser::get_parser;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;

use super::WatcherEvent;

/// Parse only new bytes appended to a session file (delta parse).
/// Seeks to `last_offset`, reads remaining content, parses new lines, appends to DB.
/// Returns Some(total_message_count) on success, None on failure.
#[allow(clippy::too_many_arguments)]
pub(super) async fn incremental_parse(
    db: &Arc<Database>,
    event_tx: &broadcast::Sender<WatcherEvent>,
    file_path: &str,
    session_id: &str,
    parser_type: &str,
    last_offset: i64,
    last_message_count: i64,
    last_max_sequence: i64,
) -> Option<usize> {
    use std::io::{Read, Seek, SeekFrom};

    let path = PathBuf::from(file_path);
    let file_path_owned = file_path.to_string();
    let offset = last_offset as u64;

    // Read only new bytes from the file
    let path_for_read = path.clone();
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
        Ok(Ok(_)) => return None, // No new content
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
    let new_event_count = result.events.len();

    if new_event_count == 0 {
        return None;
    }

    let total_message_count = last_message_count as usize + new_event_count;

    tracing::info!(
        "Incremental: parsed {} new messages for {} (total: {})",
        new_event_count,
        session_id,
        total_message_count
    );

    // Get current file metadata
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
    .unwrap_or((None, None));

    // Append to database
    let session_id_owned = session_id.to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let byte_base = last_offset;
    let seq_base = last_max_sequence + 1;
    let events = result.events.clone();
    let has_code = result.stats.has_code;
    let has_errors = result.stats.has_errors;

    let stored = db
        .with_conn(move |conn| {
            use rusqlite::params;

            // Update session metadata (incremental — merge flags, update counts)
            conn.execute(
                "UPDATE sessions SET
                    message_count = ?1,
                    file_size = ?2,
                    file_modified = ?3,
                    has_code = has_code OR ?4,
                    has_errors = has_errors OR ?5,
                    indexed_at = ?6
                WHERE id = ?7",
                params![
                    total_message_count as i64,
                    file_size,
                    file_modified,
                    has_code,
                    has_errors,
                    now,
                    session_id_owned,
                ],
            )
            .map_err(|e| format!("Failed to update session: {}", e))?;

            // Append new messages with adjusted sequence and byte_offset
            for event in &events {
                let adjusted_seq = seq_base + event.sequence as i64;
                let adjusted_offset = byte_base + event.byte_offset;

                conn.execute(
                    "INSERT OR IGNORE INTO session_messages (
                        session_id, sequence_num, role, content_preview, search_content,
                        has_code, has_error, has_file_changes, tool_name, tool_type, tool_summary,
                        byte_offset, byte_length, input_tokens, output_tokens,
                        cache_read_tokens, cache_creation_tokens, model, timestamp
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
                    params![
                        session_id_owned,
                        adjusted_seq,
                        event.role,
                        event.content_preview,
                        event.search_content,
                        event.has_code,
                        event.has_error,
                        event.has_file_changes,
                        event.tool_name,
                        event.tool_type,
                        event.tool_summary,
                        adjusted_offset,
                        event.byte_length,
                        event.input_tokens,
                        event.output_tokens,
                        event.cache_read_tokens,
                        event.cache_creation_tokens,
                        event.model,
                        event.timestamp,
                    ],
                )
                .map_err(|e| format!("Failed to insert message: {}", e))?;
            }

            Ok::<bool, String>(true)
        })
        .await;

    match stored {
        Ok(true) => {
            let _ = event_tx.send(WatcherEvent::SessionParsed {
                session_id: session_id.to_string(),
                message_count: total_message_count,
            });
            Some(total_message_count)
        }
        _ => {
            tracing::error!("Failed to store incremental parse for {}", session_id);
            None
        }
    }
}

/// Parse a session file, store in database, and return message count on success.
/// Returns Some(message_count) if session was stored, None otherwise.
pub(super) async fn parse_session_file(
    db: &Arc<Database>,
    event_tx: &broadcast::Sender<WatcherEvent>,
    file_path: &str,
    session_id: &str,
    parser_type: &str,
) -> Option<usize> {
    let path = PathBuf::from(file_path);
    let file_path_owned = file_path.to_string();

    // Read file content - run blocking I/O in spawn_blocking
    let path_for_read = path.clone();
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

    // Get parser for this type
    let parser = match get_parser(parser_type) {
        Some(p) => p,
        None => {
            tracing::warn!("Unknown parser type: {}", parser_type);
            return None;
        }
    };

    // Split content into lines
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    // Parse the file
    let result = parser.parse(&lines);

    let message_count = result.events.len();
    tracing::info!("Parsed session {}: {} messages", session_id, message_count);

    // Store session in database
    match store_session(db, file_path, session_id, parser_type, &result).await {
        Ok(true) => {
            // Session was stored - emit success event
            let _ = event_tx.send(WatcherEvent::SessionParsed {
                session_id: session_id.to_string(),
                message_count,
            });
            Some(message_count)
        }
        Ok(false) => {
            // Session was skipped (no matching project) - don't emit event
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

/// Store a parsed session in the database.
/// Returns Ok(true) if stored, Ok(false) if skipped (no matching project), Err on failure.
async fn store_session(
    db: &Arc<Database>,
    file_path: &str,
    session_id: &str,
    parser_type: &str,
    result: &crate::parser::ParseResult,
) -> std::result::Result<bool, String> {
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
    let start_time = result
        .metadata
        .start_time
        .clone()
        .unwrap_or_else(|| now.clone());
    let events = result.events.clone();

    // Run all database operations in spawn_blocking via with_conn
    let project_id = db
        .with_conn(move |conn| {
            use rusqlite::params;

            // Get or create project for this folder
            let project_id = match get_or_create_project_for_path_sync(conn, &path) {
                Some(id) => id,
                None => {
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
                    ai_tool = ?5,
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
        None => return Ok(false), // Session skipped - no matching project
    };

    tracing::info!(
        "Stored session {} with {} messages in project {}",
        session_id_for_log,
        events_len,
        project_id
    );

    Ok(true) // Session was stored
}

/// Check if a Claude Code folder name encodes a temp/system directory path.
fn is_temp_directory(folder_name: &str) -> bool {
    if folder_name == "-" {
        return true;
    }
    if !folder_name.starts_with('-') {
        return false;
    }
    let lower = folder_name[1..].to_lowercase();
    lower.starts_with("private-var-folders-")
        || lower.starts_with("var-folders-")
        || lower.starts_with("tmp-")
        || lower.starts_with("private-tmp-")
        || lower == "tmp"
}

/// Get or create a project for the given session file path.
/// If no project exists for this folder, auto-creates one with a derived name.
fn get_or_create_project_for_path_sync(
    conn: &rusqlite::Connection,
    session_path: &Path,
) -> Option<String> {
    use rusqlite::params;

    let folder = session_path.parent()?;
    let folder_path = folder.to_string_lossy().to_string();

    // Skip temp/system directories — not real projects
    let folder_name = folder.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if is_temp_directory(folder_name) {
        tracing::debug!("Skipping temp directory: {}", folder_path);
        return None;
    }

    // Try existing project first
    if let Ok(id) = conn.query_row(
        "SELECT id FROM projects WHERE folder_path = ?",
        params![folder_path],
        |row| row.get::<_, String>(0),
    ) {
        return Some(id);
    }

    // Auto-create: derive name from folder path
    let name = crate::derive_project_name(folder);
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO projects (id, name, folder_path, auto_sync, created_at, updated_at)
         VALUES (?, ?, ?, 1, datetime('now'), datetime('now'))",
        params![id, name, folder_path],
    )
    .ok()?;

    tracing::info!("Auto-created project '{}' for {}", name, folder_path);
    Some(id)
}
