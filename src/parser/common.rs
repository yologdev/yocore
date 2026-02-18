//! Shared parser utilities
//!
//! Common building blocks for session parsers: event builder, content detection,
//! statistics calculation, metadata extraction, and text utilities.

use super::types::*;
use regex::Regex;
use serde_json::Value;

// ─── ParsedEventBuilder ─────────────────────────────────────────────────────

/// Builder for constructing `ParsedEvent` without 17-field struct literals.
pub struct ParsedEventBuilder {
    sequence: usize,
    role: String,
    timestamp: String,
    byte_offset: i64,
    byte_length: i64,
    event_type: Option<String>,
    content_preview: String,
    search_content: String,
    has_code: bool,
    has_error: bool,
    has_file_changes: bool,
    tool_name: Option<String>,
    tool_type: Option<String>,
    tool_summary: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_creation_tokens: Option<i64>,
    model: Option<String>,
}

impl ParsedEventBuilder {
    pub fn new(
        sequence: usize,
        role: &str,
        timestamp: &str,
        byte_offset: i64,
        byte_length: i64,
    ) -> Self {
        Self {
            sequence,
            role: role.to_string(),
            timestamp: timestamp.to_string(),
            byte_offset,
            byte_length,
            event_type: None,
            content_preview: String::new(),
            search_content: String::new(),
            has_code: false,
            has_error: false,
            has_file_changes: false,
            tool_name: None,
            tool_type: None,
            tool_summary: None,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            model: None,
        }
    }

    pub fn event_type(mut self, t: &str) -> Self {
        self.event_type = Some(t.to_string());
        self
    }

    pub fn content(mut self, preview: String, search: String) -> Self {
        self.content_preview = preview;
        self.search_content = search;
        self
    }

    pub fn tool(mut self, name: &str, tool_type: &str, summary: &str) -> Self {
        self.tool_name = Some(name.to_string());
        self.tool_type = Some(tool_type.to_string());
        self.tool_summary = Some(summary.to_string());
        self
    }

    pub fn usage(
        mut self,
        input: Option<i64>,
        output: Option<i64>,
        cache_read: Option<i64>,
        cache_create: Option<i64>,
    ) -> Self {
        self.input_tokens = input;
        self.output_tokens = output;
        self.cache_read_tokens = cache_read;
        self.cache_creation_tokens = cache_create;
        self
    }

    pub fn model(mut self, m: &str) -> Self {
        self.model = Some(m.to_string());
        self
    }

    pub fn flags(mut self, code: bool, error: bool, file_changes: bool) -> Self {
        self.has_code = code;
        self.has_error = error;
        self.has_file_changes = file_changes;
        self
    }

    pub fn build(self) -> ParsedEvent {
        ParsedEvent {
            sequence: self.sequence,
            role: self.role,
            event_type: self.event_type,
            content_preview: self.content_preview,
            search_content: self.search_content,
            has_code: self.has_code,
            has_error: self.has_error,
            has_file_changes: self.has_file_changes,
            tool_name: self.tool_name,
            tool_type: self.tool_type,
            tool_summary: self.tool_summary,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_creation_tokens: self.cache_creation_tokens,
            model: self.model,
            timestamp: self.timestamp,
            byte_offset: self.byte_offset,
            byte_length: self.byte_length,
        }
    }
}

// ─── ContentDetector ─────────────────────────────────────────────────────────

/// Regex-based detector for code patterns and error patterns in content.
pub struct ContentDetector {
    code_regex: Regex,
    error_regex: Regex,
}

impl ContentDetector {
    pub fn new() -> Self {
        Self {
            code_regex: Regex::new(
                r"```|`[^`]+`|function |class |const |let |var |import |export ",
            )
            .unwrap(),
            error_regex: Regex::new(r"(?i)error|exception|failed|cannot|undefined|null is not")
                .unwrap(),
        }
    }

    pub fn has_code(&self, content: &str) -> bool {
        self.code_regex.is_match(content)
    }

    pub fn has_error(&self, content: &str) -> bool {
        self.error_regex.is_match(content)
    }
}

impl Default for ContentDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Text utilities ──────────────────────────────────────────────────────────

/// Truncate a string at a valid UTF-8 character boundary.
pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max_len)
        .last()
        .unwrap_or(0);
    format!("{}...", &s[..end])
}

/// Sanitize content for display preview: strip ANSI, line-number prefixes, normalize whitespace.
pub fn sanitize_preview(content: &str, max_len: usize) -> String {
    let line_num_re = Regex::new(r"^\s*\d+→").unwrap();
    let sanitized = content
        .replace('\x1b', "")
        .split('\n')
        .map(|line| line_num_re.replace(line, "").to_string())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    truncate_str(&sanitized, max_len)
}

