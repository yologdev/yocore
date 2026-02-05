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
// Project Analytics
// ============================================================================

#[derive(Debug, serde::Serialize)]
pub struct ProjectStats {
    pub total_sessions: i64,
    pub total_messages: i64,
    pub total_duration_ms: i64,
    pub messages_with_errors: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cache_read_tokens: i64,
    pub total_cache_creation_tokens: i64,
    pub models_used: std::collections::HashMap<String, i64>,
    pub user_messages: i64,
    pub assistant_messages: i64,
    pub tool_uses: i64,
    pub tool_results: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct SessionVibeData {
    pub session_id: String,
    pub created_at: String,
    pub duration_ms: Option<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub message_count: i64,
    pub user_messages: i64,
    pub assistant_messages: i64,
    pub tool_uses: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct DailyTokens {
    pub date: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct DailyErrors {
    pub date: String,
    pub error_count: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct DailyVibeMetrics {
    pub date: String,
    pub total_messages: i64,
    pub user_messages: i64,
    pub duration_ms: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ProjectAnalyticsBatch {
    pub stats: ProjectStats,
    pub session_metrics: Vec<SessionVibeData>,
    pub active_dates: Vec<String>,
    pub daily_tokens: Vec<DailyTokens>,
    pub daily_errors: Vec<DailyErrors>,
    pub daily_vibe: Vec<DailyVibeMetrics>,
}

/// Get comprehensive project analytics in a single call
pub async fn get_project_analytics(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| {
            // 1. Project Stats
            let total_sessions: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sessions WHERE project_id = ? AND is_hidden = 0",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let total_duration_ms: i64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(duration_ms), 0) FROM sessions WHERE project_id = ? AND is_hidden = 0",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // Message stats from session_messages
            let (total_messages, messages_with_errors, user_messages, assistant_messages, tool_uses, tool_results): (i64, i64, i64, i64, i64, i64) = conn
                .query_row(
                    "SELECT
                        COUNT(*),
                        SUM(CASE WHEN has_error = 1 THEN 1 ELSE 0 END),
                        SUM(CASE WHEN role = 'user' THEN 1 ELSE 0 END),
                        SUM(CASE WHEN role = 'assistant' THEN 1 ELSE 0 END),
                        SUM(CASE WHEN role = 'assistant' AND tool_name IS NOT NULL THEN 1 ELSE 0 END),
                        SUM(CASE WHEN role = 'user' AND tool_name IS NOT NULL THEN 1 ELSE 0 END)
                     FROM session_messages sm
                     JOIN sessions s ON sm.session_id = s.id
                     WHERE s.project_id = ? AND s.is_hidden = 0",
                    [&project_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
                )
                .unwrap_or((0, 0, 0, 0, 0, 0));

            // Token totals
            let (total_input_tokens, total_output_tokens, total_cache_read_tokens, total_cache_creation_tokens): (i64, i64, i64, i64) = conn
                .query_row(
                    "SELECT
                        COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cache_read_tokens), 0),
                        COALESCE(SUM(cache_creation_tokens), 0)
                     FROM session_messages sm
                     JOIN sessions s ON sm.session_id = s.id
                     WHERE s.project_id = ? AND s.is_hidden = 0",
                    [&project_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap_or((0, 0, 0, 0));

            // Models used
            let mut models_used: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
            if let Ok(mut stmt) = conn.prepare(
                "SELECT model, COUNT(*) FROM session_messages sm
                 JOIN sessions s ON sm.session_id = s.id
                 WHERE s.project_id = ? AND s.is_hidden = 0 AND model IS NOT NULL
                 GROUP BY model"
            ) {
                if let Ok(rows) = stmt.query_map([&project_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                }) {
                    for row in rows.flatten() {
                        models_used.insert(row.0, row.1);
                    }
                }
            }

            let stats = ProjectStats {
                total_sessions,
                total_messages,
                total_duration_ms,
                messages_with_errors,
                total_input_tokens,
                total_output_tokens,
                total_cache_read_tokens,
                total_cache_creation_tokens,
                models_used,
                user_messages,
                assistant_messages,
                tool_uses,
                tool_results,
            };

            // 2. Session Metrics
            let mut session_metrics: Vec<SessionVibeData> = Vec::new();
            if let Ok(mut stmt) = conn.prepare(
                "SELECT s.id, s.created_at, s.duration_ms,
                        COALESCE(SUM(sm.input_tokens), 0),
                        COALESCE(SUM(sm.output_tokens), 0),
                        COUNT(sm.id),
                        SUM(CASE WHEN sm.role = 'user' THEN 1 ELSE 0 END),
                        SUM(CASE WHEN sm.role = 'assistant' THEN 1 ELSE 0 END),
                        SUM(CASE WHEN sm.role = 'assistant' AND sm.tool_name IS NOT NULL THEN 1 ELSE 0 END)
                 FROM sessions s
                 LEFT JOIN session_messages sm ON s.id = sm.session_id
                 WHERE s.project_id = ? AND s.is_hidden = 0
                 GROUP BY s.id
                 ORDER BY s.created_at DESC"
            ) {
                if let Ok(rows) = stmt.query_map([&project_id], |row| {
                    Ok(SessionVibeData {
                        session_id: row.get(0)?,
                        created_at: row.get(1)?,
                        duration_ms: row.get(2)?,
                        input_tokens: row.get(3)?,
                        output_tokens: row.get(4)?,
                        message_count: row.get(5)?,
                        user_messages: row.get(6)?,
                        assistant_messages: row.get(7)?,
                        tool_uses: row.get(8)?,
                    })
                }) {
                    session_metrics = rows.filter_map(|r| r.ok()).collect();
                }
            }

            // 3. Active Dates
            let mut active_dates: Vec<String> = Vec::new();
            if let Ok(mut stmt) = conn.prepare(
                "SELECT DISTINCT DATE(created_at) FROM sessions
                 WHERE project_id = ? AND is_hidden = 0
                 ORDER BY DATE(created_at) DESC"
            ) {
                if let Ok(rows) = stmt.query_map([&project_id], |row| row.get::<_, String>(0)) {
                    active_dates = rows.filter_map(|r| r.ok()).collect();
                }
            }

            // 4. Daily Tokens
            let mut daily_tokens: Vec<DailyTokens> = Vec::new();
            if let Ok(mut stmt) = conn.prepare(
                "SELECT DATE(s.created_at) as date,
                        COALESCE(SUM(sm.input_tokens), 0) + COALESCE(SUM(sm.output_tokens), 0),
                        COALESCE(SUM(sm.input_tokens), 0),
                        COALESCE(SUM(sm.output_tokens), 0),
                        COALESCE(SUM(sm.cache_read_tokens), 0),
                        COALESCE(SUM(sm.cache_creation_tokens), 0)
                 FROM sessions s
                 LEFT JOIN session_messages sm ON s.id = sm.session_id
                 WHERE s.project_id = ? AND s.is_hidden = 0
                 GROUP BY DATE(s.created_at)
                 ORDER BY date DESC"
            ) {
                if let Ok(rows) = stmt.query_map([&project_id], |row| {
                    Ok(DailyTokens {
                        date: row.get(0)?,
                        total_tokens: row.get(1)?,
                        input_tokens: row.get(2)?,
                        output_tokens: row.get(3)?,
                        cache_read_tokens: row.get(4)?,
                        cache_creation_tokens: row.get(5)?,
                    })
                }) {
                    daily_tokens = rows.filter_map(|r| r.ok()).collect();
                }
            }

            // 5. Daily Errors
            let mut daily_errors: Vec<DailyErrors> = Vec::new();
            if let Ok(mut stmt) = conn.prepare(
                "SELECT DATE(s.created_at) as date, SUM(CASE WHEN sm.has_error = 1 THEN 1 ELSE 0 END)
                 FROM sessions s
                 LEFT JOIN session_messages sm ON s.id = sm.session_id
                 WHERE s.project_id = ? AND s.is_hidden = 0
                 GROUP BY DATE(s.created_at)
                 HAVING SUM(CASE WHEN sm.has_error = 1 THEN 1 ELSE 0 END) > 0
                 ORDER BY date DESC"
            ) {
                if let Ok(rows) = stmt.query_map([&project_id], |row| {
                    Ok(DailyErrors {
                        date: row.get(0)?,
                        error_count: row.get(1)?,
                    })
                }) {
                    daily_errors = rows.filter_map(|r| r.ok()).collect();
                }
            }

            // 6. Daily Vibe Metrics
            let mut daily_vibe: Vec<DailyVibeMetrics> = Vec::new();
            if let Ok(mut stmt) = conn.prepare(
                "SELECT DATE(s.created_at) as date,
                        COUNT(sm.id),
                        SUM(CASE WHEN sm.role = 'user' THEN 1 ELSE 0 END),
                        COALESCE(SUM(s.duration_ms), 0),
                        COALESCE(SUM(sm.input_tokens), 0),
                        COALESCE(SUM(sm.output_tokens), 0),
                        COALESCE(SUM(sm.cache_read_tokens), 0),
                        COALESCE(SUM(sm.cache_creation_tokens), 0)
                 FROM sessions s
                 LEFT JOIN session_messages sm ON s.id = sm.session_id
                 WHERE s.project_id = ? AND s.is_hidden = 0
                 GROUP BY DATE(s.created_at)
                 ORDER BY date DESC"
            ) {
                if let Ok(rows) = stmt.query_map([&project_id], |row| {
                    Ok(DailyVibeMetrics {
                        date: row.get(0)?,
                        total_messages: row.get(1)?,
                        user_messages: row.get(2)?,
                        duration_ms: row.get(3)?,
                        input_tokens: row.get(4)?,
                        output_tokens: row.get(5)?,
                        cache_read_tokens: row.get(6)?,
                        cache_creation_tokens: row.get(7)?,
                    })
                }) {
                    daily_vibe = rows.filter_map(|r| r.ok()).collect();
                }
            }

            Ok::<_, rusqlite::Error>(ProjectAnalyticsBatch {
                stats,
                session_metrics,
                active_dates,
                daily_tokens,
                daily_errors,
                daily_vibe,
            })
        })
        .await;

    match result {
        Ok(analytics) => Json(analytics).into_response(),
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
    let project_id_input = query.project_id.clone();

    let result = state
        .db
        .with_conn(move |conn| {
            // Resolve folder-path-based ID to actual UUID if provided
            let project_id = project_id_input
                .as_ref()
                .and_then(|pid| resolve_project_id(conn, pid));

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
            let session_id_clone = session_id.clone();
            let mut stmt = conn.prepare(
                "SELECT id, sequence_num, role, content_preview, search_content, has_code, has_error,
                        has_file_changes, tool_name, tool_type, tool_summary,
                        byte_offset, byte_length, input_tokens, output_tokens,
                        cache_read_tokens, cache_creation_tokens, model, timestamp
                 FROM session_messages
                 WHERE session_id = ?
                 ORDER BY sequence_num
                 LIMIT ? OFFSET ?",
            )?;

            let messages: Vec<serde_json::Value> = stmt
                .query_map(rusqlite::params![session_id, limit, offset], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, i64>(0)?,
                        "session_id": session_id_clone,
                        "sequence_num": row.get::<_, i64>(1)?,
                        "role": row.get::<_, String>(2)?,
                        "content_preview": row.get::<_, Option<String>>(3)?,
                        "search_content": row.get::<_, Option<String>>(4)?,
                        "has_code": row.get::<_, bool>(5)?,
                        "has_error": row.get::<_, bool>(6)?,
                        "has_file_changes": row.get::<_, bool>(7)?,
                        "tool_name": row.get::<_, Option<String>>(8)?,
                        "tool_type": row.get::<_, Option<String>>(9)?,
                        "tool_summary": row.get::<_, Option<String>>(10)?,
                        "byte_offset": row.get::<_, i64>(11)?,
                        "byte_length": row.get::<_, i64>(12)?,
                        "input_tokens": row.get::<_, Option<i64>>(13)?,
                        "output_tokens": row.get::<_, Option<i64>>(14)?,
                        "cache_read_tokens": row.get::<_, Option<i64>>(15)?,
                        "cache_creation_tokens": row.get::<_, Option<i64>>(16)?,
                        "model": row.get::<_, Option<String>>(17)?,
                        "timestamp": row.get::<_, String>(18)?,
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
    /// Sort by field: "confidence" (default), "extracted_at"
    pub sort_by: Option<String>,
    /// Sort order: "asc" or "desc" (default)
    pub sort_order: Option<String>,
}

/// Resolve a project identifier to a UUID.
/// If the input looks like a folder-path-based ID (starts with '-' or is not a valid UUID),
/// look it up by folder_path. Otherwise, use it directly as a UUID.
fn resolve_project_id(conn: &rusqlite::Connection, project_id: &str) -> Option<String> {
    // Check if it looks like a UUID (36 chars with hyphens in right places)
    let is_uuid = project_id.len() == 36
        && project_id.chars().nth(8) == Some('-')
        && project_id.chars().nth(13) == Some('-');

    if is_uuid {
        // Verify it exists
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?)",
                [project_id],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if exists {
            return Some(project_id.to_string());
        }
    }

    // Try to find by folder_path (ending with this segment)
    let folder_suffix = if project_id.starts_with('-') {
        format!("/{}", project_id)
    } else {
        format!("/{}", project_id)
    };

    conn.query_row(
        "SELECT id FROM projects WHERE folder_path LIKE ?",
        [format!("%{}", folder_suffix)],
        |row| row.get(0),
    )
    .ok()
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

            if let Some(project_id_input) = query.project_id {
                // Resolve folder-path-based ID to actual UUID
                let resolved_id = resolve_project_id(conn, &project_id_input)
                    .unwrap_or(project_id_input);
                conditions.push("project_id = ?");
                params.push(Box::new(resolved_id));
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

            // Build ORDER BY clause - whitelist allowed columns to prevent SQL injection
            let sort_column = match query.sort_by.as_deref() {
                Some("extracted_at") => "extracted_at",
                Some("confidence") | None => "confidence",
                _ => "confidence", // Default for unknown values
            };
            let sort_direction = match query.sort_order.as_deref() {
                Some("asc") => "ASC",
                Some("desc") | None => "DESC",
                _ => "DESC", // Default for unknown values
            };

            let sql = format!(
                "SELECT id, project_id, session_id, memory_type, title, content,
                        context, tags, confidence, is_validated, state, extracted_at
                 FROM memories
                 WHERE {} AND state != 'removed'
                 ORDER BY {} {}
                 LIMIT ? OFFSET ?",
                conditions.join(" AND "),
                sort_column,
                sort_direction
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
    let project_id_input = req.project_id.clone();

    let result = state
        .db
        .with_conn(move |conn| {
            // Resolve folder-path-based ID to actual UUID if provided
            let project_id = project_id_input
                .as_ref()
                .and_then(|pid| resolve_project_id(conn, pid));

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

/// Memory type count for statistics
#[derive(Debug, serde::Serialize)]
pub struct MemoryTypeCount {
    pub memory_type: String,
    pub count: i64,
}

/// Memory statistics response
#[derive(Debug, serde::Serialize)]
pub struct MemoryStatsResponse {
    pub total_count: i64,
    pub by_type: Vec<MemoryTypeCount>,
    pub validated_count: i64,
    pub avg_confidence: f64,
    pub sessions_with_memories: i64,
}

/// Get memory statistics for a project
pub async fn get_memory_stats(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| {
            // Total count (excluding removed)
            let total_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories m
                     JOIN sessions s ON m.session_id = s.id
                     WHERE s.project_id = ? AND m.state != 'removed'",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // Count by type
            let mut by_type_stmt = conn.prepare(
                "SELECT m.memory_type, COUNT(*) as count FROM memories m
                 JOIN sessions s ON m.session_id = s.id
                 WHERE s.project_id = ? AND m.state != 'removed'
                 GROUP BY m.memory_type
                 ORDER BY count DESC",
            )?;
            let by_type: Vec<MemoryTypeCount> = by_type_stmt
                .query_map([&project_id], |row| {
                    Ok(MemoryTypeCount {
                        memory_type: row.get(0)?,
                        count: row.get(1)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            // Validated count
            let validated_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories m
                     JOIN sessions s ON m.session_id = s.id
                     WHERE s.project_id = ? AND m.state != 'removed' AND m.is_validated = 1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // Average confidence
            let avg_confidence: f64 = conn
                .query_row(
                    "SELECT AVG(m.confidence) FROM memories m
                     JOIN sessions s ON m.session_id = s.id
                     WHERE s.project_id = ? AND m.state != 'removed'",
                    [&project_id],
                    |row| row.get::<_, Option<f64>>(0),
                )
                .unwrap_or(None)
                .unwrap_or(0.0);

            // Sessions with memories
            let sessions_with_memories: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT m.session_id) FROM memories m
                     JOIN sessions s ON m.session_id = s.id
                     WHERE s.project_id = ? AND m.state != 'removed'",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            Ok::<_, rusqlite::Error>(MemoryStatsResponse {
                total_count,
                by_type,
                validated_count,
                avg_confidence,
                sessions_with_memories,
            })
        })
        .await;

    match result {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Get top 25 most-used tags for a project's memories
pub async fn get_memory_tags(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| {
            // Get all tags from memories, split by comma, count occurrences
            let mut stmt = conn.prepare(
                "SELECT m.tags FROM memories m
                 JOIN sessions s ON m.session_id = s.id
                 WHERE s.project_id = ? AND m.state != 'removed' AND m.tags IS NOT NULL AND m.tags != ''",
            )?;

            let tag_strings: Vec<String> = stmt
                .query_map([&project_id], |row| row.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect();

            // Count tag occurrences
            let mut tag_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for tags_str in tag_strings {
                for tag in tags_str.split(',').map(|t| t.trim().to_lowercase()) {
                    if !tag.is_empty() {
                        *tag_counts.entry(tag).or_insert(0) += 1;
                    }
                }
            }

            // Sort by count descending, take top 25
            let mut sorted: Vec<(String, usize)> = tag_counts.into_iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(&a.1));
            let tags: Vec<String> = sorted.into_iter().take(25).map(|(tag, _)| tag).collect();

            Ok::<_, rusqlite::Error>(tags)
        })
        .await;

    match result {
        Ok(tags) => Json(serde_json::json!({ "tags": tags })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ============================================================================
// AI Features
// ============================================================================

use crate::ai::cli::detect_claude_code;
use crate::ai::title::{generate_title, store_title};
use crate::ai::types::AiEvent;

/// Get AI CLI detection status
pub async fn get_ai_cli_status() -> impl IntoResponse {
    let detected = detect_claude_code().await;
    Json(serde_json::json!({
        "provider": "claude_code",
        "installed": detected.installed,
        "path": detected.path,
        "version": detected.version,
    }))
}

/// Get AI settings
pub async fn get_ai_settings(State(state): State<AppState>) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT enabled, selected_provider, privacy_accepted FROM ai_settings WHERE id = 1",
                [],
                |row| {
                    Ok(serde_json::json!({
                        "enabled": row.get::<_, bool>(0)?,
                        "selected_provider": row.get::<_, Option<String>>(1)?,
                        "privacy_accepted": row.get::<_, bool>(2)?,
                    }))
                },
            )
        })
        .await;

    match result {
        Ok(settings) => Json(settings).into_response(),
        Err(_) => {
            // Return defaults if not found
            Json(serde_json::json!({
                "enabled": true,
                "selected_provider": "claude_code",
                "privacy_accepted": false,
            }))
            .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateAiSettingsRequest {
    pub enabled: Option<bool>,
    pub selected_provider: Option<String>,
    pub privacy_accepted: Option<bool>,
}

/// Update AI settings
pub async fn update_ai_settings(
    State(state): State<AppState>,
    Json(req): Json<UpdateAiSettingsRequest>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE ai_settings SET
                    enabled = COALESCE(?, enabled),
                    selected_provider = COALESCE(?, selected_provider),
                    privacy_accepted = COALESCE(?, privacy_accepted),
                    updated_at = ?
                 WHERE id = 1",
                rusqlite::params![req.enabled, req.selected_provider, req.privacy_accepted, now],
            )
        })
        .await;

    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Accept AI privacy warning
pub async fn accept_ai_privacy(State(state): State<AppState>) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(|conn| {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE ai_settings SET privacy_accepted = 1, updated_at = ? WHERE id = 1",
                [&now],
            )
        })
        .await;

    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ============================================================================
// AI Export
// ============================================================================

/// Provider capabilities for export decisions
#[derive(Debug, serde::Serialize)]
pub struct ProviderCapabilities {
    pub max_content_size: usize,
    pub timeout_secs: u64,
    pub supports_chunking: bool,
}

/// Get AI export capabilities
pub async fn get_ai_export_capabilities() -> impl IntoResponse {
    // Return capabilities for Claude Code as default provider
    Json(ProviderCapabilities {
        max_content_size: 100_000,
        timeout_secs: 120,
        supports_chunking: true,
    })
}

#[derive(Debug, Deserialize)]
pub struct GenerateExportRequest {
    pub format: String,
    pub raw_content: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ExportResult {
    pub content: String,
    pub format: String,
    pub provider: String,
    pub generation_time_ms: u64,
}

/// Generate AI export (stub - returns raw content for now)
pub async fn generate_ai_export(Json(req): Json<GenerateExportRequest>) -> impl IntoResponse {
    // For now, just return the raw content
    // Full implementation would use Claude Code to summarize
    Json(ExportResult {
        content: req.raw_content,
        format: req.format,
        provider: "passthrough".to_string(),
        generation_time_ms: 0,
    })
}

#[derive(Debug, Deserialize)]
pub struct ChunkRequest {
    pub format: String,
    pub chunk_content: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub is_first: bool,
    pub is_last: bool,
    pub target_output_chars: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct ChunkResult {
    pub content: String,
    pub chunk_index: usize,
    pub provider: String,
    pub generation_time_ms: u64,
}

/// Process AI export chunk (stub)
pub async fn process_ai_export_chunk(Json(req): Json<ChunkRequest>) -> impl IntoResponse {
    Json(ChunkResult {
        content: req.chunk_content,
        chunk_index: req.chunk_index,
        provider: "passthrough".to_string(),
        generation_time_ms: 0,
    })
}

#[derive(Debug, Deserialize)]
pub struct MergeRequest {
    pub format: String,
    pub partial_results: Vec<String>,
}

/// Merge AI export chunks (stub)
pub async fn merge_ai_export_chunks(Json(req): Json<MergeRequest>) -> impl IntoResponse {
    Json(ExportResult {
        content: req.partial_results.join("\n\n"),
        format: req.format,
        provider: "passthrough".to_string(),
        generation_time_ms: 0,
    })
}

// ============================================================================
// Session Limit
// ============================================================================

#[derive(Debug, serde::Serialize)]
pub struct SessionLimitInfo {
    pub count: i64,
    pub limit: i64,
    pub remaining: i64,
    pub at_limit: bool,
}

/// Get session limit info (all features now free - unlimited)
pub async fn get_session_limit_info(State(state): State<AppState>) -> impl IntoResponse {
    let count = state
        .db
        .with_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get::<_, i64>(0))
        })
        .await
        .unwrap_or(0);

    // Unlimited sessions
    Json(SessionLimitInfo {
        count,
        limit: -1, // -1 means unlimited
        remaining: -1,
        at_limit: false,
    })
}

#[derive(Debug, Deserialize)]
pub struct TitleGenerationRequest {
    #[serde(default)]
    pub force: bool,
}

/// Trigger title generation for a session (async, returns immediately)
pub async fn trigger_title_generation(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    body: Option<Json<TitleGenerationRequest>>,
) -> impl IntoResponse {
    let force = body.map(|b| b.force).unwrap_or(false);

    // Check if session exists and has title (unless force)
    if !force {
        let session_id_clone = session_id.clone();
        let has_title = state
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT title FROM sessions WHERE id = ?",
                    [&session_id_clone],
                    |row| {
                        let title: Option<String> = row.get(0)?;
                        Ok(title.is_some() && title.unwrap().len() > 0)
                    },
                )
            })
            .await;

        match has_title {
            Ok(true) => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": "skipped",
                        "message": "Session already has a title"
                    })),
                )
                    .into_response()
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": "Session not found" })),
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
            Ok(false) => {} // Continue with generation
        }
    }

    // Acquire task queue permit
    let permit = match state.ai_task_queue.acquire().await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    // Clone values for the spawned task
    let db = state.db.clone();
    let ai_event_tx = state.ai_event_tx.clone();
    let session_id_for_task = session_id.clone();

    // Spawn background task for title generation
    tokio::spawn(async move {
        // Keep permit alive during task execution
        let _permit = permit;

        // Emit start event
        let _ = ai_event_tx.send(AiEvent::TitleStart {
            session_id: session_id_for_task.clone(),
        });

        // Generate title
        let result = generate_title(&db, &session_id_for_task, None).await;

        // Store result and emit event
        if let Some(ref title) = result.title {
            if let Err(e) = store_title(&db, &session_id_for_task, title).await {
                tracing::error!("Failed to store title: {}", e);
                let _ = ai_event_tx.send(AiEvent::TitleError {
                    session_id: session_id_for_task,
                    error: format!("Failed to store title: {}", e),
                });
                return;
            }
            let _ = ai_event_tx.send(AiEvent::TitleComplete {
                session_id: session_id_for_task,
                title: title.clone(),
            });
        } else if let Some(error) = result.error {
            let _ = ai_event_tx.send(AiEvent::TitleError {
                session_id: session_id_for_task,
                error,
            });
        }
    });

    // Return immediately with 202 Accepted
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "started",
            "session_id": session_id,
            "message": "Title generation started. Listen to SSE for progress."
        })),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct MemoryExtractionRequest {
    #[serde(default)]
    pub force: bool,
}

/// Trigger memory extraction for a session (async, returns immediately)
pub async fn trigger_memory_extraction(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    body: Option<Json<MemoryExtractionRequest>>,
) -> impl IntoResponse {
    let _force = body.map(|b| b.force).unwrap_or(false);

    // Verify session exists
    let session_id_clone = session_id.clone();
    let session_exists = state
        .db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT 1 FROM sessions WHERE id = ?",
                [&session_id_clone],
                |_| Ok(true),
            )
        })
        .await;

