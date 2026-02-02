//! HTTP route handlers for the API

use super::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Health Check
// ============================================================================

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// ============================================================================
// Projects
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ListProjectsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_projects(
    State(state): State<AppState>,
    Query(query): Query<ListProjectsQuery>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    let mut stmt = match conn.prepare(
        "SELECT id, name, folder_path, description, repo_url, language, framework,
                auto_sync, longest_streak, created_at, updated_at
         FROM projects
         ORDER BY updated_at DESC
         LIMIT ? OFFSET ?"
    ) {
        Ok(stmt) => stmt,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    };

    let projects: Vec<serde_json::Value> = stmt
        .query_map([limit, offset], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "folder_path": row.get::<_, String>(2)?,
                "description": row.get::<_, Option<String>>(3)?,
                "repo_url": row.get::<_, Option<String>>(4)?,
                "language": row.get::<_, Option<String>>(5)?,
                "framework": row.get::<_, Option<String>>(6)?,
                "auto_sync": row.get::<_, bool>(7)?,
                "longest_streak": row.get::<_, i64>(8)?,
                "created_at": row.get::<_, String>(9)?,
                "updated_at": row.get::<_, String>(10)?,
            }))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .unwrap_or(0);

    Json(serde_json::json!({
        "projects": projects,
        "total": total
    })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub folder_path: String,
    pub description: Option<String>,
    pub repo_url: Option<String>,
    pub language: Option<String>,
    pub framework: Option<String>,
}

pub async fn create_project(
    State(state): State<AppState>,
    Json(req): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let conn = state.db.conn();
    match conn.execute(
        "INSERT INTO projects (id, name, folder_path, description, repo_url, language, framework, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![
            id,
            req.name,
            req.folder_path,
            req.description,
            req.repo_url,
            req.language,
            req.framework,
            now,
            now
        ],
    ) {
        Ok(_) => Json(serde_json::json!({
            "id": id,
            "name": req.name,
            "folder_path": req.folder_path,
            "created_at": now
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

pub async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    match conn.query_row(
        "SELECT id, name, folder_path, description, repo_url, language, framework,
                auto_sync, longest_streak, created_at, updated_at
         FROM projects WHERE id = ?",
        [&id],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "folder_path": row.get::<_, String>(2)?,
                "description": row.get::<_, Option<String>>(3)?,
                "repo_url": row.get::<_, Option<String>>(4)?,
                "language": row.get::<_, Option<String>>(5)?,
                "framework": row.get::<_, Option<String>>(6)?,
                "auto_sync": row.get::<_, bool>(7)?,
                "longest_streak": row.get::<_, i64>(8)?,
                "created_at": row.get::<_, String>(9)?,
                "updated_at": row.get::<_, String>(10)?,
            }))
        },
    ) {
        Ok(project) => Json(project).into_response(),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "Project not found"
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub repo_url: Option<String>,
    pub language: Option<String>,
    pub framework: Option<String>,
    pub auto_sync: Option<bool>,
}

pub async fn update_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProjectRequest>,
) -> impl IntoResponse {
    let now = chrono::Utc::now().to_rfc3339();
    let conn = state.db.conn();

    // Build dynamic update query
    let mut updates = vec!["updated_at = ?"];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now.clone())];

    if let Some(name) = &req.name {
        updates.push("name = ?");
        params.push(Box::new(name.clone()));
    }
    if let Some(desc) = &req.description {
        updates.push("description = ?");
        params.push(Box::new(desc.clone()));
    }
    if let Some(repo) = &req.repo_url {
        updates.push("repo_url = ?");
        params.push(Box::new(repo.clone()));
    }
    if let Some(lang) = &req.language {
        updates.push("language = ?");
        params.push(Box::new(lang.clone()));
    }
    if let Some(fw) = &req.framework {
        updates.push("framework = ?");
        params.push(Box::new(fw.clone()));
    }
    if let Some(sync) = req.auto_sync {
        updates.push("auto_sync = ?");
        params.push(Box::new(sync));
    }

    params.push(Box::new(id.clone()));

    let query = format!(
        "UPDATE projects SET {} WHERE id = ?",
        updates.join(", ")
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    match conn.execute(&query, params_refs.as_slice()) {
        Ok(0) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Project not found"
        }))).into_response(),
        Ok(_) => Json(serde_json::json!({
            "id": id,
            "updated_at": now
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

pub async fn delete_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    match conn.execute("DELETE FROM projects WHERE id = ?", [&id]) {
        Ok(0) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Project not found"
        }))).into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

