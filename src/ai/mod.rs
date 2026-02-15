//! AI Features Module
//!
//! Provides AI-powered features for Yolog:
//! - Title generation from session content
//! - Memory extraction (decisions, facts, preferences)
//! - Skills extraction (reusable workflow patterns)
//! - Marker detection (breakthroughs, bugs, decisions, deployments)
//! - Memory ranking and quality scoring
//!
//! AI features work by spawning a configured CLI provider (Claude Code, OpenClaw, etc.)
//! as a subprocess. Provider-specific logic is encapsulated in `cli::CliProvider`.

pub mod auto_trigger;
pub mod cli;
pub mod export;
pub mod marker;
pub mod memory;
pub mod queue;
pub mod ranking;
pub mod similarity;
pub mod skill;
pub mod title;
pub mod types;

// Re-export main types
pub use auto_trigger::AiAutoTrigger;
pub use cli::{detect_provider, CliProvider, DetectedCli};
pub use marker::detect_markers;
pub use memory::extract_memories;
pub use queue::AiTaskQueue;
pub use ranking::{rank_project_memories, RankingConfig, RankingResult};
pub use skill::extract_skills;
pub use types::AiEvent;
