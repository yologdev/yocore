//! Database storage functions for the file watcher.
//!
//! These functions implement the DB-specific operations that `SessionStore::Db` delegates to.
//! They handle project lookup/creation, session upsert, and message insertion in SQLite.

use super::store::SessionState;
use crate::db::Database;
use crate::parser::{ParseResult, ParseStats, ParsedEvent};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Get incremental parse state for a session from the database.
pub(super) async fn db_get_session_state(db: &Arc<Database>, session_id: &str) -> SessionState {
    let sid = session_id.to_string();
    db.with_conn(move |conn| {
        let (file_size, message_count) = conn
            .query_row(
                "SELECT COALESCE(file_size, 0), COALESCE(message_count, 0) FROM sessions WHERE id = ?",
                [&sid],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap_or((0, 0));

        let max_sequence: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence_num), -1) FROM session_messages WHERE session_id = ?",
                [&sid],
                |row| row.get(0),
            )
            .unwrap_or(-1);

        SessionState {
            file_size,
            message_count,
            max_sequence,
        }
    })
    .await
}

/// Store a fully-parsed session in the database.
/// Returns Ok(true) if stored, Ok(false) if skipped (no matching project), Err on failure.
pub(super) async fn db_store_session(
    db: &Arc<Database>,
    file_path: &str,
    session_id: &str,
    parser_type: &str,
    result: &ParseResult,
) -> Result<bool, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let path = PathBuf::from(file_path);

    // Get file metadata
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

    let project_id = db
        .with_conn(move |conn| {
            use rusqlite::params;

            let project_id = match get_or_create_project_for_path_sync(conn, &path) {
                Some(id) => id,
                None => {
                    return Ok(None);
                }
            };

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

            conn.execute(
                "DELETE FROM session_messages WHERE session_id = ?",
                params![session_id],
            )
            .map_err(|e| format!("Failed to delete old messages: {}", e))?;

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

    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(false),
    };

    tracing::info!(
        "Stored session {} with {} messages in project {}",
        session_id_for_log,
        events_len,
        project_id
    );

    Ok(true)
}

/// Store incrementally-parsed messages in the database.
/// Returns the new total message count on success.
#[allow(clippy::too_many_arguments)]
pub(super) async fn db_store_incremental(
    db: &Arc<Database>,
    file_path: &str,
    session_id: &str,
    events: &[ParsedEvent],
    stats: &ParseStats,
    last_offset: i64,
    last_message_count: i64,
    last_max_sequence: i64,
) -> Result<usize, String> {
    let path = PathBuf::from(file_path);
    let total_message_count = last_message_count as usize + events.len();

    // Get current file metadata
    let path_for_stat = path;
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

    let session_id_owned = session_id.to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let byte_base = last_offset;
    let seq_base = last_max_sequence + 1;
    let events = events.to_vec();
    let has_code = stats.has_code;
    let has_errors = stats.has_errors;

    db.with_conn(move |conn| {
        use rusqlite::params;

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

        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("DB error: {}", e))?;

    Ok(total_message_count)
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

    let folder_name = folder.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if is_temp_directory(folder_name) {
        tracing::debug!("Skipping temp directory: {}", folder_path);
        return None;
    }

    if let Ok(id) = conn.query_row(
        "SELECT id FROM projects WHERE folder_path = ?",
        params![folder_path],
        |row| row.get::<_, String>(0),
    ) {
        return Some(id);
    }

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
