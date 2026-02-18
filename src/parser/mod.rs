//! JSONL session file parsers
//!
//! This module contains parsers for different AI coding assistant session formats.
//! Each parser implements the `SessionParser` trait and is registered in `get_parser()`.
//!
//! ## Adding a new parser
//!
//! 1. Create `src/parser/<tool>.rs` implementing `SessionParser`
//! 2. Use utilities from `common` module (`ParsedEventBuilder`, `ContentDetector`, etc.)
//! 3. Add `pub mod <tool>;` below and register in `get_parser()`
//! 4. Add display name in `watcher/storage.rs` and `watcher/store.rs`

pub mod claude_code;
pub mod common;
pub mod openclaw;
pub mod types;

pub use claude_code::ClaudeCodeParser;
pub use openclaw::OpenClawParser;
pub use types::*;

/// Parser trait for session file formats
pub trait SessionParser: Send + Sync {
    /// Parse a JSONL file and return parsed events
    fn parse(&self, lines: &[String]) -> ParseResult;

    /// Get the parser name
    fn name(&self) -> &'static str;
}

/// Get a parser for the specified AI tool.
///
/// Supported parsers:
/// - `"claude_code"` / `"claude-code"` → Claude Code sessions
/// - `"openclaw"` → OpenClaw sessions
pub fn get_parser(tool: &str) -> Option<Box<dyn SessionParser + Send + Sync>> {
    match tool {
        "claude_code" | "claude-code" => Some(Box::new(ClaudeCodeParser::new())),
        "openclaw" => Some(Box::new(OpenClawParser::new())),
        // Future parsers:
        // "codex" => Some(Box::new(CodexParser::new())),
        // "cursor" => Some(Box::new(CursorParser::new())),
        _ => None,
    }
}
