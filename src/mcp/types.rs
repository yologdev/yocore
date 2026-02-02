//! Memory types for MCP server

use serde::{Deserialize, Serialize};

/// Universal memory types that work across any domain
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    /// Choices made with reasoning
    Decision,
    /// Learned information, discoveries, how things work
    Fact,
    /// User preferences, style choices
    Preference,
    /// Background information, domain knowledge
    Context,
    /// Work items, action items, things to do
    Task,
}

impl MemoryType {
    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            MemoryType::Decision => "Decision",
            MemoryType::Fact => "Fact",
            MemoryType::Preference => "Preference",
            MemoryType::Context => "Context",
            MemoryType::Task => "Task",
        }
    }

    /// Parse from string (includes legacy type mapping)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "decision" => Some(MemoryType::Decision),
            "fact" => Some(MemoryType::Fact),
            // Legacy mappings: pattern, architecture, bug -> fact
            "pattern" | "architecture" | "bug" => Some(MemoryType::Fact),
            "preference" => Some(MemoryType::Preference),
            "context" => Some(MemoryType::Context),
            // Legacy mapping: spec -> context
            "spec" => Some(MemoryType::Context),
            "task" => Some(MemoryType::Task),
            _ => None,
        }
    }

    /// Convert to database string
    pub fn to_db_str(&self) -> &'static str {
        match self {
            MemoryType::Decision => "decision",
            MemoryType::Fact => "fact",
            MemoryType::Preference => "preference",
            MemoryType::Context => "context",
            MemoryType::Task => "task",
        }
    }
}

/// A memory extracted from a coding session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub project_id: String,
    pub session_id: String,
    pub memory_type: MemoryType,
    pub title: String,
    pub content: String,
    pub context: Option<String>,
    pub tags: Vec<String>,
    pub confidence: f32,
    pub is_validated: bool,
    pub extracted_at: String,
    pub file_reference: Option<String>,
}

/// Filters for memory queries
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryFilters {
    #[serde(default)]
    pub memory_types: Option<Vec<MemoryType>>,
    #[serde(default)]
    pub min_confidence: Option<f32>,
    #[serde(default)]
    pub validated_only: bool,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

/// Search result with relevance score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub memory: Memory,
    pub score: f32,
    pub match_type: SearchMatchType,
}

/// Type of search match
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMatchType {
    Keyword,
    Semantic,
    Hybrid,
}

/// MCP-specific search request parameters
#[derive(Debug, Clone, Deserialize)]
pub struct SearchMemoriesParams {
    pub query: String,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub memory_types: Option<Vec<String>>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    10
}

/// MCP-specific project context request parameters
#[derive(Debug, Clone, Deserialize)]
pub struct GetProjectContextParams {
    pub project_path: String,
}

/// MCP-specific get memories by type request parameters
#[derive(Debug, Clone, Deserialize)]
pub struct GetMemoriesByTypeParams {
    pub project_path: String,
    pub memory_type: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// MCP-specific get memories by tag request parameters
#[derive(Debug, Clone, Deserialize)]
pub struct GetMemoriesByTagParams {
    pub project_path: String,
    pub tag: String,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// MCP-specific get recent memories request parameters
#[derive(Debug, Clone, Deserialize)]
pub struct GetRecentMemoriesParams {
    pub project_path: String,
    #[serde(default = "default_sessions")]
    pub sessions: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_sessions() -> usize {
    3
}

/// Project context response with categorized memories
#[derive(Debug, Clone, Serialize)]
pub struct ProjectContext {
    pub project_name: String,
    pub project_path: String,
    pub decisions: Vec<Memory>,
    pub facts: Vec<Memory>,
    pub preferences: Vec<Memory>,
    pub context: Vec<Memory>,
    pub tasks: Vec<Memory>,
    pub total_memories: usize,
}

/// Memory state for ranking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryState {
    New,     // Unranked, just extracted
    Low,     // De-prioritized in search results
    High,    // Always included in context
    Removed, // Marked for removal (noise, duplicates, outdated)
}

impl MemoryState {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "new" => Some(MemoryState::New),
            "low" => Some(MemoryState::Low),
            "high" => Some(MemoryState::High),
            "removed" => Some(MemoryState::Removed),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn to_db_str(&self) -> &'static str {
        match self {
            MemoryState::New => "new",
            MemoryState::Low => "low",
            MemoryState::High => "high",
            MemoryState::Removed => "removed",
        }
    }
}

/// Session context for lifeboat pattern - survives context compaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    pub session_id: String,
    pub project_id: String,
    pub active_task: Option<String>,
    pub recent_decisions: Vec<String>,
    pub open_questions: Vec<String>,
    pub resume_context: Option<String>,
    pub source: String, // startup, resume, clear, compact
    pub created_at: String,
    pub updated_at: String,
}

/// Session context with memories - returned by get_session_context
#[derive(Debug, Clone, Serialize)]
pub struct SessionContextResult {
    /// Current session state (lifeboat)
    pub session: SessionContext,
    /// Memories from this session
    pub session_memories: Vec<Memory>,
    /// Recent memories from last N other sessions
    pub recent_memories: Vec<Memory>,
    /// All high-state memories (project-level persistent)
    pub persistent_memories: Vec<Memory>,
}

/// MCP-specific get session context request parameters
#[derive(Debug, Clone, Deserialize)]
pub struct GetSessionContextParams {
    pub session_id: String,
    #[serde(default)]
    pub project_path: Option<String>,
}

/// MCP-specific save lifeboat request parameters (called by PreCompact hook)
#[derive(Debug, Clone, Deserialize)]
pub struct SaveLifeboatParams {
    pub session_id: String,
    #[serde(default)]
    pub summary: Option<String>,
}

/// Project data
#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub folder_path: String,
}