    match session_exists {
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Session not found" })),
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
        Ok(_) => {}
    }

    // Acquire task queue permit
    let permit = match state.ai_task_queue.acquire().await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    // Clone values for the spawned task
    let db = state.db.clone();
    let ai_event_tx = state.ai_event_tx.clone();
    let session_id_for_task = session_id.clone();

    // Spawn background task for memory extraction
    tokio::spawn(async move {
        // Keep permit alive during task execution
        let _permit = permit;

        // Emit start event
        let _ = ai_event_tx.send(AiEvent::MemoryStart {
            session_id: session_id_for_task.clone(),
        });

        // Extract memories
        let result = crate::ai::extract_memories(&db, &session_id_for_task, None).await;

        // Emit completion or error event
        if let Some(error) = result.error {
            let _ = ai_event_tx.send(AiEvent::MemoryError {
                session_id: session_id_for_task,
                error,
            });
        } else {
            let _ = ai_event_tx.send(AiEvent::MemoryComplete {
                session_id: session_id_for_task,
                count: result.memories_extracted,
            });
        }
    });

    // Return immediately with 202 Accepted
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "started",
            "session_id": session_id,
            "message": "Memory extraction started. Listen to SSE for progress."
        })),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SkillExtractionRequest {
    #[serde(default)]
    pub force: bool,
}

