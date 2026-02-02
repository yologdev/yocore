//! JSONL session file parsers
//!
//! This module contains parsers for different AI coding assistant session formats.

pub mod claude_code;
pub mod types;

pub use claude_code::ClaudeCodeParser;
pub use types::*;

/// Parser trait for session file formats
pub trait SessionParser: Send + Sync {
    /// Parse a JSONL file and return parsed events
    fn parse(&self, lines: &[String]) -> ParseResult;

    /// Get the parser name
    fn name(&self) -> &'static str;
}

/// Get a parser for the specified AI tool
pub fn get_parser(tool: &str) -> Option<Box<dyn SessionParser + Send + Sync>> {
    match tool {
        "claude_code" | "claude-code" => Some(Box::new(ClaudeCodeParser::new())),
        // TODO: Add more parsers
        // "openclaw" => Some(Box::new(OpenClawParser::new())),
        // "cursor" => Some(Box::new(CursorParser::new())),
        _ => None,
    }
}