// ============================================================================
// Sessions
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    pub project_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub include_hidden: Option<bool>,
}

pub async fn list_sessions(
    State(state): State<AppState>,
    Query(query): Query<ListSessionsQuery>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    let include_hidden = query.include_hidden.unwrap_or(false);

    let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(project_id) = &query.project_id {
        let hidden_filter = if include_hidden { "" } else { " AND is_hidden = 0" };
        (
            format!(
                "SELECT id, project_id, file_path, title, ai_tool, message_count,
                        duration_ms, has_code, has_errors, is_hidden, created_at, indexed_at
                 FROM sessions
                 WHERE project_id = ?{hidden_filter}
                 ORDER BY created_at DESC
                 LIMIT ? OFFSET ?"
            ),
            vec![Box::new(project_id.clone()), Box::new(limit), Box::new(offset)]
        )
    } else {
        let hidden_filter = if include_hidden { "" } else { " WHERE is_hidden = 0" };
        (
            format!(
                "SELECT id, project_id, file_path, title, ai_tool, message_count,
                        duration_ms, has_code, has_errors, is_hidden, created_at, indexed_at
                 FROM sessions{hidden_filter}
                 ORDER BY created_at DESC
                 LIMIT ? OFFSET ?"
            ),
            vec![Box::new(limit), Box::new(offset)]
        )
    };

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = match conn.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    };

    let sessions: Vec<serde_json::Value> = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "project_id": row.get::<_, String>(1)?,
                "file_path": row.get::<_, String>(2)?,
                "title": row.get::<_, Option<String>>(3)?,
                "ai_tool": row.get::<_, String>(4)?,
                "message_count": row.get::<_, i64>(5)?,
                "duration_ms": row.get::<_, Option<i64>>(6)?,
                "has_code": row.get::<_, bool>(7)?,
                "has_errors": row.get::<_, bool>(8)?,
                "is_hidden": row.get::<_, bool>(9)?,
                "created_at": row.get::<_, String>(10)?,
                "indexed_at": row.get::<_, String>(11)?,
            }))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    // Get total count
    let count_sql = if let Some(project_id) = &query.project_id {
        let hidden_filter = if include_hidden { "" } else { " AND is_hidden = 0" };
        format!("SELECT COUNT(*) FROM sessions WHERE project_id = ?{hidden_filter}")
    } else {
        let hidden_filter = if include_hidden { "" } else { " WHERE is_hidden = 0" };
        format!("SELECT COUNT(*) FROM sessions{hidden_filter}")
    };

    let total: i64 = if let Some(project_id) = &query.project_id {
        conn.query_row(&count_sql, [project_id], |row| row.get(0)).unwrap_or(0)
    } else {
        conn.query_row(&count_sql, [], |row| row.get(0)).unwrap_or(0)
    };

    Json(serde_json::json!({
        "sessions": sessions,
        "total": total
    })).into_response()
}

pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    match conn.query_row(
        "SELECT id, project_id, file_path, title, ai_tool, message_count,
                duration_ms, has_code, has_errors, is_hidden, created_at, indexed_at
         FROM sessions WHERE id = ?",
        [&id],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "project_id": row.get::<_, String>(1)?,
                "file_path": row.get::<_, String>(2)?,
                "title": row.get::<_, Option<String>>(3)?,
                "ai_tool": row.get::<_, String>(4)?,
                "message_count": row.get::<_, i64>(5)?,
                "duration_ms": row.get::<_, Option<i64>>(6)?,
                "has_code": row.get::<_, bool>(7)?,
                "has_errors": row.get::<_, bool>(8)?,
                "is_hidden": row.get::<_, bool>(9)?,
                "created_at": row.get::<_, String>(10)?,
                "indexed_at": row.get::<_, String>(11)?,
            }))
        },
    ) {
        Ok(session) => Json(session).into_response(),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "Session not found"
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
    pub is_hidden: Option<bool>,
}