/// Trigger skill extraction for a session (async, returns immediately)
pub async fn trigger_skill_extraction(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    body: Option<Json<SkillExtractionRequest>>,
) -> impl IntoResponse {
    let _force = body.map(|b| b.force).unwrap_or(false);

    // Verify session exists
    let session_id_clone = session_id.clone();
    let session_exists = state
        .db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT 1 FROM sessions WHERE id = ?",
                [&session_id_clone],
                |_| Ok(true),
            )
        })
        .await;

    match session_exists {
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Session not found" })),
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
        Ok(_) => {}
    }

    // Acquire task queue permit
    let permit = match state.ai_task_queue.acquire().await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    // Clone values for the spawned task
    let db = state.db.clone();
    let ai_event_tx = state.ai_event_tx.clone();
    let session_id_for_task = session_id.clone();

    // Spawn background task for skill extraction
    tokio::spawn(async move {
        // Keep permit alive during task execution
        let _permit = permit;

        // Emit start event
        let _ = ai_event_tx.send(AiEvent::SkillStart {
            session_id: session_id_for_task.clone(),
        });

        // Extract skills
        let result = crate::ai::extract_skills(&db, &session_id_for_task, None).await;

        // Emit completion or error event
        if let Some(error) = result.error {
            let _ = ai_event_tx.send(AiEvent::SkillError {
                session_id: session_id_for_task,
                error,
            });
        } else {
            let _ = ai_event_tx.send(AiEvent::SkillComplete {
                session_id: session_id_for_task,
                count: result.skills_extracted,
            });
        }
    });

    // Return immediately with 202 Accepted
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "started",
            "session_id": session_id,
            "message": "Skill extraction started. Listen to SSE for progress."
        })),
    )
        .into_response()
}

