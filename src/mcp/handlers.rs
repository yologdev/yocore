//! MCP tool handlers
//! Implements the tools/list and tools/call methods

use serde_json::{json, Value};

use super::db::McpDb;
use super::protocol::{
    InitializeResult, JsonRpcError, JsonRpcResponse, ResourceDefinition, ServerCapabilities,
    ServerInfo, ToolCallResult, ToolDefinition, ToolsCapability,
};
use super::types::{
    GetProjectContextParams, GetRecentMemoriesParams, GetSessionContextParams, MemoryType,
    ProjectContext, SaveLifeboatParams, SearchMemoriesParams, SessionContextResult,
};

/// Handle the initialize method
pub fn handle_initialize(id: Value) -> JsonRpcResponse {
    let result = InitializeResult {
        protocol_version: "2024-11-05".to_string(),
        capabilities: ServerCapabilities {
            tools: ToolsCapability {
                list_changed: Some(false),
            },
            resources: None,
        },
        server_info: ServerInfo {
            name: "yocore".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    };

    match serde_json::to_value(result) {
        Ok(value) => JsonRpcResponse::success(id, value),
        Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(e.to_string())),
    }
}

/// Handle the tools/list method
pub fn handle_tools_list(id: Value) -> JsonRpcResponse {
    let tools = vec![
        ToolDefinition {
            name: "yolog_search_memories".to_string(),
            description: "Search and browse project memories. Use with a query for semantic+keyword search, or without a query to browse/filter by type and tags. Returns decisions, facts, preferences, context, and tasks from past sessions.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (optional — omit to browse/filter without keyword search)"
                    },
                    "project_path": {
                        "type": "string",
                        "description": "Project directory path (defaults to current working directory)"
                    },
                    "memory_types": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["decision", "fact", "preference", "context", "task"]
                        },
                        "description": "Filter by memory types (e.g., [\"decision\", \"fact\"])"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter by tags — memories must contain ALL specified tags (AND logic)"
                    },
                    "limit": {
                        "type": "integer",
                        "default": 10,
                        "description": "Maximum number of results"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "yolog_get_project_context".to_string(),
            description: "Get high-level project context including key decisions, facts, and preferences.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_path": {
                        "type": "string",
                        "description": "Project directory path"
                    }
                },
                "required": ["project_path"]
            }),
        },
        ToolDefinition {
            name: "yolog_get_recent_memories".to_string(),
            description: "Get memories from the most recent coding sessions.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_path": {
                        "type": "string",
                        "description": "Project directory path"
                    },
                    "sessions": {
                        "type": "integer",
                        "default": 3,
                        "description": "Number of recent sessions to include"
                    },
                    "limit": {
                        "type": "integer",
                        "default": 10,
                        "description": "Maximum number of memories"
                    }
                },
                "required": ["project_path"]
            }),
        },
        ToolDefinition {
            name: "yolog_get_session_context".to_string(),
            description: "Get session context including current task state, recent decisions, and relevant memories.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Claude Code session ID (from YOLOG_SESSION_ID env var)"
                    },
                    "project_path": {
                        "type": "string",
                        "description": "Project directory path (optional)"
                    }
                },
                "required": ["session_id"]
            }),
        },
        ToolDefinition {
            name: "yolog_save_lifeboat".to_string(),
            description: "Emergency save of session state before context compaction.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Claude Code session ID"
                    },
                    "summary": {
                        "type": "string",
                        "description": "Brief summary of current work state"
                    }
                },
                "required": ["session_id"]
            }),
        },
    ];

    JsonRpcResponse::success(id, json!({ "tools": tools }))
}

/// Handle the tools/call method
pub fn handle_tools_call(id: Value, params: Option<Value>, db: &McpDb) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::invalid_params("Missing params".to_string()),
            );
        }
    };

    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(name) => name,
        None => {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::invalid_params("Missing tool name".to_string()),
            );
        }
    };

    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    let result = match tool_name {
        "yolog_search_memories" => handle_search_memories(arguments, db),
        "yolog_get_project_context" => handle_get_project_context(arguments, db),
        "yolog_get_recent_memories" => handle_get_recent_memories(arguments, db),
        "yolog_get_session_context" => handle_get_session_context(arguments, db),
        "yolog_save_lifeboat" => handle_save_lifeboat(arguments, db),
        _ => ToolCallResult::error(format!("Unknown tool: {}", tool_name)),
    };

    match serde_json::to_value(result) {
        Ok(value) => JsonRpcResponse::success(id, value),
        Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(e.to_string())),
    }
}

/// Handle the resources/list method
pub fn handle_resources_list(id: Value) -> JsonRpcResponse {
    let resources: Vec<ResourceDefinition> = vec![];
    JsonRpcResponse::success(id, json!({ "resources": resources }))
}

