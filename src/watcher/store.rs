//! SessionStore — abstracts storage backend for the file watcher.
//!
//! Uses enum dispatch to support multiple backends without trait objects.
//! - `Db` variant — SQLite database (storage = "db")
//! - `Ephemeral` variant — in-memory index (storage = "ephemeral", added in Phase 3)

use crate::db::Database;
use crate::ephemeral::EphemeralIndex;
use crate::parser::{ParseResult, ParseStats, ParsedEvent};
use std::sync::Arc;

/// Incremental parse state for a session
pub struct SessionState {
    /// Last known file size in bytes
    pub file_size: i64,
    /// Last known message count
    pub message_count: i64,
    /// Highest sequence number stored
    pub max_sequence: i64,
}

impl Default for SessionState {
    fn default() -> Self {
        SessionState {
            file_size: 0,
            message_count: 0,
            max_sequence: -1,
        }
    }
}

/// Storage backend used by the file watcher.
///
/// Abstracts over DB vs in-memory storage so the watcher logic is identical
/// regardless of the storage mode.
pub enum SessionStore {
    /// SQLite database backend
    Db(Arc<Database>),
    /// In-memory ephemeral backend
    Ephemeral(Arc<EphemeralIndex>),
}

impl SessionStore {
    /// Get the incremental parse state for a session.
    /// Returns defaults (0, 0, -1) if the session doesn't exist yet.
    pub async fn get_session_state(&self, session_id: &str) -> SessionState {
        match self {
            SessionStore::Db(db) => super::storage::db_get_session_state(db, session_id).await,
            SessionStore::Ephemeral(idx) => {
                let (file_size, message_count, max_sequence) = idx.get_session_state(session_id);
                SessionState {
                    file_size,
                    message_count,
                    max_sequence,
                }
            }
        }
    }

    /// Store a fully-parsed session (full parse or re-parse after truncation).
    /// Returns `Ok(true)` if stored, `Ok(false)` if skipped (e.g., temp directory).
    pub async fn store_full_parse(
        &self,
        file_path: &str,
        session_id: &str,
        parser_type: &str,
        result: &ParseResult,
    ) -> Result<bool, String> {
        match self {
            SessionStore::Db(db) => {
                super::storage::db_store_session(db, file_path, session_id, parser_type, result)
                    .await
            }
            SessionStore::Ephemeral(idx) => {
                use crate::ephemeral::MessageMeta;
                use std::path::PathBuf;

                let path = PathBuf::from(file_path);
                let folder = path
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let folder_name = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                let ai_tool = match parser_type {
                    "claude_code" | "claude-code" => "Claude Code",
                    "openclaw" => "OpenClaw",
                    "cursor" => "Cursor",
                    _ => parser_type,
                };

                let project_id = idx.get_or_create_project(&folder, folder_name);
                let messages: Vec<MessageMeta> =
                    result.events.iter().map(MessageMeta::from).collect();

                let file_size = std::fs::metadata(file_path)
                    .map(|m| m.len() as i64)
                    .unwrap_or(0);

                idx.store_session(
                    session_id,
                    file_path,
                    &project_id,
                    result.metadata.title.clone(),
                    ai_tool,
                    messages,
                    file_size,
                    result.stats.has_code,
                    result.stats.has_errors,
                );

                Ok(true)
            }
        }
    }

    /// Append incrementally-parsed messages to an existing session.
    /// Returns the new total message count on success.
    #[allow(clippy::too_many_arguments)]
    pub async fn store_incremental_parse(
        &self,
        file_path: &str,
        session_id: &str,
        events: &[ParsedEvent],
        stats: &ParseStats,
        last_offset: i64,
        last_message_count: i64,
        last_max_sequence: i64,
    ) -> Result<usize, String> {
        match self {
            SessionStore::Db(db) => {
                super::storage::db_store_incremental(
                    db,
                    file_path,
                    session_id,
                    events,
                    stats,
                    last_offset,
                    last_message_count,
                    last_max_sequence,
                )
                .await
            }
            SessionStore::Ephemeral(idx) => {
                use crate::ephemeral::MessageMeta;

                let new_file_size = std::fs::metadata(file_path)
                    .map(|m| m.len() as i64)
                    .unwrap_or(0);

                let seq_base = last_max_sequence + 1;
                let byte_base = last_offset;
                let messages: Vec<MessageMeta> = events
                    .iter()
                    .map(|e| {
                        let mut m = MessageMeta::from(e);
                        m.sequence_num = seq_base + e.sequence as i64;
                        m.byte_offset = byte_base + e.byte_offset;
                        m
                    })
                    .collect();

                let total = idx.append_messages(
                    session_id,
                    messages,
                    new_file_size,
                    stats.has_code,
                    stats.has_errors,
                );

                Ok(total)
            }
        }
    }
}