// ─── Stats & metadata ────────────────────────────────────────────────────────

/// Calculate parsing statistics from a list of events.
pub fn calculate_stats(events: &[ParsedEvent]) -> ParseStats {
    let mut stats = ParseStats::default();

    for event in events {
        stats.total_events += 1;

        match event.role.as_str() {
            "user" => {
                if event.tool_type.is_some() {
                    stats.tool_uses += 1;
                } else {
                    stats.human_messages += 1;
                }
            }
            "assistant" => {
                if event.tool_type.is_some() {
                    stats.tool_uses += 1;
                } else {
                    stats.assistant_messages += 1;
                }
            }
            _ => {}
        }

        if event.has_code {
            stats.has_code = true;
        }
        if event.has_error {
            stats.has_errors = true;
        }

        if let Some(tokens) = event.input_tokens {
            stats.total_input_tokens += tokens;
        }
        if let Some(tokens) = event.output_tokens {
            stats.total_output_tokens += tokens;
        }
        if let Some(tokens) = event.cache_read_tokens {
            stats.total_cache_read_tokens += tokens;
        }
        if let Some(tokens) = event.cache_creation_tokens {
            stats.total_cache_creation_tokens += tokens;
        }
    }

    stats
}

/// Extract session metadata (timestamps, duration, title, model) from events.
pub fn extract_metadata(events: &[ParsedEvent]) -> SessionMetadata {
    let mut metadata = SessionMetadata::default();

    let mut timestamps: Vec<i64> = events
        .iter()
        .filter(|e| e.role != "system")
        .filter_map(|e| {
            chrono::DateTime::parse_from_rfc3339(&e.timestamp)
                .ok()
                .map(|dt| dt.timestamp_millis())
        })
        .collect();

    timestamps.sort();

    if !timestamps.is_empty() {
        metadata.start_time = events.iter().filter(|e| e.role != "system").find_map(|e| {
            chrono::DateTime::parse_from_rfc3339(&e.timestamp)
                .ok()
                .map(|_| e.timestamp.clone())
        });

        metadata.end_time = events
            .iter()
            .filter(|e| e.role != "system")
            .rev()
            .find_map(|e| {
                chrono::DateTime::parse_from_rfc3339(&e.timestamp)
                    .ok()
                    .map(|_| e.timestamp.clone())
            });

        // Calculate active duration (excluding idle periods > 30 min)
        if timestamps.len() > 1 {
            let idle_threshold_ms: i64 = 30 * 60 * 1000;
            let mut active_duration: i64 = 0;

            for i in 1..timestamps.len() {
                let gap = timestamps[i] - timestamps[i - 1];
                if gap <= idle_threshold_ms {
                    active_duration += gap;
                }
            }

            metadata.duration_ms = Some(active_duration);
        }
    }

    // Model from first assistant message with usage
    metadata.model = events.iter().find_map(|e| e.model.clone());

    // Title from first user message
    metadata.title = events
        .iter()
        .find(|e| e.role == "user" && e.tool_type.is_none())
        .map(|e| truncate_str(&e.search_content, 80));

    metadata
}

// ─── Tool summary ────────────────────────────────────────────────────────────

/// Generate a human-readable summary for a tool invocation.
pub fn generate_tool_summary(tool_name: &str, tool_input: Option<&Value>) -> String {
    match tool_name {
        "Bash" | "bash" => {
            if let Some(cmd) = tool_input
                .and_then(|i| i.get("command"))
                .and_then(|c| c.as_str())
            {
                truncate_str(cmd, 50)
            } else {
                "Bash command".to_string()
            }
        }
        "Write" | "write" => {
            if let Some(path) = tool_input
                .and_then(|i| i.get("file_path"))
                .and_then(|p| p.as_str())
            {
                let file_name = path.split('/').next_back().unwrap_or(path);
                format!("Write {}", file_name)
            } else {
                "Writing file".to_string()
            }
        }
        "Edit" | "edit" => {
            if let Some(path) = tool_input
                .and_then(|i| i.get("file_path"))
                .and_then(|p| p.as_str())
            {
                let file_name = path.split('/').next_back().unwrap_or(path);
                format!("Edit {}", file_name)
            } else {
                "Editing file".to_string()
            }
        }
        "Read" | "read" => {
            if let Some(path) = tool_input
                .and_then(|i| i.get("file_path"))
                .and_then(|p| p.as_str())
            {
                let file_name = path.split('/').next_back().unwrap_or(path);
                format!("Read {}", file_name)
            } else {
                "Reading file".to_string()
            }
        }
        "Grep" | "grep" => {
            if let Some(pattern) = tool_input
                .and_then(|i| i.get("pattern"))
                .and_then(|p| p.as_str())
            {
                format!("Search: {}", truncate_str(pattern, 30))
            } else {
                "Grep search".to_string()
            }
        }
        "Glob" | "glob" => {
            if let Some(pattern) = tool_input
                .and_then(|i| i.get("pattern"))
                .and_then(|p| p.as_str())
            {
                format!("Files: {}", pattern)
            } else {
                "File glob".to_string()
            }
        }
        "Task" | "task" => {
            if let Some(desc) = tool_input
                .and_then(|i| i.get("description"))
                .and_then(|d| d.as_str())
            {
                truncate_str(desc, 50)
            } else {
                "Task agent".to_string()
            }
        }
        _ => format!("Used {}", tool_name),
    }
}

