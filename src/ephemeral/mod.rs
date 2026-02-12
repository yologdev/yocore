//! Ephemeral in-memory storage backend.
//!
//! Provides volatile session/message storage when `storage = "ephemeral"`.
//! All data is lost on restart. Uses LRU eviction to bound memory usage.

use crate::config::EphemeralConfig;
use crate::parser::ParsedEvent;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

/// In-memory project metadata
#[derive(Debug, Clone)]
pub struct ProjectMeta {
    pub id: String,
    pub name: String,
    pub folder_path: String,
    pub created_at: String,
}

/// In-memory session metadata
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id: String,
    pub project_id: String,
    pub file_path: String,
    pub title: Option<String>,
    pub ai_tool: String,
    pub message_count: usize,
    pub file_size: i64,
    pub has_code: bool,
    pub has_errors: bool,
    pub is_hidden: bool,
    pub title_generated: bool,
    pub created_at: String,
    /// For LRU eviction
    last_accessed: Instant,
}

/// In-memory message metadata (mirrors session_messages schema)
#[derive(Debug, Clone)]
pub struct MessageMeta {
    pub sequence_num: i64,
    pub role: String,
    pub content_preview: Option<String>,
    pub has_code: bool,
    pub has_error: bool,
    pub has_file_changes: bool,
    pub tool_name: Option<String>,
    pub tool_type: Option<String>,
    pub tool_summary: Option<String>,
    pub byte_offset: i64,
    pub byte_length: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub model: Option<String>,
    pub timestamp: String,
}

impl From<&ParsedEvent> for MessageMeta {
    fn from(e: &ParsedEvent) -> Self {
        MessageMeta {
            sequence_num: e.sequence as i64,
            role: e.role.clone(),
            content_preview: Some(e.content_preview.clone()),
            has_code: e.has_code,
            has_error: e.has_error,
            has_file_changes: e.has_file_changes,
            tool_name: e.tool_name.clone(),
            tool_type: e.tool_type.clone(),
            tool_summary: e.tool_summary.clone(),
            byte_offset: e.byte_offset,
            byte_length: e.byte_length,
            input_tokens: e.input_tokens,
            output_tokens: e.output_tokens,
            cache_read_tokens: e.cache_read_tokens,
            cache_creation_tokens: e.cache_creation_tokens,
            model: e.model.clone(),
            timestamp: e.timestamp.clone(),
        }
    }
}

/// In-memory volatile index for ephemeral storage mode.
///
/// Thread-safe via `RwLock`. Uses LRU eviction on sessions when `max_sessions` is exceeded.
pub struct EphemeralIndex {
    projects: RwLock<HashMap<String, ProjectMeta>>,
    /// Maps folder_path → project_id for lookup
    folder_to_project: RwLock<HashMap<String, String>>,
    sessions: RwLock<HashMap<String, SessionMeta>>,
    messages: RwLock<HashMap<String, Vec<MessageMeta>>>,
    config: EphemeralConfig,
}