/// Handle yolog_search_memories tool call
/// Supports: query-only, type-filter-only, tag-filter-only, and combined modes
fn handle_search_memories(arguments: Value, db: &McpDb) -> ToolCallResult {
    let params: SearchMemoriesParams = match serde_json::from_value(arguments) {
        Ok(p) => p,
        Err(e) => return ToolCallResult::error(format!("Invalid parameters: {}", e)),
    };

    let project_path = params.project_path.as_deref().unwrap_or(".");
    let project = match db.get_project_by_path_prefix(project_path) {
        Ok(Some(p)) => p,
        Ok(None) => {
            return ToolCallResult::text(format!(
                "No Yolog project found for path: {}\n\nTo use memory search, first add this project to Yolog desktop app.",
                project_path
            ));
        }
        Err(e) => return ToolCallResult::error(format!("Database error: {}", e)),
    };

    let memory_types: Option<Vec<MemoryType>> = params.memory_types.map(|types| {
        types
            .iter()
            .filter_map(|t| MemoryType::from_str(t))
            .collect()
    });

    let tag_filters = params.tags.as_deref();

    // Fetch more results when tag filtering is needed (filter happens post-query)
    let fetch_limit = if tag_filters.is_some() {
        params.limit * 5
    } else {
        params.limit
    };

    let query_str = params.query.as_deref().unwrap_or("").trim();

    let results = if query_str.is_empty() {
        // Browse mode: no search query, just filter by type/tags
        match db.browse_memories(&project.id, memory_types.as_deref(), fetch_limit) {
            Ok(r) => r,
            Err(e) => return ToolCallResult::error(format!("Browse failed: {}", e)),
        }
    } else {
        // Search mode: hybrid search with optional type filter
        match db.search_memories_hybrid(
            query_str,
            &project.id,
            memory_types.as_deref(),
            fetch_limit,
        ) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("Hybrid search failed, falling back to FTS5: {}", e);
                match db.search_memories_fts(
                    query_str,
                    &project.id,
                    memory_types.as_deref(),
                    fetch_limit,
                ) {
                    Ok(r) => r,
                    Err(e) => return ToolCallResult::error(format!("Search failed: {}", e)),
                }
            }
        }
    };

    // Apply tag filtering post-query (tags are stored as JSON arrays)
    let filtered: Vec<_> = if let Some(tags) = tag_filters {
        results
            .into_iter()
            .filter(|m| tags.iter().all(|tag| m.tags.iter().any(|t| t == tag)))
            .take(params.limit)
            .collect()
    } else {
        results.into_iter().take(params.limit).collect()
    };

    // Track access for returned memories (feeds into ranking)
    let memory_ids: Vec<i64> = filtered.iter().map(|m| m.id).collect();
    if !memory_ids.is_empty() {
        let _ = db.track_memory_access(&memory_ids);
    }

    if filtered.is_empty() {
        let mut msg = String::from("No memories found");
        if !query_str.is_empty() {
            msg.push_str(&format!(" for query '{}'", query_str));
        }
        if let Some(types) = &memory_types {
            let type_names: Vec<&str> = types.iter().map(|t| t.display_name()).collect();
            msg.push_str(&format!(" with types [{}]", type_names.join(", ")));
        }
        if let Some(tags) = tag_filters {
            msg.push_str(&format!(" with tags [{}]", tags.join(", ")));
        }
        msg.push_str(&format!(" in project '{}'.", project.name));
        return ToolCallResult::text(msg);
    }

    // Build output header
    let mut header_parts = Vec::new();
    if !query_str.is_empty() {
        header_parts.push(format!("for query '{}'", query_str));
    }
    if let Some(types) = &memory_types {
        let type_names: Vec<&str> = types.iter().map(|t| t.display_name()).collect();
        header_parts.push(format!("types [{}]", type_names.join(", ")));
    }
    if let Some(tags) = tag_filters {
        header_parts.push(format!("tags [{}]", tags.join(", ")));
    }

    let mut output = format!(
        "Found {} memories in project '{}'",
        filtered.len(),
        project.name
    );
    if !header_parts.is_empty() {
        output.push_str(&format!(" {}", header_parts.join(", ")));
    }
    output.push_str(":\n\n");

    for (i, m) in filtered.iter().enumerate() {
        output.push_str(&format!(
            "{}. [{}] {} (confidence: {:.0}%)\n",
            i + 1,
            m.memory_type.display_name(),
            m.title,
            m.confidence * 100.0
        ));
        output.push_str(&format!("   {}\n", m.content));
        if let Some(ctx) = &m.context {
            output.push_str(&format!("   Context: {}\n", ctx));
        }
        if !m.tags.is_empty() {
            output.push_str(&format!("   Tags: {}\n", m.tags.join(", ")));
        }
        output.push('\n');
    }

    ToolCallResult::text(output)
}

