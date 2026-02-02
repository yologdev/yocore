//! Database operations for MCP server
//! Wraps yocore's Database with MCP-specific query methods

use crate::db::Database;
use super::types::{Memory, MemoryType, Project, SessionContext};
use std::sync::Arc;

/// MCP database operations
pub struct McpDb {
    db: Arc<Database>,
}

impl McpDb {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Look up a project by path prefix (for nested project directories)
    pub fn get_project_by_path_prefix(&self, folder_path: &str) -> Result<Option<Project>, String> {
        let conn = self.db.conn();

        // Normalize the path
        let normalized_path = folder_path.trim_end_matches('/');

        // First try exact match
        let result = conn.query_row(
            "SELECT id, name, folder_path FROM projects WHERE folder_path = ?",
            [normalized_path],
            |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    folder_path: row.get(2)?,
                })
            },
        );

        match result {
            Ok(project) => return Ok(Some(project)),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(format!("Failed to query project: {}", e)),
        }

        // Try converting filesystem path to Claude Code project path format
        let claude_project_path = convert_to_claude_project_path(normalized_path);
        if let Some(claude_path) = &claude_project_path {
            let result = conn.query_row(
                "SELECT id, name, folder_path FROM projects WHERE folder_path = ?",
                [claude_path.as_str()],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        folder_path: row.get(2)?,
                    })
                },
            );

            match result {
                Ok(project) => return Ok(Some(project)),
                Err(rusqlite::Error::QueryReturnedNoRows) => {}
                Err(e) => return Err(format!("Failed to query project: {}", e)),
            }
        }

        // Then try prefix match
        let result = conn.query_row(
            "SELECT id, name, folder_path FROM projects
             WHERE ? LIKE folder_path || '%'
             ORDER BY LENGTH(folder_path) DESC
             LIMIT 1",
            [normalized_path],
            |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    folder_path: row.get(2)?,
                })
            },
        );

        match result {
            Ok(project) => Ok(Some(project)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to query project by prefix: {}", e)),
        }
    }

    /// Get recent sessions for a project
    pub fn get_recent_sessions(&self, project_id: &str, limit: usize) -> Result<Vec<String>, String> {
        let conn = self.db.conn();

        let mut stmt = conn
            .prepare(
                "SELECT id FROM sessions
                 WHERE project_id = ?
                 ORDER BY created_at DESC
                 LIMIT ?",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let session_ids = stmt
            .query_map([project_id, &limit.to_string()], |row| row.get(0))
            .map_err(|e| format!("Failed to query sessions: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(session_ids)
    }

    /// Get or create session context for a session
    pub fn get_or_create_session_context(
        &self,
        session_id: &str,
        project_id: &str,
        source: &str,
    ) -> Result<SessionContext, String> {
        let conn = self.db.conn();

        // Try to get existing context
        let result = conn.query_row(
            "SELECT session_id, project_id, active_task, recent_decisions, open_questions,
                    resume_context, source, created_at, updated_at
             FROM session_context WHERE session_id = ?",
            [session_id],
            |row| {
                let decisions_json: String = row.get(3)?;
                let questions_json: String = row.get(4)?;
                Ok(SessionContext {
                    session_id: row.get(0)?,
                    project_id: row.get(1)?,
                    active_task: row.get(2)?,
                    recent_decisions: serde_json::from_str(&decisions_json).unwrap_or_default(),
                    open_questions: serde_json::from_str(&questions_json).unwrap_or_default(),
                    resume_context: row.get(5)?,
                    source: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        );

        match result {
            Ok(ctx) => Ok(ctx),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // Create new session context
                let now = chrono::Utc::now().to_rfc3339();
                conn.execute(
                    "INSERT INTO session_context (session_id, project_id, active_task,
                        recent_decisions, open_questions, resume_context, source, created_at, updated_at)
                     VALUES (?, ?, NULL, '[]', '[]', NULL, ?, ?, ?)",
                    [session_id, project_id, source, &now, &now],
                ).map_err(|e| format!("Failed to create session context: {}", e))?;

                Ok(SessionContext {
                    session_id: session_id.to_string(),
                    project_id: project_id.to_string(),
                    active_task: None,
                    recent_decisions: vec![],
                    open_questions: vec![],
                    resume_context: None,
                    source: source.to_string(),
                    created_at: now.clone(),
                    updated_at: now,
                })
            }
            Err(e) => Err(format!("Failed to get session context: {}", e)),
        }
    }

    /// Get session context by session ID
    pub fn get_session_context(&self, session_id: &str) -> Result<Option<SessionContext>, String> {
        let conn = self.db.conn();

        let result = conn.query_row(
            "SELECT session_id, project_id, active_task, recent_decisions, open_questions,
                    resume_context, source, created_at, updated_at
             FROM session_context WHERE session_id = ?",
            [session_id],
            |row| {
                let decisions_json: String = row.get(3)?;
                let questions_json: String = row.get(4)?;
                Ok(SessionContext {
                    session_id: row.get(0)?,
                    project_id: row.get(1)?,
                    active_task: row.get(2)?,
                    recent_decisions: serde_json::from_str(&decisions_json).unwrap_or_default(),
                    open_questions: serde_json::from_str(&questions_json).unwrap_or_default(),
                    resume_context: row.get(5)?,
                    source: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        );

        match result {
            Ok(ctx) => Ok(Some(ctx)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get session context: {}", e)),
        }
    }

    /// Save lifeboat state
    pub fn save_lifeboat(&self, session_id: &str, summary: &str) -> Result<(), String> {
        let conn = self.db.conn();
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE session_context SET resume_context = ?, updated_at = ? WHERE session_id = ?",
            [summary, &now, session_id],
        ).map_err(|e| format!("Failed to save lifeboat: {}", e))?;

        Ok(())
    }

    /// Get recent sessions with context (excluding current)
    pub fn get_recent_sessions_with_context(
        &self,
        project_id: &str,
        exclude_session_id: &str,
        limit: usize,
    ) -> Result<Vec<String>, String> {
        let conn = self.db.conn();

        let mut stmt = conn
            .prepare(
                "SELECT session_id FROM session_context
                 WHERE project_id = ? AND session_id != ?
                 ORDER BY updated_at DESC
                 LIMIT ?",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let session_ids = stmt
            .query_map([project_id, exclude_session_id, &limit.to_string()], |row| row.get(0))
            .map_err(|e| format!("Failed to query sessions: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(session_ids)
    }

    /// Track memory access
    pub fn track_memory_access(&self, memory_ids: &[i64]) -> Result<(), String> {
        if memory_ids.is_empty() {
            return Ok(());
        }

        let conn = self.db.conn();
        let now = chrono::Utc::now().to_rfc3339();

        let placeholders: Vec<&str> = memory_ids.iter().map(|_| "?").collect();
        let sql = format!(
            "UPDATE memories SET access_count = access_count + 1, last_accessed_at = ?
             WHERE id IN ({})",
            placeholders.join(", ")
        );

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        params.push(Box::new(now));
        for id in memory_ids {
            params.push(Box::new(*id));
        }

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        conn.execute(&sql, params_refs.as_slice())
            .map_err(|e| format!("Failed to track memory access: {}", e))?;

        Ok(())
    }

    /// Search memories using FTS5
    pub fn search_memories_fts(
        &self,
        query: &str,
        project_id: &str,
        memory_types: Option<&[MemoryType]>,
        limit: usize,
    ) -> Result<Vec<Memory>, String> {
        let conn = self.db.conn();

        let mut sql = String::from(
            "SELECT m.id, m.project_id, m.session_id, m.memory_type, m.title, m.content,
                    m.context, m.tags, m.confidence, m.is_validated, m.extracted_at, m.file_reference
             FROM memories m
             JOIN memories_fts ON m.id = memories_fts.rowid
             WHERE memories_fts MATCH ? AND m.state != 'removed' AND m.project_id = ?",
        );

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let fts_query = build_fts_query(query);
        params.push(Box::new(fts_query));
        params.push(Box::new(project_id.to_string()));

        if let Some(types) = memory_types {
            if !types.is_empty() {
                let placeholders: Vec<&str> = types.iter().map(|_| "?").collect();
                sql.push_str(&format!(
                    " AND m.memory_type IN ({})",
                    placeholders.join(", ")
                ));
                for t in types {
                    params.push(Box::new(t.to_db_str().to_string()));
                }
            }
        }

        sql.push_str(&format!(" ORDER BY bm25(memories_fts) LIMIT {}", limit));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare search query: {}", e))?;

        let memories = stmt
            .query_map(params_refs.as_slice(), |row| {
                row_to_memory(row)
            })
            .map_err(|e| format!("Failed to execute search: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(memories)
    }

    /// Get memories by type
    pub fn get_memories_by_type(
        &self,
        project_id: &str,
        memory_type: MemoryType,
        limit: usize,
    ) -> Result<Vec<Memory>, String> {
        let conn = self.db.conn();

        let mut stmt = conn
            .prepare(
                "SELECT m.id, m.project_id, m.session_id, m.memory_type, m.title, m.content,
                        m.context, m.tags, m.confidence, m.is_validated, m.extracted_at, m.file_reference
                 FROM memories m
                 WHERE m.project_id = ? AND m.memory_type = ? AND m.state != 'removed'
                 ORDER BY m.confidence DESC, m.extracted_at DESC
                 LIMIT ?",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let memories = stmt
            .query_map([project_id, memory_type.to_db_str(), &limit.to_string()], |row| {
                row_to_memory(row)
            })
            .map_err(|e| format!("Failed to execute query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(memories)
    }

    /// Get memories by tag
    pub fn get_memories_by_tag(
        &self,
        project_id: &str,
        tag: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Memory>, String> {
        let conn = self.db.conn();

        let tag_pattern = format!("%\"{}%", tag);

        let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(q) = query {
            let fts_query = build_fts_query(q);
            (
                "SELECT m.id, m.project_id, m.session_id, m.memory_type, m.title, m.content,
                        m.context, m.tags, m.confidence, m.is_validated, m.extracted_at, m.file_reference
                 FROM memories m
                 JOIN memories_fts ON m.id = memories_fts.rowid
                 WHERE m.project_id = ? AND m.tags LIKE ? AND m.state != 'removed'
                 AND memories_fts MATCH ?
                 ORDER BY bm25(memories_fts)
                 LIMIT ?".to_string(),
                vec![
                    Box::new(project_id.to_string()),
                    Box::new(tag_pattern),
                    Box::new(fts_query),
                    Box::new(limit.to_string()),
                ]
            )
        } else {
            (
                "SELECT m.id, m.project_id, m.session_id, m.memory_type, m.title, m.content,
                        m.context, m.tags, m.confidence, m.is_validated, m.extracted_at, m.file_reference
                 FROM memories m
                 WHERE m.project_id = ? AND m.tags LIKE ? AND m.state != 'removed'
                 ORDER BY m.confidence DESC, m.extracted_at DESC
                 LIMIT ?".to_string(),
                vec![
                    Box::new(project_id.to_string()),
                    Box::new(tag_pattern),
                    Box::new(limit.to_string()),
                ]
            )
        };

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let memories = stmt
            .query_map(params_refs.as_slice(), |row| row_to_memory(row))
            .map_err(|e| format!("Failed to execute query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(memories)
    }

    /// Get memories from specific sessions
    pub fn get_memories_by_sessions(
        &self,
        session_ids: &[String],
        limit: usize,
    ) -> Result<Vec<Memory>, String> {
        if session_ids.is_empty() {
            return Ok(vec![]);
        }

        let conn = self.db.conn();
        let placeholders: Vec<&str> = session_ids.iter().map(|_| "?").collect();

        let sql = format!(
            "SELECT m.id, m.project_id, m.session_id, m.memory_type, m.title, m.content,
                    m.context, m.tags, m.confidence, m.is_validated, m.extracted_at, m.file_reference
             FROM memories m
             WHERE m.session_id IN ({}) AND m.state != 'removed'
             ORDER BY m.extracted_at DESC
             LIMIT ?",
            placeholders.join(", ")
        );

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = session_ids
            .iter()
            .map(|s| Box::new(s.clone()) as Box<dyn rusqlite::ToSql>)
            .collect();
        params.push(Box::new(limit.to_string()));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let memories = stmt
            .query_map(params_refs.as_slice(), |row| row_to_memory(row))
            .map_err(|e| format!("Failed to execute query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(memories)
    }

    /// Get high-state (persistent) memories for a project
    pub fn get_persistent_memories(&self, project_id: &str, limit: usize) -> Result<Vec<Memory>, String> {
        let conn = self.db.conn();

        let mut stmt = conn
            .prepare(
                "SELECT m.id, m.project_id, m.session_id, m.memory_type, m.title, m.content,
                        m.context, m.tags, m.confidence, m.is_validated, m.extracted_at, m.file_reference
                 FROM memories m
                 WHERE m.project_id = ? AND m.state = 'high'
                 ORDER BY m.confidence DESC, m.extracted_at DESC
                 LIMIT ?",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let memories = stmt
            .query_map([project_id, &limit.to_string()], |row| row_to_memory(row))
            .map_err(|e| format!("Failed to execute query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(memories)
    }
}

/// Convert a database row to Memory
fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
    let memory_type_str: String = row.get(3)?;
    let tags_json: String = row.get(7)?;

    Ok(Memory {
        id: row.get(0)?,
        project_id: row.get(1)?,
        session_id: row.get(2)?,
        memory_type: MemoryType::from_str(&memory_type_str).unwrap_or(MemoryType::Context),
        title: row.get(4)?,
        content: row.get(5)?,
        context: row.get(6)?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        confidence: row.get(8)?,
        is_validated: row.get(9)?,
        extracted_at: row.get(10)?,
        file_reference: row.get(11)?,
    })
}

/// Build FTS5 query from user input
fn build_fts_query(query: &str) -> String {
    // Split into words and wrap with wildcards for prefix matching
    query
        .split_whitespace()
        .map(|word| format!("{}*", word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Convert filesystem path to Claude Code project path format
fn convert_to_claude_project_path(path: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    let home_str = home.to_str()?;
    let path_component = path.replace('/', "-");
    Some(format!("{}/.claude/projects/{}", home_str, path_component))
}