pub async fn update_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> impl IntoResponse {
    let now = chrono::Utc::now().to_rfc3339();
    let conn = state.db.conn();

    let mut updates = vec!["indexed_at = ?"];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now.clone())];

    if let Some(title) = &req.title {
        updates.push("title = ?");
        updates.push("title_edited = 1");
        params.push(Box::new(title.clone()));
    }
    if let Some(hidden) = req.is_hidden {
        updates.push("is_hidden = ?");
        params.push(Box::new(hidden));
    }

    params.push(Box::new(id.clone()));

    let query = format!(
        "UPDATE sessions SET {} WHERE id = ?",
        updates.join(", ")
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    match conn.execute(&query, params_refs.as_slice()) {
        Ok(0) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Session not found"
        }))).into_response(),
        Ok(_) => Json(serde_json::json!({
            "id": id,
            "updated_at": now
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

pub async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    match conn.execute("DELETE FROM sessions WHERE id = ?", [&id]) {
        Ok(0) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Session not found"
        }))).into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct GetMessagesQuery {
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

pub async fn get_session_messages(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<GetMessagesQuery>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    let mut stmt = match conn.prepare(
        "SELECT id, sequence_num, role, content_preview, has_code, has_error,
                has_file_changes, tool_name, tool_type, tool_summary,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                model, timestamp
         FROM session_messages
         WHERE session_id = ?
         ORDER BY sequence_num
         LIMIT ? OFFSET ?"
    ) {
        Ok(stmt) => stmt,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    };

    let messages: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params![session_id, limit, offset], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "sequence_num": row.get::<_, i64>(1)?,
                "role": row.get::<_, String>(2)?,
                "content_preview": row.get::<_, Option<String>>(3)?,
                "has_code": row.get::<_, bool>(4)?,
                "has_error": row.get::<_, bool>(5)?,
                "has_file_changes": row.get::<_, bool>(6)?,
                "tool_name": row.get::<_, Option<String>>(7)?,
                "tool_type": row.get::<_, Option<String>>(8)?,
                "tool_summary": row.get::<_, Option<String>>(9)?,
                "input_tokens": row.get::<_, Option<i64>>(10)?,
                "output_tokens": row.get::<_, Option<i64>>(11)?,
                "cache_read_tokens": row.get::<_, Option<i64>>(12)?,
                "cache_creation_tokens": row.get::<_, Option<i64>>(13)?,
                "model": row.get::<_, Option<String>>(14)?,
                "timestamp": row.get::<_, String>(15)?,
            }))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM session_messages WHERE session_id = ?",
            [&session_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Json(serde_json::json!({
        "messages": messages,
        "total": total
    })).into_response()
}

pub async fn get_message_content(
    State(state): State<AppState>,
    Path((session_id, seq)): Path<(String, i64)>,
) -> impl IntoResponse {
    let conn = state.db.conn();

    // Get the file path and byte offset for this message
    let result: Result<(String, i64, i64), _> = conn.query_row(
        "SELECT s.file_path, m.byte_offset, m.byte_length
         FROM session_messages m
         JOIN sessions s ON s.id = m.session_id
         WHERE m.session_id = ? AND m.sequence_num = ?",
        rusqlite::params![session_id, seq],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    );

    match result {
        Ok((file_path, byte_offset, byte_length)) => {
            // Read the raw JSONL content from file
            match std::fs::File::open(&file_path) {
                Ok(mut file) => {
                    use std::io::{Read, Seek, SeekFrom};
                    if file.seek(SeekFrom::Start(byte_offset as u64)).is_err() {
                        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                            "error": "Failed to seek to message offset"
                        }))).into_response();
                    }

                    let mut buffer = vec![0u8; byte_length as usize];
                    if file.read_exact(&mut buffer).is_err() {
                        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                            "error": "Failed to read message content"
                        }))).into_response();
                    }

                    match String::from_utf8(buffer) {
                        Ok(content) => {
                            // Parse as JSON and return
                            match serde_json::from_str::<serde_json::Value>(&content) {
                                Ok(json) => Json(json).into_response(),
                                Err(_) => {
                                    // Return as raw string if not valid JSON
                                    Json(serde_json::json!({ "raw": content })).into_response()
                                }
                            }
                        }
                        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                            "error": "Invalid UTF-8 in message content"
                        }))).into_response(),
                    }
                }
                Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
                    "error": format!("Session file not found: {}", e)
                }))).into_response(),
            }
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "Message not found"
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

// ============================================================================
// Search
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub project_id: Option<String>,
    #[serde(rename = "type", default = "default_search_type")]
    pub search_type: String,
    pub limit: Option<i64>,
}

fn default_search_type() -> String {
    "fulltext".to_string()
}