// ─── Content extraction helpers ──────────────────────────────────────────────

/// Extract text content from a JSON value that may be a string, an array of content blocks,
/// or nested under a `message` key. Handles common patterns across AI tool formats.
pub fn extract_text_content(value: &Value) -> String {
    // Try direct content field
    if let Some(content) = value.get("content") {
        return content_to_string(content);
    }

    // Try message.content (Claude Code / Anthropic API style)
    if let Some(content) = value.get("message").and_then(|m| m.get("content")) {
        return content_to_string(content);
    }

    String::new()
}

/// Convert a content value (string or content-block array) to a plain text string.
pub fn content_to_string(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        return arr
            .iter()
            .filter_map(|block| {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match block_type {
                    "text" => block
                        .get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string()),
                    "thinking" => block
                        .get("thinking")
                        .or_else(|| block.get("text"))
                        .and_then(|t| t.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| format!("Thinking...\n\n{}", s)),
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    serde_json::to_string(content).unwrap_or_default()
}

/// Extract token usage from an event's message.usage object.
pub fn extract_usage(event: &Value) -> (Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
    let usage = event.get("message").and_then(|m| m.get("usage"));
    let input = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_i64());
    let output = usage
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_i64());
    let cache_read = usage
        .and_then(|u| u.get("cache_read_input_tokens"))
        .and_then(|v| v.as_i64());
    let cache_create = usage
        .and_then(|u| u.get("cache_creation_input_tokens"))
        .and_then(|v| v.as_i64());
    (input, output, cache_read, cache_create)
}

/// Extract the model string from an event.
pub fn extract_model(event: &Value) -> Option<String> {
    event
        .get("message")
        .and_then(|m| m.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Find the first tool_use content block in an event's message.content array.
pub fn find_tool_use_block(event: &Value) -> Option<Value> {
    event
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|block| block.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                .cloned()
        })
}

