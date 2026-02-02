//! Parser types shared across all session parsers

use serde::{Deserialize, Serialize};

/// Result of parsing a session file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseResult {
    /// Parsed events from the session
    pub events: Vec<ParsedEvent>,

    /// Session metadata
    pub metadata: SessionMetadata,

    /// Parsing statistics
    pub stats: ParseStats,

    /// Any parsing errors encountered
    pub errors: Vec<String>,
}

/// A parsed event from a session file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedEvent {
    /// Event sequence number (0-indexed)
    pub sequence: usize,

    /// Event role (human, assistant, system, etc.)
    pub role: String,

    /// Event type for assistant messages (text, tool_use, tool_result)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,

    /// Content preview (truncated for display)
    pub content_preview: String,

    /// Full content for search indexing
    pub search_content: String,

    /// Whether this event contains code
    pub has_code: bool,

    /// Whether this event indicates an error
    pub has_error: bool,

    /// Whether this event has file changes
    pub has_file_changes: bool,

    /// Tool name if this is a tool event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,

    /// Tool type (read, write, bash, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_type: Option<String>,

    /// Tool summary for display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_summary: Option<String>,

    /// Token usage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<i64>,

    /// Model used for this event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Timestamp of the event
    pub timestamp: String,

    /// Byte offset in the original file
    pub byte_offset: i64,

    /// Byte length in the original file
    pub byte_length: i64,
}

/// Session metadata extracted during parsing
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Session ID (usually from file name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Session title (may be auto-generated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// First timestamp in the session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,

    /// Last timestamp in the session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,

    /// Duration in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,

    /// Model used (if consistent)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Statistics from parsing
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParseStats {
    /// Total number of events
    pub total_events: usize,

    /// Number of human messages
    pub human_messages: usize,

    /// Number of assistant messages
    pub assistant_messages: usize,

    /// Number of tool uses
    pub tool_uses: usize,

    /// Whether session has code
    pub has_code: bool,

    /// Whether session has errors
    pub has_errors: bool,

    /// Total input tokens
    pub total_input_tokens: i64,

    /// Total output tokens
    pub total_output_tokens: i64,

    /// Total cache read tokens
    pub total_cache_read_tokens: i64,

    /// Total cache creation tokens
    pub total_cache_creation_tokens: i64,
}