impl EphemeralIndex {
    pub fn new(config: EphemeralConfig) -> Self {
        EphemeralIndex {
            projects: RwLock::new(HashMap::new()),
            folder_to_project: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            messages: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Get or create a project for the given folder path.
    /// Returns the project ID.
    pub fn get_or_create_project(&self, folder_path: &str, name: &str) -> String {
        // Check existing
        {
            let lookup = self.folder_to_project.read().unwrap();
            if let Some(id) = lookup.get(folder_path) {
                return id.clone();
            }
        }

        // Create new
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let project = ProjectMeta {
            id: id.clone(),
            name: name.to_string(),
            folder_path: folder_path.to_string(),
            created_at: now,
        };

        self.projects.write().unwrap().insert(id.clone(), project);
        self.folder_to_project
            .write()
            .unwrap()
            .insert(folder_path.to_string(), id.clone());

        tracing::info!(
            "Ephemeral: auto-created project '{}' for {}",
            name,
            folder_path
        );

        id
    }

    /// Get session state for incremental parsing
    pub fn get_session_state(&self, session_id: &str) -> (i64, i64, i64) {
        let sessions = self.sessions.read().unwrap();
        let session = match sessions.get(session_id) {
            Some(s) => s,
            None => return (0, 0, -1),
        };

        let file_size = session.file_size;
        let message_count = session.message_count as i64;

        let messages = self.messages.read().unwrap();
        let max_seq = messages
            .get(session_id)
            .and_then(|msgs| msgs.iter().map(|m| m.sequence_num).max())
            .unwrap_or(-1);

        (file_size, message_count, max_seq)
    }

    /// Store or update a session and its messages (full parse).
    /// Returns the project_id if stored, None if skipped.
    #[allow(clippy::too_many_arguments)]
    pub fn store_session(
        &self,
        session_id: &str,
        file_path: &str,
        project_id: &str,
        title: Option<String>,
        ai_tool: &str,
        messages: Vec<MessageMeta>,
        file_size: i64,
        has_code: bool,
        has_errors: bool,
    ) {
        self.evict_if_needed(session_id);

        let now = chrono::Utc::now().to_rfc3339();
        let message_count = messages.len();

        let session = SessionMeta {
            id: session_id.to_string(),
            project_id: project_id.to_string(),
            file_path: file_path.to_string(),
            title,
            ai_tool: ai_tool.to_string(),
            message_count,
            file_size,
            has_code,
            has_errors,
            is_hidden: false,
            title_generated: false,
            created_at: now,
            last_accessed: Instant::now(),
        };

        self.sessions
            .write()
            .unwrap()
            .insert(session_id.to_string(), session);
        // Keep only the last N messages from full parse to save memory.
        // Older messages can still be read from the JSONL file via byte offsets.
        let tail_size = self.config.max_messages_per_session;
        let trimmed = if messages.len() > tail_size {
            let skip = messages.len() - tail_size;
            messages.into_iter().skip(skip).collect()
        } else {
            messages
        };
        self.messages
            .write()
            .unwrap()
            .insert(session_id.to_string(), trimmed);
    }

    /// Append messages from incremental parse
    pub fn append_messages(
        &self,
        session_id: &str,
        new_messages: Vec<MessageMeta>,
        new_file_size: i64,
        has_code: bool,
        has_errors: bool,
    ) -> usize {
        // Update session metadata
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.get_mut(session_id) {
            session.message_count += new_messages.len();
            session.file_size = new_file_size;
            session.has_code = session.has_code || has_code;
            session.has_errors = session.has_errors || has_errors;
            session.last_accessed = Instant::now();
        }
        let total = sessions
            .get(session_id)
            .map(|s| s.message_count)
            .unwrap_or(0);
        drop(sessions);

        // Append new messages (incremental updates are small, no cap needed)
        let mut messages = self.messages.write().unwrap();
        let msgs = messages.entry(session_id.to_string()).or_default();
        msgs.extend(new_messages);

        total
    }

    // ========================================================================
    // Query methods (for API routes in ephemeral mode)
    // ========================================================================

    /// List all projects, ordered by name.
    pub fn list_projects(&self) -> Vec<ProjectMeta> {
        let projects = self.projects.read().unwrap();
        let mut result: Vec<ProjectMeta> = projects.values().cloned().collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    /// Get a single project by ID.
    pub fn get_project(&self, project_id: &str) -> Option<ProjectMeta> {
        self.projects.read().unwrap().get(project_id).cloned()
    }

    /// Resolve a project by folder path.
    pub fn resolve_project_by_folder(&self, folder_path: &str) -> Option<ProjectMeta> {
        let lookup = self.folder_to_project.read().unwrap();
        let project_id = lookup.get(folder_path)?;
        self.projects.read().unwrap().get(project_id).cloned()
    }

    /// List sessions, optionally filtered by project_id.
    /// By default, hidden sessions are excluded unless `include_hidden` is true.
    pub fn list_sessions(&self, project_id: Option<&str>) -> Vec<SessionMeta> {
        self.list_sessions_filtered(project_id, false)
    }

    /// List sessions with optional hidden filter.
    pub fn list_sessions_filtered(
        &self,
        project_id: Option<&str>,
        include_hidden: bool,
    ) -> Vec<SessionMeta> {
        let sessions = self.sessions.read().unwrap();
        let mut result: Vec<SessionMeta> = sessions
            .values()
            .filter(|s| project_id.is_none_or(|pid| s.project_id == pid))
            .filter(|s| include_hidden || !s.is_hidden)
            .cloned()
            .collect();
        // Sort by created_at descending
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        result
    }

    /// Get a single session by ID. Updates last_accessed for LRU.
    pub fn get_session(&self, session_id: &str) -> Option<SessionMeta> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.get_mut(session_id) {
            session.last_accessed = Instant::now();
            Some(session.clone())
        } else {
            None
        }
    }

    /// Get messages for a session.
    pub fn get_messages(&self, session_id: &str) -> Vec<MessageMeta> {
        self.messages
            .read()
            .unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get a single message by session_id and sequence number.
    pub fn get_message(&self, session_id: &str, sequence_num: i64) -> Option<MessageMeta> {
        let messages = self.messages.read().unwrap();
        messages.get(session_id).and_then(|msgs| {
            msgs.iter()
                .find(|m| m.sequence_num == sequence_num)
                .cloned()
        })
    }

    // ========================================================================
    // Mutation methods (for API routes in ephemeral mode)
    // ========================================================================

    /// Update session title and/or visibility. Returns true if found.
    pub fn update_session(
        &self,
        session_id: &str,
        title: Option<String>,
        is_hidden: Option<bool>,
    ) -> bool {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.get_mut(session_id) {
            if let Some(t) = title {
                session.title = Some(t);
            }
            if let Some(h) = is_hidden {
                session.is_hidden = h;
            }
            session.last_accessed = Instant::now();
            true
        } else {
            false
        }
    }

    /// Delete a session and its messages. Returns true if found.
    pub fn delete_session(&self, session_id: &str) -> bool {
        let removed = self.sessions.write().unwrap().remove(session_id).is_some();
        if removed {
            self.messages.write().unwrap().remove(session_id);
        }
        removed
    }

    /// Update a project's name. Returns true if found.
    pub fn update_project(&self, project_id: &str, name: Option<String>) -> bool {
        let mut projects = self.projects.write().unwrap();
        if let Some(project) = projects.get_mut(project_id) {
            if let Some(n) = name {
                project.name = n;
            }
            true
        } else {
            false
        }
    }

    /// Delete a project and all its sessions/messages. Returns true if found.
    pub fn delete_project(&self, project_id: &str) -> bool {
        let removed = self.projects.write().unwrap().remove(project_id).is_some();
        if removed {
            // Remove folder_path mapping
            self.folder_to_project
                .write()
                .unwrap()
                .retain(|_, pid| pid != project_id);
            // Remove all sessions for this project
            let session_ids: Vec<String> = self
                .sessions
                .read()
                .unwrap()
                .values()
                .filter(|s| s.project_id == project_id)
                .map(|s| s.id.clone())
                .collect();
            let mut sessions = self.sessions.write().unwrap();
            let mut messages = self.messages.write().unwrap();
            for sid in session_ids {
                sessions.remove(&sid);
                messages.remove(&sid);
            }
        }
        removed
    }

    /// Count total sessions (for session limit info).
    pub fn session_count(&self) -> usize {
        self.sessions.read().unwrap().len()
    }

    /// Check if a session already has an AI-generated title.
    pub fn has_title(&self, session_id: &str) -> bool {
        self.sessions
            .read()
            .unwrap()
            .get(session_id)
            .map(|s| s.title_generated)
            .unwrap_or(false)
    }

    /// Mark a session's title as AI-generated.
    pub fn set_title_generated(&self, session_id: &str) {
        if let Some(session) = self.sessions.write().unwrap().get_mut(session_id) {
            session.title_generated = true;
        }
    }

    /// Get first N user messages from the JSONL file for title generation.
    /// Reads directly from disk since the in-memory index may only hold recent messages.
    pub fn get_first_user_messages(
        &self,
        session_id: &str,
        max_messages: usize,
        max_chars: usize,
    ) -> Option<String> {
        let file_path = {
            let sessions = self.sessions.read().unwrap();
            sessions.get(session_id)?.file_path.clone()
        };

        // Read first user messages from the JSONL file
        let file = std::fs::File::open(&file_path).ok()?;
        let reader = std::io::BufReader::new(file);

        use std::io::BufRead;
        let mut user_messages = Vec::new();
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                if val.get("type").and_then(|t| t.as_str()) == Some("user") {
                    if let Some(message) = val.get("message") {
                        if let Some(content) = message.get("content") {
                            let text = if let Some(s) = content.as_str() {
                                s.to_string()
                            } else if let Some(arr) = content.as_array() {
                                // Content can be array of blocks
                                arr.iter()
                                    .filter_map(|b| {
                                        if b.get("type")?.as_str()? == "text" {
                                            b.get("text")?.as_str().map(|s| s.to_string())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            } else {
                                continue;
                            };
                            if !text.is_empty() {
                                user_messages
                                    .push(format!("user: {}", &text[..text.len().min(500)]));
                                if user_messages.len() >= max_messages {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        if user_messages.is_empty() {
            return None;
        }

        let combined = user_messages.join("\n\n");
        Some(if combined.len() > max_chars {
            combined[..max_chars].to_string()
        } else {
            combined
        })
    }

    /// Evict the oldest session if we've reached the limit
    fn evict_if_needed(&self, incoming_session_id: &str) {
        let sessions = self.sessions.read().unwrap();
        if sessions.len() < self.config.max_sessions {
            return;
        }
        // Don't evict if the incoming session already exists
        if sessions.contains_key(incoming_session_id) {
            return;
        }

        // Find the session with the oldest last_accessed
        let oldest = sessions
            .iter()
            .min_by_key(|(_, s)| s.last_accessed)
            .map(|(id, _)| id.clone());
        drop(sessions);

        if let Some(oldest_id) = oldest {
            tracing::debug!("Ephemeral: evicting session {} (LRU)", &oldest_id[..8]);
            self.sessions.write().unwrap().remove(&oldest_id);
            self.messages.write().unwrap().remove(&oldest_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EphemeralConfig;

    fn test_config() -> EphemeralConfig {
        EphemeralConfig {
            max_sessions: 3,
            max_messages_per_session: 100,
        }
    }

    #[test]
    fn test_get_or_create_project() {
        let index = EphemeralIndex::new(test_config());
        let id1 = index.get_or_create_project("/path/to/project", "my-project");
        let id2 = index.get_or_create_project("/path/to/project", "my-project");
        assert_eq!(id1, id2);

        let id3 = index.get_or_create_project("/path/to/other", "other");
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_session_state_empty() {
        let index = EphemeralIndex::new(test_config());
        let (fs, mc, ms) = index.get_session_state("nonexistent");
        assert_eq!(fs, 0);
        assert_eq!(mc, 0);
        assert_eq!(ms, -1);
    }

    #[test]
    fn test_store_and_get_session() {
        let index = EphemeralIndex::new(test_config());

        let msg = MessageMeta {
            sequence_num: 0,
            role: "user".to_string(),
            content_preview: Some("hello".to_string()),
            has_code: false,
            has_error: false,
            has_file_changes: false,
            tool_name: None,
            tool_type: None,
            tool_summary: None,
            byte_offset: 0,
            byte_length: 100,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            model: None,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };

        index.store_session(
            "sess1",
            "/f.jsonl",
            "proj1",
            None,
            "Claude Code",
            vec![msg],
            1000,
            false,
            false,
        );

        let (fs, mc, ms) = index.get_session_state("sess1");
        assert_eq!(fs, 1000);
        assert_eq!(mc, 1);
        assert_eq!(ms, 0);
    }

    #[test]
    fn test_lru_eviction() {
        let index = EphemeralIndex::new(test_config()); // max_sessions = 3

        for i in 0..3 {
            index.store_session(
                &format!("sess{}", i),
                &format!("/f{}.jsonl", i),
                "proj1",
                None,
                "Claude Code",
                vec![],
                100,
                false,
                false,
            );
            // Small sleep to ensure different Instant values
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        // All 3 exist
        assert_eq!(index.sessions.read().unwrap().len(), 3);

        // Add a 4th — should evict sess0 (oldest)
        index.store_session(
            "sess3",
            "/f3.jsonl",
            "proj1",
            None,
            "Claude Code",
            vec![],
            100,
            false,
            false,
        );

        let sessions = index.sessions.read().unwrap();
        assert_eq!(sessions.len(), 3);
        assert!(!sessions.contains_key("sess0"));
        assert!(sessions.contains_key("sess3"));
    }
}
