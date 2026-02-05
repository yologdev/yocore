//! AI Types
//!
//! Shared types for AI features including events and settings.

use serde::{Deserialize, Serialize};

/// AI-related events for SSE broadcast
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AiEvent {
    /// Title generation started
    TitleStart { session_id: String },
    /// Title generation completed successfully
    TitleComplete {
        session_id: String,
        title: String,
    },
    /// Title generation failed
    TitleError {
        session_id: String,
        error: String,
    },
    /// Memory extraction started
    MemoryStart { session_id: String },
    /// Memory extraction completed
    MemoryComplete {
        session_id: String,
        count: usize,
    },
    /// Memory extraction failed
    MemoryError {
        session_id: String,
        error: String,
    },
    /// Skill extraction started
    SkillStart { session_id: String },
    /// Skill extraction completed
    SkillComplete {
        session_id: String,
        count: usize,
    },
    /// Skill extraction failed
    SkillError {
        session_id: String,
        error: String,
    },
    /// Marker detection started
    MarkerStart { session_id: String },
    /// Marker detection completed
    MarkerComplete {
        session_id: String,
        count: usize,
    },
    /// Marker detection failed
    MarkerError {
        session_id: String,
        error: String,
    },
}

impl AiEvent {
    /// Get the SSE event type name
    pub fn event_type(&self) -> &'static str {
        match self {
            AiEvent::TitleStart { .. } => "ai:title:start",
            AiEvent::TitleComplete { .. } => "ai:title:complete",
            AiEvent::TitleError { .. } => "ai:title:error",
            AiEvent::MemoryStart { .. } => "ai:memory:start",
            AiEvent::MemoryComplete { .. } => "ai:memory:complete",
            AiEvent::MemoryError { .. } => "ai:memory:error",
            AiEvent::SkillStart { .. } => "ai:skill:start",
            AiEvent::SkillComplete { .. } => "ai:skill:complete",
            AiEvent::SkillError { .. } => "ai:skill:error",
            AiEvent::MarkerStart { .. } => "ai:markers:start",
            AiEvent::MarkerComplete { .. } => "ai:markers:complete",
            AiEvent::MarkerError { .. } => "ai:markers:error",
        }
    }
}

/// Result of title generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleGenerationResult {
    pub session_id: String,
    pub title: Option<String>,
    pub error: Option<String>,
}

/// Result of memory extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryExtractionResult {
    pub session_id: String,
    pub memories_extracted: usize,
    pub memories_skipped: usize,
    pub error: Option<String>,
}

/// Result of skill extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillExtractionResult {
    pub session_id: String,
    pub skills_extracted: usize,
    pub duplicates_found: usize,
    pub error: Option<String>,
}

/// Request to trigger AI operation
#[derive(Debug, Clone, Deserialize)]
pub struct AiTriggerRequest {
    /// Optional: force re-generation even if already exists
    #[serde(default)]
    pub force: bool,
}