/// Find the first tool_result content block in an event's message.content array.
pub fn find_tool_result_block(event: &Value) -> Option<Value> {
    event
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|block| block.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
                .cloned()
        })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        let result = truncate_str("hello world", 5);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 9); // 5 chars + "..."
    }

    #[test]
    fn test_truncate_str_multibyte() {
        // Ensure we don't split in the middle of a multi-byte character
        let result = truncate_str("héllo wörld", 5);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_sanitize_preview() {
        let content = "  1→first line\n  2→second line";
        let result = sanitize_preview(content, 100);
        assert!(!result.contains("→"));
        assert!(result.contains("first"));
        assert!(result.contains("second"));
    }

    #[test]
    fn test_sanitize_preview_truncates() {
        let long = "a ".repeat(200);
        let result = sanitize_preview(&long, 50);
        assert!(result.len() <= 54); // 50 + "..."
    }

    #[test]
    fn test_content_detector_code() {
        let detector = ContentDetector::new();
        assert!(detector.has_code("```python\nprint('hello')\n```"));
        assert!(detector.has_code("function foo() {}"));
        assert!(detector.has_code("import os"));
        assert!(!detector.has_code("just plain text here"));
    }

    #[test]
    fn test_content_detector_error() {
        let detector = ContentDetector::new();
        assert!(detector.has_error("Error: file not found"));
        assert!(detector.has_error("exception thrown"));
        assert!(detector.has_error("Build failed"));
        assert!(!detector.has_error("everything is fine"));
    }

    #[test]
    fn test_event_builder_basic() {
        let event = ParsedEventBuilder::new(0, "user", "2024-01-01T00:00:00Z", 0, 50)
            .content("Hello...".to_string(), "Hello world".to_string())
            .build();

        assert_eq!(event.sequence, 0);
        assert_eq!(event.role, "user");
        assert_eq!(event.content_preview, "Hello...");
        assert_eq!(event.search_content, "Hello world");
        assert_eq!(event.timestamp, "2024-01-01T00:00:00Z");
        assert!(!event.has_code);
        assert!(event.tool_name.is_none());
        assert!(event.model.is_none());
    }

    #[test]
    fn test_event_builder_full() {
        let event = ParsedEventBuilder::new(1, "assistant", "2024-01-01T00:01:00Z", 50, 100)
            .event_type("tool_use")
            .content("Read file".to_string(), "Read file content".to_string())
            .tool("Read", "use", "Read main.rs")
            .usage(Some(100), Some(50), Some(80), None)
            .model("claude-opus-4-6")
            .flags(true, false, false)
            .build();

        assert_eq!(event.event_type.as_deref(), Some("tool_use"));
        assert_eq!(event.tool_name.as_deref(), Some("Read"));
        assert_eq!(event.tool_type.as_deref(), Some("use"));
        assert_eq!(event.input_tokens, Some(100));
        assert_eq!(event.model.as_deref(), Some("claude-opus-4-6"));
        assert!(event.has_code);
    }

    #[test]
    fn test_calculate_stats() {
        let events = vec![
            ParsedEventBuilder::new(0, "user", "2024-01-01T00:00:00Z", 0, 10)
                .content("Hi".to_string(), "Hi".to_string())
                .build(),
            ParsedEventBuilder::new(1, "assistant", "2024-01-01T00:00:01Z", 10, 20)
                .content("Hello".to_string(), "Hello".to_string())
                .usage(Some(100), Some(50), None, None)
                .flags(true, false, false)
                .build(),
            ParsedEventBuilder::new(2, "assistant", "2024-01-01T00:00:02Z", 30, 20)
                .event_type("tool_use")
                .tool("Read", "use", "Read file")
                .content("".to_string(), "".to_string())
                .build(),
        ];

        let stats = calculate_stats(&events);
        assert_eq!(stats.total_events, 3);
        assert_eq!(stats.human_messages, 1);
        assert_eq!(stats.assistant_messages, 1);
        assert_eq!(stats.tool_uses, 1);
        assert_eq!(stats.total_input_tokens, 100);
        assert_eq!(stats.total_output_tokens, 50);
        assert!(stats.has_code);
        assert!(!stats.has_errors);
    }

    #[test]
    fn test_extract_metadata() {
        let events = vec![
            ParsedEventBuilder::new(0, "user", "2024-01-01T00:00:00Z", 0, 10)
                .content(
                    "Fix the login bug".to_string(),
                    "Fix the login bug".to_string(),
                )
                .build(),
            ParsedEventBuilder::new(1, "assistant", "2024-01-01T00:05:00Z", 10, 20)
                .content("Done".to_string(), "Done".to_string())
                .model("claude-opus-4-6")
                .build(),
        ];

        let metadata = extract_metadata(&events);
        assert_eq!(metadata.title.as_deref(), Some("Fix the login bug"));
        assert_eq!(metadata.model.as_deref(), Some("claude-opus-4-6"));
        assert!(metadata.start_time.is_some());
        assert!(metadata.end_time.is_some());
        assert!(metadata.duration_ms.is_some());
    }

    #[test]
    fn test_content_to_string_plain() {
        let val: Value = serde_json::json!("hello world");
        assert_eq!(content_to_string(&val), "hello world");
    }

    #[test]
    fn test_content_to_string_blocks() {
        let val: Value = serde_json::json!([
            {"type": "text", "text": "Hello"},
            {"type": "tool_use", "name": "Read"},
            {"type": "text", "text": "World"}
        ]);
        assert_eq!(content_to_string(&val), "Hello\n\nWorld");
    }

    #[test]
    fn test_extract_usage() {
        let event: Value = serde_json::json!({
            "message": {
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 50,
                    "cache_read_input_tokens": 80
                }
            }
        });
        let (input, output, cache_read, cache_create) = extract_usage(&event);
        assert_eq!(input, Some(100));
        assert_eq!(output, Some(50));
        assert_eq!(cache_read, Some(80));
        assert_eq!(cache_create, None);
    }

    #[test]
    fn test_generate_tool_summary() {
        let input: Value = serde_json::json!({"command": "ls -la /tmp"});
        assert_eq!(generate_tool_summary("Bash", Some(&input)), "ls -la /tmp");

        let input: Value = serde_json::json!({"file_path": "/src/main.rs"});
        assert_eq!(generate_tool_summary("Read", Some(&input)), "Read main.rs");

        assert_eq!(generate_tool_summary("Unknown", None), "Used Unknown");
    }
}