// ============================================================================
// Markers
// ============================================================================

/// Get markers for a session
pub async fn get_session_markers(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| crate::ai::marker::get_markers(conn, &session_id))
        .await;

    match result {
        Ok(markers) => Json(serde_json::json!({ "markers": markers })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Delete a marker by ID
pub async fn delete_marker(
    State(state): State<AppState>,
    Path(marker_id): Path<i64>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| crate::ai::marker::delete_marker_by_id(conn, marker_id))
        .await;

    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) if e.contains("not found") => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Marker not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Trigger marker detection for a session (async, returns immediately)
pub async fn trigger_marker_detection(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    // Verify session exists
    let session_id_clone = session_id.clone();
    let session_exists = state
        .db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT 1 FROM sessions WHERE id = ?",
                [&session_id_clone],
                |_| Ok(true),
            )
        })
        .await;

    match session_exists {
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Session not found" })),
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
        Ok(_) => {}
    }

    // Acquire task queue permit
    let permit = match state.ai_task_queue.acquire().await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    // Clone values for the spawned task
    let db = state.db.clone();
    let ai_event_tx = state.ai_event_tx.clone();
    let session_id_for_task = session_id.clone();

    // Spawn background task for marker detection
    tokio::spawn(async move {
        // Keep permit alive during task execution
        let _permit = permit;

        // Emit start event
        let _ = ai_event_tx.send(AiEvent::MarkerStart {
            session_id: session_id_for_task.clone(),
        });

        // Detect CLI
        let cli = crate::ai::cli::detect_cli();

        // Run marker detection
        let result = crate::ai::detect_markers(&db, &session_id_for_task, cli).await;

        // Emit completion event
        let _ = ai_event_tx.send(AiEvent::MarkerComplete {
            session_id: session_id_for_task,
            count: result.markers_detected,
        });
    });

    // Return immediately with 202 Accepted
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "started",
            "session_id": session_id,
            "message": "Marker detection started. Listen to SSE for progress."
        })),
    )
        .into_response()
}

