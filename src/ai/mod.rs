//! AI Features Module
//!
//! Provides AI-powered features for Yolog:
//! - Title generation from session content
//! - Memory extraction (decisions, facts, preferences)
//! - Skills extraction (reusable workflow patterns)
//!
//! All AI features work by spawning Claude Code CLI as a subprocess.

pub mod cli;
pub mod memory;
pub mod queue;
pub mod skill;
pub mod title;
pub mod types;

// Re-export main types
pub use cli::{CliProvider, DetectedCli};
pub use memory::extract_memories;
pub use queue::AiTaskQueue;
pub use skill::extract_skills;
pub use types::{AiEvent, AiSettings};