pub async fn search(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    let limit = req.limit.unwrap_or(50);

    // Full-text search using FTS5
    let sql = if let Some(project_id) = &req.project_id {
        format!(
            "SELECT m.session_id, m.sequence_num, m.content_preview, m.timestamp,
                    bm25(session_messages_fts) as score
             FROM session_messages_fts fts
             JOIN session_messages m ON m.id = fts.rowid
             JOIN sessions s ON s.id = m.session_id
             WHERE session_messages_fts MATCH ? AND s.project_id = ?
             ORDER BY score
             LIMIT {limit}"
        )
    } else {
        format!(
            "SELECT m.session_id, m.sequence_num, m.content_preview, m.timestamp,
                    bm25(session_messages_fts) as score
             FROM session_messages_fts fts
             JOIN session_messages m ON m.id = fts.rowid
             WHERE session_messages_fts MATCH ?
             ORDER BY score
             LIMIT {limit}"
        )
    };

    let mut stmt = match conn.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    };

    let results: Vec<serde_json::Value> = if let Some(project_id) = &req.project_id {
        stmt.query_map([&req.query, project_id], |row| {
            Ok(serde_json::json!({
                "session_id": row.get::<_, String>(0)?,
                "message_seq": row.get::<_, i64>(1)?,
                "snippet": row.get::<_, Option<String>>(2)?,
                "timestamp": row.get::<_, String>(3)?,
                "score": row.get::<_, f64>(4)?,
            }))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    } else {
        stmt.query_map([&req.query], |row| {
            Ok(serde_json::json!({
                "session_id": row.get::<_, String>(0)?,
                "message_seq": row.get::<_, i64>(1)?,
                "snippet": row.get::<_, Option<String>>(2)?,
                "timestamp": row.get::<_, String>(3)?,
                "score": row.get::<_, f64>(4)?,
            }))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    };

    Json(serde_json::json!({
        "results": results
    })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct SearchSessionQuery {
    pub q: String,
    pub limit: Option<i64>,
}

pub async fn search_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<SearchSessionQuery>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    let limit = query.limit.unwrap_or(50);

    let mut stmt = match conn.prepare(&format!(
        "SELECT m.sequence_num, m.content_preview, m.timestamp,
                bm25(session_messages_fts) as score
         FROM session_messages_fts fts
         JOIN session_messages m ON m.id = fts.rowid
         WHERE session_messages_fts MATCH ? AND m.session_id = ?
         ORDER BY score
         LIMIT {limit}"
    )) {
        Ok(stmt) => stmt,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    };

    let results: Vec<serde_json::Value> = stmt
        .query_map([&query.q, &session_id], |row| {
            Ok(serde_json::json!({
                "message_seq": row.get::<_, i64>(0)?,
                "snippet": row.get::<_, Option<String>>(1)?,
                "timestamp": row.get::<_, String>(2)?,
                "score": row.get::<_, f64>(3)?,
            }))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    Json(serde_json::json!({
        "results": results
    })).into_response()
}

// ============================================================================
// Memories
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ListMemoriesQuery {
    pub project_id: Option<String>,
    pub memory_type: Option<String>,
    pub state: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_memories(
    State(state): State<AppState>,
    Query(query): Query<ListMemoriesQuery>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    let mut conditions = vec!["1=1"];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(project_id) = &query.project_id {
        conditions.push("project_id = ?");
        params.push(Box::new(project_id.clone()));
    }
    if let Some(memory_type) = &query.memory_type {
        conditions.push("memory_type = ?");
        params.push(Box::new(memory_type.clone()));
    }
    if let Some(memory_state) = &query.state {
        conditions.push("state = ?");
        params.push(Box::new(memory_state.clone()));
    }

    params.push(Box::new(limit));
    params.push(Box::new(offset));

    let sql = format!(
        "SELECT id, project_id, session_id, memory_type, title, content,
                context, tags, confidence, is_validated, state, extracted_at
         FROM memories
         WHERE {} AND state != 'removed'
         ORDER BY confidence DESC
         LIMIT ? OFFSET ?",
        conditions.join(" AND ")
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = match conn.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    };

    let memories: Vec<serde_json::Value> = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "project_id": row.get::<_, String>(1)?,
                "session_id": row.get::<_, String>(2)?,
                "memory_type": row.get::<_, String>(3)?,
                "title": row.get::<_, String>(4)?,
                "content": row.get::<_, String>(5)?,
                "context": row.get::<_, Option<String>>(6)?,
                "tags": row.get::<_, String>(7)?,
                "confidence": row.get::<_, f64>(8)?,
                "is_validated": row.get::<_, bool>(9)?,
                "state": row.get::<_, String>(10)?,
                "extracted_at": row.get::<_, String>(11)?,
            }))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    Json(serde_json::json!({
        "memories": memories
    })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct SearchMemoriesRequest {
    pub query: String,
    pub project_id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub limit: Option<i64>,
}

pub async fn search_memories(
    State(state): State<AppState>,
    Json(req): Json<SearchMemoriesRequest>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    let limit = req.limit.unwrap_or(20);

    // FTS5 search on memories
    let sql = if let Some(project_id) = &req.project_id {
        format!(
            "SELECT m.id, m.project_id, m.session_id, m.memory_type, m.title,
                    m.content, m.context, m.tags, m.confidence, m.state,
                    bm25(memories_fts) as score
             FROM memories_fts fts
             JOIN memories m ON m.id = fts.rowid
             WHERE memories_fts MATCH ? AND m.project_id = ? AND m.state != 'removed'
             ORDER BY score
             LIMIT {limit}"
        )
    } else {
        format!(
            "SELECT m.id, m.project_id, m.session_id, m.memory_type, m.title,
                    m.content, m.context, m.tags, m.confidence, m.state,
                    bm25(memories_fts) as score
             FROM memories_fts fts
             JOIN memories m ON m.id = fts.rowid
             WHERE memories_fts MATCH ? AND m.state != 'removed'
             ORDER BY score
             LIMIT {limit}"
        )
    };

    let mut stmt = match conn.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    };

    let memories: Vec<serde_json::Value> = if let Some(project_id) = &req.project_id {
        stmt.query_map([&req.query, project_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "project_id": row.get::<_, String>(1)?,
                "session_id": row.get::<_, String>(2)?,
                "memory_type": row.get::<_, String>(3)?,
                "title": row.get::<_, String>(4)?,
                "content": row.get::<_, String>(5)?,
                "context": row.get::<_, Option<String>>(6)?,
                "tags": row.get::<_, String>(7)?,
                "confidence": row.get::<_, f64>(8)?,
                "state": row.get::<_, String>(9)?,
                "score": row.get::<_, f64>(10)?,
            }))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    } else {
        stmt.query_map([&req.query], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "project_id": row.get::<_, String>(1)?,
                "session_id": row.get::<_, String>(2)?,
                "memory_type": row.get::<_, String>(3)?,
                "title": row.get::<_, String>(4)?,
                "content": row.get::<_, String>(5)?,
                "context": row.get::<_, Option<String>>(6)?,
                "tags": row.get::<_, String>(7)?,
                "confidence": row.get::<_, f64>(8)?,
                "state": row.get::<_, String>(9)?,
                "score": row.get::<_, f64>(10)?,
            }))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    };

    Json(serde_json::json!({
        "memories": memories
    })).into_response()
}

pub async fn get_memory(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    match conn.query_row(
        "SELECT id, project_id, session_id, memory_type, title, content,
                context, tags, confidence, is_validated, state, extracted_at
         FROM memories WHERE id = ?",
        [id],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "project_id": row.get::<_, String>(1)?,
                "session_id": row.get::<_, String>(2)?,
                "memory_type": row.get::<_, String>(3)?,
                "title": row.get::<_, String>(4)?,
                "content": row.get::<_, String>(5)?,
                "context": row.get::<_, Option<String>>(6)?,
                "tags": row.get::<_, String>(7)?,
                "confidence": row.get::<_, f64>(8)?,
                "is_validated": row.get::<_, bool>(9)?,
                "state": row.get::<_, String>(10)?,
                "extracted_at": row.get::<_, String>(11)?,
            }))
        },
    ) {
        Ok(memory) => Json(memory).into_response(),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "Memory not found"
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateMemoryRequest {
    pub state: Option<String>,
    pub confidence: Option<f64>,
    pub is_validated: Option<bool>,
}

pub async fn update_memory(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateMemoryRequest>,
) -> impl IntoResponse {
    let conn = state.db.conn();

    let mut updates = vec![];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(memory_state) = &req.state {
        updates.push("state = ?");
        params.push(Box::new(memory_state.clone()));
    }
    if let Some(confidence) = req.confidence {
        updates.push("confidence = ?");
        params.push(Box::new(confidence));
    }
    if let Some(validated) = req.is_validated {
        updates.push("is_validated = ?");
        params.push(Box::new(validated));
    }

    if updates.is_empty() {
        return Json(serde_json::json!({ "id": id })).into_response();
    }

    params.push(Box::new(id));

    let query = format!(
        "UPDATE memories SET {} WHERE id = ?",
        updates.join(", ")
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    match conn.execute(&query, params_refs.as_slice()) {
        Ok(0) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Memory not found"
        }))).into_response(),
        Ok(_) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

pub async fn delete_memory(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.conn();
    // Soft delete by setting state to 'removed'
    match conn.execute("UPDATE memories SET state = 'removed' WHERE id = ?", [id]) {
        Ok(0) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Memory not found"
        }))).into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}