// ============================================================================
// Memory Ranking
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct RankMemoriesQuery {
    pub batch_size: Option<usize>,
}

/// Trigger memory ranking for a project
pub async fn rank_project_memories(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Query(query): Query<RankMemoriesQuery>,
) -> impl IntoResponse {
    let batch_size = query.batch_size.unwrap_or(500);
    let project_id_clone = project_id.clone();

    // Verify project exists
    let exists = state
        .db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT 1 FROM projects WHERE id = ?",
                [&project_id_clone],
                |_| Ok(true),
            )
            .unwrap_or(false)
        })
        .await;

    if !exists {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Project not found"
            })),
        )
            .into_response();
    }

    // Run ranking in spawn_blocking since it uses sync database access
    let db = state.db.clone();
    let project_id_for_ranking = project_id.clone();

    let result = tokio::task::spawn_blocking(move || {
        crate::ai::ranking::rank_project_memories(&db, &project_id_for_ranking, batch_size)
    })
    .await;

    match result {
        Ok(Ok(ranking_result)) => Json(serde_json::json!({
            "project_id": ranking_result.project_id,
            "memories_evaluated": ranking_result.memories_evaluated,
            "promoted": ranking_result.promoted,
            "demoted": ranking_result.demoted,
            "removed": ranking_result.removed,
            "unchanged": ranking_result.unchanged,
            "transitions": ranking_result.transitions,
        }))
        .into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": e
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Task panicked: {}", e)
            })),
        )
            .into_response(),
    }
}

