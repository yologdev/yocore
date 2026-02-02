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
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    let result = state
        .db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, folder_path, description, repo_url, language, framework,
                        auto_sync, longest_streak, created_at, updated_at
                 FROM projects
                 ORDER BY updated_at DESC
                 LIMIT ? OFFSET ?",
            )?;

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
                })?
                .filter_map(|r| r.ok())
                .collect();

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
                .unwrap_or(0);

            Ok::<_, rusqlite::Error>((projects, total))
        })
        .await;

    match result {
        Ok((projects, total)) => Json(serde_json::json!({
            "projects": projects,
            "total": total
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
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

    let id_clone = id.clone();
    let now_clone = now.clone();
    let name = req.name.clone();
    let folder_path = req.folder_path.clone();

    let result = state
        .db
        .with_conn(move |conn| {
            conn.execute(
                "INSERT INTO projects (id, name, folder_path, description, repo_url, language, framework, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    id_clone,
                    req.name,
                    req.folder_path,
                    req.description,
                    req.repo_url,
                    req.language,
                    req.framework,
                    now_clone,
                    now_clone
                ],
            )
        })
        .await;

    match result {
        Ok(_) => Json(serde_json::json!({
            "id": id,
            "name": name,
            "folder_path": folder_path,
            "created_at": now
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| {
            conn.query_row(
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
            )
        })
        .await;

    match result {
        Ok(project) => Json(project).into_response(),
        Err(rusqlite::Error::QueryReturnedNoRows) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Project not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
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
    let id_clone = id.clone();
    let now_clone = now.clone();

    let result = state
        .db
        .with_conn(move |conn| {
            // Build dynamic update query
            let mut updates = vec!["updated_at = ?"];
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now_clone)];

            if let Some(name) = req.name {
                updates.push("name = ?");
                params.push(Box::new(name));
            }
            if let Some(desc) = req.description {
                updates.push("description = ?");
                params.push(Box::new(desc));
            }
            if let Some(repo) = req.repo_url {
                updates.push("repo_url = ?");
                params.push(Box::new(repo));
            }
            if let Some(lang) = req.language {
                updates.push("language = ?");
                params.push(Box::new(lang));
            }
            if let Some(fw) = req.framework {
                updates.push("framework = ?");
                params.push(Box::new(fw));
            }
            if let Some(sync) = req.auto_sync {
                updates.push("auto_sync = ?");
                params.push(Box::new(sync));
            }

            params.push(Box::new(id_clone));

            let query = format!("UPDATE projects SET {} WHERE id = ?", updates.join(", "));
            let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

            conn.execute(&query, params_refs.as_slice())
        })
        .await;

    match result {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Project not found" })),
        )
            .into_response(),
        Ok(_) => Json(serde_json::json!({
            "id": id,
            "updated_at": now
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn delete_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| conn.execute("DELETE FROM projects WHERE id = ?", [&id]))
        .await;

    match result {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Project not found" })),
        )
            .into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
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
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    let include_hidden = query.include_hidden.unwrap_or(false);
    let project_id = query.project_id.clone();

    let result = state
        .db
        .with_conn(move |conn| {
            let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) =
                if let Some(ref pid) = project_id {
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
                        vec![Box::new(pid.clone()), Box::new(limit), Box::new(offset)],
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
                        vec![Box::new(limit), Box::new(offset)],
                    )
                };

            let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
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
                })?
                .filter_map(|r| r.ok())
                .collect();

            // Get total count
            let count_sql = if let Some(ref pid) = project_id {
                let hidden_filter = if include_hidden { "" } else { " AND is_hidden = 0" };
                format!("SELECT COUNT(*) FROM sessions WHERE project_id = ?{hidden_filter}")
            } else {
                let hidden_filter = if include_hidden { "" } else { " WHERE is_hidden = 0" };
                format!("SELECT COUNT(*) FROM sessions{hidden_filter}")
            };

            let total: i64 = if let Some(ref pid) = project_id {
                conn.query_row(&count_sql, [pid], |row| row.get(0)).unwrap_or(0)
            } else {
                conn.query_row(&count_sql, [], |row| row.get(0)).unwrap_or(0)
            };

            Ok::<_, rusqlite::Error>((sessions, total))
        })
        .await;

    match result {
        Ok((sessions, total)) => Json(serde_json::json!({
            "sessions": sessions,
            "total": total
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| {
            conn.query_row(
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
            )
        })
        .await;

    match result {
        Ok(session) => Json(session).into_response(),
        Err(rusqlite::Error::QueryReturnedNoRows) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Session not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
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
    let id_clone = id.clone();
    let now_clone = now.clone();

    let result = state
        .db
        .with_conn(move |conn| {
            let mut updates = vec!["indexed_at = ?"];
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now_clone)];

            if let Some(title) = req.title {
                updates.push("title = ?");
                updates.push("title_edited = 1");
                params.push(Box::new(title));
            }
            if let Some(hidden) = req.is_hidden {
                updates.push("is_hidden = ?");
                params.push(Box::new(hidden));
            }

            params.push(Box::new(id_clone));
            let query = format!("UPDATE sessions SET {} WHERE id = ?", updates.join(", "));
            let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

            conn.execute(&query, params_refs.as_slice())
        })
        .await;

    match result {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Session not found" })),
        )
            .into_response(),
        Ok(_) => Json(serde_json::json!({ "id": id, "updated_at": now })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| conn.execute("DELETE FROM sessions WHERE id = ?", [&id]))
        .await;

    match result {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Session not found" })),
        )
            .into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
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
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    let result = state
        .db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, sequence_num, role, content_preview, has_code, has_error,
                        has_file_changes, tool_name, tool_type, tool_summary,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        model, timestamp
                 FROM session_messages
                 WHERE session_id = ?
                 ORDER BY sequence_num
                 LIMIT ? OFFSET ?",
            )?;

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
                })?
                .filter_map(|r| r.ok())
                .collect();

            let total: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM session_messages WHERE session_id = ?",
                    [&session_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            Ok::<_, rusqlite::Error>((messages, total))
        })
        .await;

    match result {
        Ok((messages, total)) => Json(serde_json::json!({
            "messages": messages,
            "total": total
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn get_message_content(
    State(state): State<AppState>,
    Path((session_id, seq)): Path<(String, i64)>,
) -> impl IntoResponse {
    // Get the file path and byte offset from database
    let db_result = state
        .db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT s.file_path, m.byte_offset, m.byte_length
                 FROM session_messages m
                 JOIN sessions s ON s.id = m.session_id
                 WHERE m.session_id = ? AND m.sequence_num = ?",
                rusqlite::params![session_id, seq],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
            )
        })
        .await;

    let (file_path, byte_offset, byte_length) = match db_result {
        Ok(result) => result,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Message not found" })),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    // Read file content in spawn_blocking to avoid blocking async runtime
    let file_result = tokio::task::spawn_blocking(move || {
        use std::io::{Read, Seek, SeekFrom};

        let mut file = std::fs::File::open(&file_path)?;
        file.seek(SeekFrom::Start(byte_offset as u64))?;

        let mut buffer = vec![0u8; byte_length as usize];
        file.read_exact(&mut buffer)?;

        String::from_utf8(buffer).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })
    .await;

    match file_result {
        Ok(Ok(content)) => {
            // Parse as JSON and return
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(json) => Json(json).into_response(),
                Err(_) => Json(serde_json::json!({ "raw": content })).into_response(),
            }
        }
        Ok(Err(e)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Failed to read file: {}", e) })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Task panicked" })),
        )
            .into_response(),
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
    /// Filter by role: "all", "user", "assistant", "tool"
    pub role: Option<String>,
    /// Only return messages with code
    pub has_code: Option<bool>,
}