/// Handle yolog_get_project_context tool call
fn handle_get_project_context(arguments: Value, db: &McpDb) -> ToolCallResult {
    let params: GetProjectContextParams = match serde_json::from_value(arguments) {
        Ok(p) => p,
        Err(e) => return ToolCallResult::error(format!("Invalid parameters: {}", e)),
    };

    let project = match db.get_project_by_path_prefix(&params.project_path) {
        Ok(Some(p)) => p,
        Ok(None) => {
            return ToolCallResult::text(format!(
                "No Yolog project found for path: {}",
                params.project_path
            ));
        }
        Err(e) => return ToolCallResult::error(format!("Database error: {}", e)),
    };

    // Get memories by type
    let decisions = db
        .get_memories_by_type(&project.id, MemoryType::Decision, 5)
        .unwrap_or_default();
    let facts = db
        .get_memories_by_type(&project.id, MemoryType::Fact, 5)
        .unwrap_or_default();
    let preferences = db
        .get_memories_by_type(&project.id, MemoryType::Preference, 5)
        .unwrap_or_default();
    let context_memories = db
        .get_memories_by_type(&project.id, MemoryType::Context, 5)
        .unwrap_or_default();
    let tasks = db
        .get_memories_by_type(&project.id, MemoryType::Task, 5)
        .unwrap_or_default();

    let total =
        decisions.len() + facts.len() + preferences.len() + context_memories.len() + tasks.len();

    let context = ProjectContext {
        project_name: project.name.clone(),
        project_path: project.folder_path.clone(),
        decisions,
        facts,
        preferences,
        context: context_memories,
        tasks,
        total_memories: total,
    };

    // Track access for all returned memories (feeds into ranking)
    let all_ids: Vec<i64> = [
        &context.decisions, &context.facts, &context.preferences,
        &context.context, &context.tasks,
    ].iter().flat_map(|v| v.iter().map(|m| m.id)).collect();
    if !all_ids.is_empty() {
        let _ = db.track_memory_access(&all_ids);
    }

    if context.total_memories == 0 {
        return ToolCallResult::text(format!("No memories found for project '{}'.", project.name));
    }

    let mut output = format!(
        "Project Context for '{}' ({} memories):\n\n",
        context.project_name, context.total_memories
    );

    if !context.decisions.is_empty() {
        output.push_str("## Key Decisions\n");
        for m in &context.decisions {
            output.push_str(&format!("- **{}**: {}\n", m.title, m.content));
        }
        output.push('\n');
    }

    if !context.facts.is_empty() {
        output.push_str("## Facts & Discoveries\n");
        for m in &context.facts {
            output.push_str(&format!("- **{}**: {}\n", m.title, m.content));
        }
        output.push('\n');
    }

    if !context.preferences.is_empty() {
        output.push_str("## Preferences\n");
        for m in &context.preferences {
            output.push_str(&format!("- **{}**: {}\n", m.title, m.content));
        }
        output.push('\n');
    }

    if !context.context.is_empty() {
        output.push_str("## Context\n");
        for m in &context.context {
            output.push_str(&format!("- **{}**: {}\n", m.title, m.content));
        }
        output.push('\n');
    }

    if !context.tasks.is_empty() {
        output.push_str("## Tasks\n");
        for m in &context.tasks {
            output.push_str(&format!("- **{}**: {}\n", m.title, m.content));
        }
        output.push('\n');
    }

    ToolCallResult::text(output)
}

/// Handle yolog_get_recent_memories tool call
fn handle_get_recent_memories(arguments: Value, db: &McpDb) -> ToolCallResult {
    let params: GetRecentMemoriesParams = match serde_json::from_value(arguments) {
        Ok(p) => p,
        Err(e) => return ToolCallResult::error(format!("Invalid parameters: {}", e)),
    };

    let project = match db.get_project_by_path_prefix(&params.project_path) {
        Ok(Some(p)) => p,
        Ok(None) => {
            return ToolCallResult::text(format!(
                "No Yolog project found for path: {}",
                params.project_path
            ));
        }
        Err(e) => return ToolCallResult::error(format!("Database error: {}", e)),
    };

    let session_ids = match db.get_recent_sessions(&project.id, params.sessions) {
        Ok(ids) => ids,
        Err(e) => return ToolCallResult::error(format!("Failed to get sessions: {}", e)),
    };

    if session_ids.is_empty() {
        return ToolCallResult::text(format!("No sessions found for project '{}'.", project.name));
    }

    let memories = match db.get_memories_by_sessions(&session_ids, params.limit) {
        Ok(m) => m,
        Err(e) => return ToolCallResult::error(format!("Query failed: {}", e)),
    };

    if memories.is_empty() {
        return ToolCallResult::text(format!(
            "No memories extracted from recent {} sessions in project '{}'.",
            params.sessions, project.name
        ));
    }

    let mut output = format!(
        "Found {} memories from last {} sessions in project '{}':\n\n",
        memories.len(),
        params.sessions,
        project.name
    );

    for (i, m) in memories.iter().enumerate() {
        output.push_str(&format!(
            "{}. [{}] {} (confidence: {:.0}%)\n",
            i + 1,
            m.memory_type.display_name(),
            m.title,
            m.confidence * 100.0
        ));
        output.push_str(&format!("   {}\n", m.content));
        output.push('\n');
    }

    ToolCallResult::text(output)
}