/// Get ranking statistics for a project
pub async fn get_ranking_stats(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let project_id_clone = project_id.clone();

    // Verify project exists
    let exists = state
        .db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT 1 FROM projects WHERE id = ?",
                [&project_id_clone],
                |_| Ok(true),
            )
            .unwrap_or(false)
        })
        .await;

    if !exists {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Project not found"
            })),
        )
            .into_response();
    }

    // Run stats query in spawn_blocking since it uses sync database access
    let db = state.db.clone();
    let project_id_for_stats = project_id.clone();

    let result = tokio::task::spawn_blocking(move || {
        crate::ai::ranking::get_ranking_stats(&db, &project_id_for_stats)
    })
    .await;

    match result {
        Ok(Ok(stats)) => Json(stats).into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": e
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Task panicked: {}", e)
            })),
        )
            .into_response(),
    }
}

// ============================================================================
// Skills
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ListSkillsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub sort_by: Option<String>,
}

/// Session reference for skill frequency tracking
#[derive(Debug, serde::Serialize)]
pub struct SessionRef {
    pub id: String,
    pub title: Option<String>,
}

/// Skill with frequency information
#[derive(Debug, serde::Serialize)]
pub struct SkillWithFrequency {
    pub id: i64,
    pub project_id: String,
    pub session_id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<String>,
    pub confidence: f64,
    pub extracted_at: String,
    pub frequency: usize,
    pub sessions: Vec<SessionRef>,
}