fn default_search_type() -> String {
    "fulltext".to_string()
}

pub async fn search(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    let limit = req.limit.unwrap_or(100);
    let query_str = req.query.clone();
    let project_id = req.project_id.clone();
    let role_filter = req.role.clone();
    let has_code_filter = req.has_code;

    let result = state
        .db
        .with_conn(move |conn| {
            // Build filter clauses
            let mut filter_clauses = String::new();

            // Exclude system messages
            filter_clauses.push_str(" AND m.role != 'system'");

            // Exclude Write/Edit tool_type='use' - redundant with tool_type='result'
            filter_clauses.push_str(
                " AND (m.tool_type IS NULL OR m.tool_type != 'use' OR m.tool_name NOT IN ('Write', 'Edit'))",
            );

            // Apply role filter
            if let Some(ref role) = role_filter {
                match role.as_str() {
                    "all" => {}
                    "tool" => filter_clauses.push_str(" AND m.tool_type IS NOT NULL"),
                    "user" => filter_clauses.push_str(" AND m.role = 'user' AND m.tool_type IS NULL"),
                    "assistant" => {
                        filter_clauses.push_str(" AND m.role = 'assistant' AND m.tool_type IS NULL")
                    }
                    _ => {}
                }
            }

            // Apply has_code filter
            if has_code_filter == Some(true) {
                filter_clauses.push_str(" AND m.has_code = 1");
            }

            // Build SQL with all fields needed by Desktop
            let sql = if project_id.is_some() {
                format!(
                    "SELECT m.session_id, s.title, s.file_path, m.sequence_num, m.content_preview,
                            m.role, m.timestamp, m.tool_name, m.tool_type, m.has_code,
                            m.byte_offset, m.byte_length, bm25(session_messages_fts) as score
                     FROM session_messages_fts fts
                     JOIN session_messages m ON m.id = fts.rowid
                     JOIN sessions s ON s.id = m.session_id
                     WHERE session_messages_fts MATCH ? AND s.project_id = ?{filter_clauses}
                     ORDER BY score
                     LIMIT {limit}"
                )
            } else {
                format!(
                    "SELECT m.session_id, s.title, s.file_path, m.sequence_num, m.content_preview,
                            m.role, m.timestamp, m.tool_name, m.tool_type, m.has_code,
                            m.byte_offset, m.byte_length, bm25(session_messages_fts) as score
                     FROM session_messages_fts fts
                     JOIN session_messages m ON m.id = fts.rowid
                     JOIN sessions s ON s.id = m.session_id
                     WHERE session_messages_fts MATCH ?{filter_clauses}
                     ORDER BY score
                     LIMIT {limit}"
                )
            };

            let mut stmt = conn.prepare(&sql)?;

            let map_row = |row: &rusqlite::Row| -> rusqlite::Result<serde_json::Value> {
                let score: f64 = row.get(12)?;
                // Normalize BM25 score (negative, lower is better) to 0-1 scale
                let normalized_score = 1.0 / (1.0 + (-score).abs());

                Ok(serde_json::json!({
                    "session_id": row.get::<_, String>(0)?,
                    "session_title": row.get::<_, Option<String>>(1)?,
                    "session_file_path": row.get::<_, String>(2)?,
                    "line_number": row.get::<_, i64>(3)?,
                    "preview": row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    "role": row.get::<_, String>(5)?,
                    "timestamp": row.get::<_, String>(6)?,
                    "tool_name": row.get::<_, Option<String>>(7)?,
                    "tool_type": row.get::<_, Option<String>>(8)?,
                    "has_code": row.get::<_, bool>(9)?,
                    "byte_offset": row.get::<_, i64>(10)?,
                    "byte_length": row.get::<_, i64>(11)?,
                    "relevance_score": normalized_score,
                }))
            };

            let results: Vec<serde_json::Value> = if let Some(ref pid) = project_id {
                stmt.query_map([&query_str, pid], map_row)?
                    .filter_map(|r| r.ok())
                    .collect()
            } else {
                stmt.query_map([&query_str], map_row)?
                    .filter_map(|r| r.ok())
                    .collect()
            };

            let total_count = results.len();
            Ok::<_, rusqlite::Error>((results, total_count))
        })
        .await;

    match result {
        Ok((results, total_count)) => Json(serde_json::json!({
            "results": results,
            "total_count": total_count,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
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
    let limit = query.limit.unwrap_or(50);
    let search_query = query.q.clone();

    let result = state
        .db
        .with_conn(move |conn| {
            let sql = format!(
                "SELECT m.sequence_num, m.content_preview, m.timestamp,
                        bm25(session_messages_fts) as score
                 FROM session_messages_fts fts
                 JOIN session_messages m ON m.id = fts.rowid
                 WHERE session_messages_fts MATCH ? AND m.session_id = ?
                 ORDER BY score
                 LIMIT {limit}"
            );

            let mut stmt = conn.prepare(&sql)?;
            let results: Vec<serde_json::Value> = stmt
                .query_map([&search_query, &session_id], |row| {
                    Ok(serde_json::json!({
                        "message_seq": row.get::<_, i64>(0)?,
                        "snippet": row.get::<_, Option<String>>(1)?,
                        "timestamp": row.get::<_, String>(2)?,
                        "score": row.get::<_, f64>(3)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok::<_, rusqlite::Error>(results)
        })
        .await;

    match result {
        Ok(results) => Json(serde_json::json!({ "results": results })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
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
    let result = state
        .db
        .with_conn(move |conn| {
            let limit = query.limit.unwrap_or(100);
            let offset = query.offset.unwrap_or(0);

            let mut conditions = vec!["1=1"];
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

            if let Some(project_id) = query.project_id {
                conditions.push("project_id = ?");
                params.push(Box::new(project_id));
            }
            if let Some(memory_type) = query.memory_type {
                conditions.push("memory_type = ?");
                params.push(Box::new(memory_type));
            }
            if let Some(memory_state) = query.state {
                conditions.push("state = ?");
                params.push(Box::new(memory_state));
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
            let mut stmt = conn.prepare(&sql)?;

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
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok::<_, rusqlite::Error>(memories)
        })
        .await;

    match result {
        Ok(memories) => Json(serde_json::json!({ "memories": memories })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
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
    let limit = req.limit.unwrap_or(20);
    let query_str = req.query.clone();
    let project_id = req.project_id.clone();

    let result = state
        .db
        .with_conn(move |conn| {
            let sql = if project_id.is_some() {
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

            let mut stmt = conn.prepare(&sql)?;

            let memories: Vec<serde_json::Value> = if let Some(ref pid) = project_id {
                stmt.query_map([&query_str, pid], |row| {
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
                })?
                .filter_map(|r| r.ok())
                .collect()
            } else {
                stmt.query_map([&query_str], |row| {
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
                })?
                .filter_map(|r| r.ok())
                .collect()
            };

            Ok::<_, rusqlite::Error>(memories)
        })
        .await;

    match result {
        Ok(memories) => Json(serde_json::json!({ "memories": memories })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn get_memory(State(state): State<AppState>, Path(id): Path<i64>) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| {
            conn.query_row(
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
            )
        })
        .await;

    match result {
        Ok(memory) => Json(memory).into_response(),
        Err(rusqlite::Error::QueryReturnedNoRows) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Memory not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
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
    // Early return if no updates
    if req.state.is_none() && req.confidence.is_none() && req.is_validated.is_none() {
        return Json(serde_json::json!({ "id": id })).into_response();
    }

    let result = state
        .db
        .with_conn(move |conn| {
            let mut updates = vec![];
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

            if let Some(memory_state) = req.state {
                updates.push("state = ?");
                params.push(Box::new(memory_state));
            }
            if let Some(confidence) = req.confidence {
                updates.push("confidence = ?");
                params.push(Box::new(confidence));
            }
            if let Some(validated) = req.is_validated {
                updates.push("is_validated = ?");
                params.push(Box::new(validated));
            }

            params.push(Box::new(id));

            let query = format!("UPDATE memories SET {} WHERE id = ?", updates.join(", "));
            let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

            conn.execute(&query, params_refs.as_slice())
        })
        .await;

    match result {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Memory not found" })),
        )
            .into_response(),
        Ok(_) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn delete_memory(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    // Soft delete by setting state to 'removed'
    let result = state
        .db
        .with_conn(move |conn| {
            conn.execute("UPDATE memories SET state = 'removed' WHERE id = ?", [id])
        })
        .await;

    match result {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Memory not found" })),
        )
            .into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