/// Handle yolog_get_session_context tool call
fn handle_get_session_context(arguments: Value, db: &McpDb) -> ToolCallResult {
    let params: GetSessionContextParams = match serde_json::from_value(arguments) {
        Ok(p) => p,
        Err(e) => return ToolCallResult::error(format!("Invalid parameters: {}", e)),
    };

    let project_path = params.project_path.as_deref().unwrap_or(".");
    let project = match db.get_project_by_path_prefix(project_path) {
        Ok(Some(p)) => p,
        Ok(None) => {
            return ToolCallResult::text(format!(
                "No Yolog project found for path: {}",
                project_path
            ));
        }
        Err(e) => return ToolCallResult::error(format!("Database error: {}", e)),
    };

    let session_context =
        match db.get_or_create_session_context(&params.session_id, &project.id, "startup") {
            Ok(ctx) => ctx,
            Err(e) => {
                return ToolCallResult::error(format!("Failed to get session context: {}", e))
            }
        };

    // Get memories from this session
    let session_memories = db
        .get_memories_by_sessions(&[params.session_id.clone()], 20)
        .unwrap_or_default();

    // Get recent sessions (excluding current)
    let recent_session_ids = db
        .get_recent_sessions_with_context(&project.id, &params.session_id, 3)
        .unwrap_or_default();

    let recent_memories = if !recent_session_ids.is_empty() {
        db.get_memories_by_sessions(&recent_session_ids, 15)
            .unwrap_or_default()
    } else {
        vec![]
    };

    // Get persistent memories
    let persistent_memories = db
        .get_persistent_memories(&project.id, 20)
        .unwrap_or_default();

    // Track access
    let all_memory_ids: Vec<i64> = session_memories
        .iter()
        .chain(recent_memories.iter())
        .chain(persistent_memories.iter())
        .map(|m| m.id)
        .collect();
    let _ = db.track_memory_access(&all_memory_ids);

    let result = SessionContextResult {
        session: session_context,
        session_memories,
        recent_memories,
        persistent_memories,
    };

    let mut output = format!("## Session Context for {}\n\n", project.name);

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
            output.push_str(&format!(
                "- **[{}] {}**: {}\n",
                m.memory_type.display_name(),
                m.title,
                m.content
            ));
        }
    }

    if !result.session_memories.is_empty() {
        output.push_str("\n### This Session's Memories\n");
        for m in result.session_memories.iter().take(5) {
            output.push_str(&format!(
                "- **[{}] {}**: {}\n",
                m.memory_type.display_name(),
                m.title,
                m.content
            ));
        }
    }

    if !result.recent_memories.is_empty() {
        output.push_str("\n### Recent Memories (Last 3 Sessions)\n");
        for m in result.recent_memories.iter().take(5) {
            output.push_str(&format!(
                "- **[{}] {}**: {}\n",
                m.memory_type.display_name(),
                m.title,
                m.content
            ));
        }
    }

    ToolCallResult::text(output)
}

/// Handle yolog_save_lifeboat tool call
fn handle_save_lifeboat(arguments: Value, db: &McpDb) -> ToolCallResult {
    let params: SaveLifeboatParams = match serde_json::from_value(arguments) {
        Ok(p) => p,
        Err(e) => return ToolCallResult::error(format!("Invalid parameters: {}", e)),
    };

    let existing = match db.get_session_context(&params.session_id) {
        Ok(Some(ctx)) => ctx,
        Ok(None) => {
            return ToolCallResult::text(format!(
                "No session context found for session {}. Cannot save lifeboat.",
                params.session_id
            ));
        }
        Err(e) => return ToolCallResult::error(format!("Database error: {}", e)),
    };

    let summary = params.summary.unwrap_or_else(|| {
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

    if let Err(e) = db.save_lifeboat(&params.session_id, &summary) {
        return ToolCallResult::error(format!("Failed to save lifeboat: {}", e));
    }

    ToolCallResult::text(format!(
        "Lifeboat saved successfully for session {}.\n\nResume context: {}",
        params.session_id, summary
    ))
}