/// List skills for a project with pagination and sorting
pub async fn list_project_skills(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Query(query): Query<ListSkillsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);
    let sort_by = query.sort_by.clone();

    let result = state
        .db
        .with_conn(move |conn| {
            // First, get the total count
            let total: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM skills WHERE project_id = ?",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // For frequency sorting, we need to use a subquery to count linked sessions
            let is_frequency_sort = sort_by.as_deref() == Some("frequency");

            let sql = if is_frequency_sort {
                // Frequency = 1 (original session) + count of linked sessions
                // Only use subquery in ORDER BY, select same columns as non-frequency query
                "SELECT s.id, s.project_id, s.session_id, s.name, s.description, s.steps, s.confidence, s.extracted_at
                 FROM skills s
                 WHERE s.project_id = ?
                 ORDER BY (1 + COALESCE((SELECT COUNT(*) FROM skill_sessions WHERE skill_id = s.id), 0)) DESC, s.extracted_at DESC
                 LIMIT ? OFFSET ?".to_string()
            } else {
                let order_clause = match sort_by.as_deref() {
                    Some("date_newest") => "ORDER BY extracted_at DESC",
                    Some("date_oldest") => "ORDER BY extracted_at ASC",
                    Some("confidence") => "ORDER BY confidence DESC",
                    _ => "ORDER BY extracted_at DESC",
                };
                format!(
                    "SELECT id, project_id, session_id, name, description, steps, confidence, extracted_at
                     FROM skills
                     WHERE project_id = ?
                     {}
                     LIMIT ? OFFSET ?",
                    order_clause
                )
            };
            let mut stmt = conn.prepare(&sql)?;

            let skill_rows: Vec<(i64, String, String, String, String, String, f64, String)> = stmt
                .query_map(rusqlite::params![project_id, limit, offset], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            // Collect all session IDs for batch lookup
            let mut all_session_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut skill_session_map: std::collections::HashMap<i64, Vec<String>> =
                std::collections::HashMap::new();

            for (skill_id, _, session_id, _, _, _, _, _) in &skill_rows {
                all_session_ids.insert(session_id.clone());

                // Also get linked sessions from skill_sessions table
                let mut linked: Vec<String> = vec![session_id.clone()];
                if let Ok(mut link_stmt) =
                    conn.prepare("SELECT session_id FROM skill_sessions WHERE skill_id = ?")
                {
                    if let Ok(rows) = link_stmt.query_map([skill_id], |row| row.get::<_, String>(0)) {
                        for row in rows.flatten() {
                            if !linked.contains(&row) {
                                linked.push(row.clone());
                                all_session_ids.insert(row);
                            }
                        }
                    }
                }
                skill_session_map.insert(*skill_id, linked);
            }

            // Batch fetch session titles
            let session_titles: std::collections::HashMap<String, Option<String>> =
                if !all_session_ids.is_empty() {
                    let ids: Vec<&String> = all_session_ids.iter().collect();
                    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                    let query = format!("SELECT id, title FROM sessions WHERE id IN ({})", placeholders);

                    let mut stmt = conn.prepare(&query).unwrap();
                    let params: Vec<&dyn rusqlite::ToSql> =
                        ids.iter().map(|s| *s as &dyn rusqlite::ToSql).collect();

                    stmt.query_map(params.as_slice(), |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                    })
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default()
                } else {
                    std::collections::HashMap::new()
                };

            // Build final skills list
            let skills: Vec<SkillWithFrequency> = skill_rows
                .into_iter()
                .map(|(id, proj_id, sess_id, name, desc, steps_json, conf, extracted)| {
                    let steps: Vec<String> =
                        serde_json::from_str(&steps_json).unwrap_or_default();

                    let session_ids = skill_session_map
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| vec![sess_id.clone()]);

                    let sessions: Vec<SessionRef> = session_ids
                        .iter()
                        .map(|sid| SessionRef {
                            id: sid.clone(),
                            title: session_titles.get(sid).cloned().flatten(),
                        })
                        .collect();

                    SkillWithFrequency {
                        id,
                        project_id: proj_id,
                        session_id: sess_id,
                        name,
                        description: desc,
                        steps,
                        confidence: conf,
                        extracted_at: extracted,
                        frequency: sessions.len(),
                        sessions,
                    }
                })
                .collect();

            Ok::<_, rusqlite::Error>((skills, total))
        })
        .await;

    match result {
        Ok((skills, total)) => Json(serde_json::json!({
            "skills": skills,
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

/// Get skill statistics for a project
pub async fn get_skill_stats(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| {
            let total_skills: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM skills WHERE project_id = ?",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let sessions_with_skills: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT session_id) FROM skills WHERE project_id = ?",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let unique_skills: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT name) FROM skills WHERE project_id = ?",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            Ok::<_, rusqlite::Error>(serde_json::json!({
                "total_skills": total_skills,
                "unique_skills": unique_skills,
                "sessions_with_skills": sessions_with_skills,
            }))
        })
        .await;

    match result {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Delete a skill by ID
pub async fn delete_skill_by_id(
    State(state): State<AppState>,
    Path(skill_id): Path<i64>,
) -> impl IntoResponse {
    let result = state
        .db
        .with_conn(move |conn| conn.execute("DELETE FROM skills WHERE id = ?", [skill_id]))
        .await;

    match result {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Skill not found" })),
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
