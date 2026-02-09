//! Context API routes
//!
//! Provides endpoints for LLM skills and hooks to access project/session context.
//! These mirror the MCP tools but use HTTP, enabling remote access and simpler integration.
//! All handlers use McpDb (from mcp::db) inside spawn_blocking for DB operations.

use super::AppState;
use crate::mcp::db::McpDb;
use crate::mcp::types::{Memory, MemoryType, SessionContextResult};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;

// ============================================================================
// Request types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ProjectContextQuery {
    pub project_path: String,
}

#[derive(Debug, Deserialize)]
pub struct SessionContextRequest {
    pub session_id: String,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RecentMemoriesQuery {
    pub project_path: String,
    #[serde(default = "default_sessions")]
    pub sessions: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct SaveLifeboatRequest {
    pub session_id: String,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchContextRequest {
    #[serde(default)]
    pub query: Option<String>,
    pub project_path: String,
    #[serde(default)]
    pub memory_types: Option<Vec<String>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_sessions() -> usize {
    3
}
fn default_limit() -> usize {
    10
}

// ============================================================================
// Formatting helpers (produce same markdown as MCP handlers)
// ============================================================================

fn format_memory_line(m: &Memory) -> String {
    format!(
        "- **[{}] {}**: {}\n",
        m.memory_type.display_name(),
        m.title,
        m.content
    )
}

fn format_project_context(
    project_name: &str,
    decisions: &[Memory],
    facts: &[Memory],
    preferences: &[Memory],
    context: &[Memory],
    tasks: &[Memory],
    total: usize,
) -> String {
    if total == 0 {
        return format!("No memories found for project '{}'.", project_name);
    }

    let mut output = format!(
        "Project Context for '{}' ({} memories):\n\n",
        project_name, total
    );

    let sections: &[(&str, &[Memory])] = &[
        ("## Key Decisions", decisions),
        ("## Facts & Discoveries", facts),
        ("## Preferences", preferences),
        ("## Context", context),
        ("## Tasks", tasks),
    ];

    for (heading, memories) in sections {
        if !memories.is_empty() {
            output.push_str(heading);
            output.push('\n');
            for m in *memories {
                output.push_str(&format!("- **{}**: {}\n", m.title, m.content));
            }
            output.push('\n');
        }
    }

    output
}

fn format_session_context(project_name: &str, result: &SessionContextResult) -> String {
    let mut output = format!("## Session Context for {}\n\n", project_name);

    output.push_str("### Current State\n");
    if let Some(task) = &result.session.active_task {
        output.push_str(&format!("**Active Task:** {}\n", task));
    }
    if let Some(resume) = &result.session.resume_context {
        output.push_str(&format!("**Resume Context:** {}\n", resume));
    }
    if !result.session.recent_decisions.is_empty() {
        output.push_str("\n**Recent Decisions:**\n");
        for decision in &result.session.recent_decisions {
            output.push_str(&format!("- {}\n", decision));
        }
    }
    if !result.session.open_questions.is_empty() {
        output.push_str("\n**Open Questions:**\n");
        for question in &result.session.open_questions {
            output.push_str(&format!("- {}\n", question));
        }
    }

    if !result.persistent_memories.is_empty() {
        output.push_str("\n### Persistent Knowledge (High Importance)\n");
        for m in result.persistent_memories.iter().take(5) {
            output.push_str(&format_memory_line(m));
        }
    }

    if !result.session_memories.is_empty() {
        output.push_str("\n### This Session's Memories\n");
        for m in result.session_memories.iter().take(5) {
            output.push_str(&format_memory_line(m));
        }
    }

    if !result.recent_memories.is_empty() {
        output.push_str("\n### Recent Memories (Last 3 Sessions)\n");
        for m in result.recent_memories.iter().take(5) {
            output.push_str(&format_memory_line(m));
        }
    }

    output
}

fn format_recent_memories(project_name: &str, sessions: usize, memories: &[Memory]) -> String {
    if memories.is_empty() {
        return format!(
            "No memories extracted from recent {} sessions in project '{}'.",
            sessions, project_name
        );
    }

    let mut output = format!(
        "Found {} memories from last {} sessions in project '{}':\n\n",
        memories.len(),
        sessions,
        project_name
    );

    for (i, m) in memories.iter().enumerate() {
        output.push_str(&format!(
            "{}. [{}] {} (confidence: {:.0}%)\n",
            i + 1,
            m.memory_type.display_name(),
            m.title,
            m.confidence * 100.0
        ));
        output.push_str(&format!("   {}\n\n", m.content));
    }

    output
}

fn format_search_results(project_name: &str, query: Option<&str>, memories: &[Memory]) -> String {
    if memories.is_empty() {
        return match query {
            Some(q) => format!(
                "No memories found for query '{}' in project '{}'.",
                q, project_name
            ),
            None => format!("No memories found in project '{}'.", project_name),
        };
    }

    let mut output = match query {
        Some(q) => format!(
            "Found {} memories in project '{}' for query '{}':\n\n",
            memories.len(),
            project_name,
            q
        ),
        None => format!(
            "Found {} memories in project '{}':\n\n",
            memories.len(),
            project_name
        ),
    };

    for (i, m) in memories.iter().enumerate() {
        output.push_str(&format!(
            "{}. [{}] {} (confidence: {:.0}%)\n",
            i + 1,
            m.memory_type.display_name(),
            m.title,
            m.confidence * 100.0
        ));
        output.push_str(&format!("   {}\n", m.content));
        if !m.tags.is_empty() {
            output.push_str(&format!("   Tags: {}\n", m.tags.join(", ")));
        }
        output.push('\n');
    }

    output
}

// ============================================================================
// Helper: resolve project by path with error response
// ============================================================================

fn resolve_project(
    db: &McpDb,
    project_path: &str,
) -> Result<crate::mcp::types::Project, (StatusCode, serde_json::Value)> {
    match db.get_project_by_path_prefix(project_path) {
        Ok(Some(p)) => Ok(p),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            json!({ "error": format!("No project found for path: {}", project_path) }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": format!("Database error: {}", e) }),
        )),
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/context/project?project_path=...
/// Returns project context with categorized memories
pub async fn get_project_context(
    State(state): State<AppState>,
    Query(query): Query<ProjectContextQuery>,
) -> impl IntoResponse {
    let db = state.db.clone();
    let project_path = query.project_path;

    let result = tokio::task::spawn_blocking(move || {
        let mcp_db = McpDb::new(db);
        let project = resolve_project(&mcp_db, &project_path)?;

        let decisions = mcp_db
            .get_memories_by_type(&project.id, MemoryType::Decision, 5)
            .unwrap_or_default();
        let facts = mcp_db
            .get_memories_by_type(&project.id, MemoryType::Fact, 5)
            .unwrap_or_default();
        let preferences = mcp_db
            .get_memories_by_type(&project.id, MemoryType::Preference, 5)
            .unwrap_or_default();
        let context_memories = mcp_db
            .get_memories_by_type(&project.id, MemoryType::Context, 5)
            .unwrap_or_default();
        let tasks = mcp_db
            .get_memories_by_type(&project.id, MemoryType::Task, 5)
            .unwrap_or_default();

        let total = decisions.len()
            + facts.len()
            + preferences.len()
            + context_memories.len()
            + tasks.len();

        // Track access for all returned memories (feeds into ranking)
        let all_ids: Vec<i64> = [&decisions, &facts, &preferences, &context_memories, &tasks]
            .iter()
            .flat_map(|v| v.iter().map(|m| m.id))
            .collect();
        if !all_ids.is_empty() {
            let _ = mcp_db.track_memory_access(&all_ids);
        }

        let formatted = format_project_context(
            &project.name,
            &decisions,
            &facts,
            &preferences,
            &context_memories,
            &tasks,
            total,
        );

        Ok::<_, (StatusCode, serde_json::Value)>(json!({
            "project_name": project.name,
            "decisions": decisions,
            "facts": facts,
            "preferences": preferences,
            "context": context_memories,
            "tasks": tasks,
            "total_memories": total,
            "formatted_text": formatted,
        }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err((status, err))) => (status, Json(err)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/context/session
/// Returns session context with state, session memories, recent memories, persistent memories
pub async fn get_session_context(
    State(state): State<AppState>,
    Json(req): Json<SessionContextRequest>,
) -> impl IntoResponse {
    let db = state.db.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mcp_db = McpDb::new(db);
        let project_path = req.project_path.as_deref().unwrap_or(".");
        let project = resolve_project(&mcp_db, project_path)?;

        let session_context = mcp_db
            .get_or_create_session_context(&req.session_id, &project.id, "startup")
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": format!("Failed to get session context: {}", e) }),
                )
            })?;

        let session_memories = mcp_db
            .get_memories_by_sessions(std::slice::from_ref(&req.session_id), 20)
            .unwrap_or_default();

        let recent_session_ids = mcp_db
            .get_recent_sessions_with_context(&project.id, &req.session_id, 3)
            .unwrap_or_default();

        let recent_memories = if !recent_session_ids.is_empty() {
            mcp_db
                .get_memories_by_sessions(&recent_session_ids, 15)
                .unwrap_or_default()
        } else {
            vec![]
        };

        let persistent_memories = mcp_db
            .get_persistent_memories(&project.id, 20)
            .unwrap_or_default();

        // Track access
        let all_ids: Vec<i64> = session_memories
            .iter()
            .chain(recent_memories.iter())
            .chain(persistent_memories.iter())
            .map(|m| m.id)
            .collect();
        let _ = mcp_db.track_memory_access(&all_ids);

        let ctx_result = SessionContextResult {
            session: session_context,
            session_memories,
            recent_memories,
            persistent_memories,
        };

        let formatted = format_session_context(&project.name, &ctx_result);

        Ok::<_, (StatusCode, serde_json::Value)>(json!({
            "session": ctx_result.session,
            "session_memories": ctx_result.session_memories,
            "recent_memories": ctx_result.recent_memories,
            "persistent_memories": ctx_result.persistent_memories,
            "project_name": project.name,
            "formatted_text": formatted,
        }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err((status, err))) => (status, Json(err)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/context/recent-memories?project_path=...&sessions=3&limit=10
/// Returns memories from recent sessions
pub async fn get_recent_memories(
    State(state): State<AppState>,
    Query(query): Query<RecentMemoriesQuery>,
) -> impl IntoResponse {
    let db = state.db.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mcp_db = McpDb::new(db);
        let project = resolve_project(&mcp_db, &query.project_path)?;

        let session_ids = mcp_db
            .get_recent_sessions(&project.id, query.sessions)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": format!("Failed to get sessions: {}", e) }),
                )
            })?;

        let memories = if session_ids.is_empty() {
            vec![]
        } else {
            mcp_db
                .get_memories_by_sessions(&session_ids, query.limit)
                .unwrap_or_default()
        };

        let formatted = format_recent_memories(&project.name, query.sessions, &memories);

        Ok::<_, (StatusCode, serde_json::Value)>(json!({
            "project_name": project.name,
            "memories": memories,
            "formatted_text": formatted,
        }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err((status, err))) => (status, Json(err)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/context/lifeboat
/// Save session state before context compaction
pub async fn save_lifeboat(
    State(state): State<AppState>,
    Json(req): Json<SaveLifeboatRequest>,
) -> impl IntoResponse {
    let db = state.db.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mcp_db = McpDb::new(db);

        let existing = mcp_db.get_session_context(&req.session_id).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": format!("Database error: {}", e) }),
            )
        })?;

        let existing = existing.ok_or_else(|| (
            StatusCode::NOT_FOUND,
            json!({ "error": format!("No session context found for session {}", req.session_id) }),
        ))?;

        // Auto-generate summary if not provided
        let summary = req.summary.unwrap_or_else(|| {
            let mut parts = vec![];
            if let Some(task) = &existing.active_task {
                parts.push(format!("Task: {}", task));
            }
            if !existing.recent_decisions.is_empty() {
                parts.push(format!("Decisions: {}", existing.recent_decisions.len()));
            }
            if !existing.open_questions.is_empty() {
                parts.push(format!("Questions: {}", existing.open_questions.join(", ")));
            }
            if parts.is_empty() {
                "Session context saved".to_string()
            } else {
                parts.join("; ")
            }
        });

        mcp_db
            .save_lifeboat(&req.session_id, &summary)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": format!("Failed to save lifeboat: {}", e) }),
                )
            })?;

        Ok::<_, (StatusCode, serde_json::Value)>(json!({
            "success": true,
            "session_id": req.session_id,
            "summary": summary,
        }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err((status, err))) => (status, Json(err)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/context/search
/// Search memories with path-based project resolution, type and tag filtering
pub async fn search_context(
    State(state): State<AppState>,
    Json(req): Json<SearchContextRequest>,
) -> impl IntoResponse {
    let db = state.db.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mcp_db = McpDb::new(db);
        let project = resolve_project(&mcp_db, &req.project_path)?;

        let memory_types: Option<Vec<MemoryType>> = req.memory_types.map(|types| {
            types
                .iter()
                .filter_map(|t| MemoryType::from_str(t))
                .collect()
        });

        let tag_filters = req.tags.as_deref();

        // Fetch more when tag filtering needed (post-query filter)
        let fetch_limit = if tag_filters.is_some() {
            req.limit * 5
        } else {
            req.limit
        };

        let query_str = req.query.as_deref().unwrap_or("").trim();

        let results = if query_str.is_empty() {
            mcp_db
                .browse_memories(&project.id, memory_types.as_deref(), fetch_limit)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": e })))?
        } else {
            match mcp_db.search_memories_hybrid(
                query_str,
                &project.id,
                memory_types.as_deref(),
                fetch_limit,
            ) {
                Ok(r) => r,
                Err(e) => {
                    tracing::debug!("Hybrid search failed, falling back to FTS5: {}", e);
                    mcp_db
                        .search_memories_fts(
                            query_str,
                            &project.id,
                            memory_types.as_deref(),
                            fetch_limit,
                        )
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": e })))?
                }
            }
        };

        // Apply tag filtering post-query
        let filtered: Vec<Memory> = if let Some(tags) = tag_filters {
            results
                .into_iter()
                .filter(|m| tags.iter().all(|tag| m.tags.iter().any(|t| t == tag)))
                .take(req.limit)
                .collect()
        } else {
            results.into_iter().take(req.limit).collect()
        };

        // Track access for returned memories (feeds into ranking)
        let memory_ids: Vec<i64> = filtered.iter().map(|m| m.id).collect();
        if !memory_ids.is_empty() {
            let _ = mcp_db.track_memory_access(&memory_ids);
        }

        let query_opt = if query_str.is_empty() {
            None
        } else {
            Some(query_str)
        };
        let formatted = format_search_results(&project.name, query_opt, &filtered);

        Ok::<_, (StatusCode, serde_json::Value)>(json!({
            "project_name": project.name,
            "memories": filtered,
            "formatted_text": formatted,
        }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err((status, err))) => (status, Json(err)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
