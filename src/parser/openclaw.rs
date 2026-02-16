//! OpenClaw session parser
//!
//! Parses JSONL session files from OpenClaw (AI agent gateway).
//!
//! OpenClaw's JSONL format is similar to Claude Code but may differ in:
//! - Content representation (string vs content-block arrays)
//! - Tool call/result structure
//! - Presence of uuid/parentUuid for parent-child linking
//! - Metadata fields
//!
//! This parser handles both Claude Code-like and standard Anthropic API formats
//! gracefully, falling back to sensible defaults when fields are missing.

use super::common::{
    calculate_stats, content_to_string, extract_metadata, extract_model, extract_text_content,
    extract_usage, find_tool_result_block, find_tool_use_block, generate_tool_summary,
    sanitize_preview, ContentDetector, ParsedEventBuilder,
};
use super::types::*;
use super::SessionParser;
use serde_json::Value;
use std::collections::HashMap;

/// Parser for OpenClaw session files.
pub struct OpenClawParser {
    detector: ContentDetector,
}

impl OpenClawParser {
    pub fn new() -> Self {
        Self {
            detector: ContentDetector::new(),
        }
    }

    /// Parse a single JSONL line into a ParsedEvent.
    fn parse_line(
        &self,
        line: &str,
        sequence: usize,
        byte_offset: i64,
        events_by_uuid: &HashMap<String, Value>,
    ) -> Option<ParsedEvent> {
        let event: Value = serde_json::from_str(line).ok()?;
        let byte_length = line.len() as i64;

        let timestamp = event
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Determine event type — OpenClaw uses "type" field like Claude Code
        let event_type = event.get("type").and_then(|v| v.as_str())?;

        match event_type {
            "user" => self.parse_user_event(
                &event,
                sequence,
                byte_offset,
                byte_length,
                &timestamp,
                events_by_uuid,
            ),
            "assistant" => {
                self.parse_assistant_event(&event, sequence, byte_offset, byte_length, &timestamp)
            }
            "system" => {
                self.parse_system_event(&event, sequence, byte_offset, byte_length, &timestamp)
            }
            _ => None,
        }
    }

    fn parse_user_event(
        &self,
        event: &Value,
        sequence: usize,
        byte_offset: i64,
        byte_length: i64,
        timestamp: &str,
        events_by_uuid: &HashMap<String, Value>,
    ) -> Option<ParsedEvent> {
        // Check for tool result
        if let Some(tool_result) = find_tool_result_block(event) {
            return self.parse_tool_result(
                event,
                &tool_result,
                sequence,
                byte_offset,
                byte_length,
                timestamp,
                events_by_uuid,
            );
        }

        // Regular user message
        let content = extract_text_content(event);
        let has_code = self.detector.has_code(&content);

        Some(
            ParsedEventBuilder::new(sequence, "user", timestamp, byte_offset, byte_length)
                .content(sanitize_preview(&content, 200), content)
                .flags(has_code, false, false)
                .build(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_tool_result(
        &self,
        event: &Value,
        tool_result: &Value,
        sequence: usize,
        byte_offset: i64,
        byte_length: i64,
        timestamp: &str,
        events_by_uuid: &HashMap<String, Value>,
    ) -> Option<ParsedEvent> {
        let content = tool_result
            .get("content")
            .map(content_to_string)
            .unwrap_or_default();

        // Try to find the parent tool call via parentUuid
        let parent_uuid = event.get("parentUuid").and_then(|v| v.as_str());
        let parent_tool = parent_uuid
            .and_then(|uuid| events_by_uuid.get(uuid))
            .and_then(find_tool_use_block);

        let tool_name = parent_tool
            .as_ref()
            .and_then(|tc| tc.get("name").and_then(|n| n.as_str()))
            .map(|s| s.to_string())
            .unwrap_or_else(|| "tool".to_string());

        let tool_input = parent_tool.as_ref().and_then(|tc| tc.get("input"));

        let has_code = self.detector.has_code(&content);
        let has_error = self.detector.has_error(&content)
            || tool_result
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        let has_file_changes = matches!(tool_name.as_str(), "Write" | "Edit" | "NotebookEdit");
        let summary = generate_tool_summary(&tool_name, tool_input);

        Some(
            ParsedEventBuilder::new(sequence, "user", timestamp, byte_offset, byte_length)
                .event_type("tool_result")
                .content(sanitize_preview(&content, 200), content)
                .tool(&tool_name, "result", &summary)
                .flags(has_code, has_error, has_file_changes)
                .build(),
        )
    }

    fn parse_assistant_event(
        &self,
        event: &Value,
        sequence: usize,
        byte_offset: i64,
        byte_length: i64,
        timestamp: &str,
    ) -> Option<ParsedEvent> {
        let (input_tokens, output_tokens, cache_read, cache_create) = extract_usage(event);
        let model = extract_model(event);

        // Check for tool call
        if let Some(tool_call) = find_tool_use_block(event) {
            let tool_name = tool_call
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("tool")
                .to_string();

            let tool_input = tool_call.get("input");
            let summary = generate_tool_summary(&tool_name, tool_input);

            // Also extract any text content alongside the tool call
            let text_content = self.extract_assistant_text(event);
            let preview = if text_content.is_empty() {
                summary.clone()
            } else {
                sanitize_preview(&text_content, 200)
            };

            let search_content = format!(
                "{} {}",
                text_content,
                serde_json::to_string(&tool_call).unwrap_or_default()
            );

            let mut builder =
                ParsedEventBuilder::new(sequence, "assistant", timestamp, byte_offset, byte_length)
                    .event_type("tool_use")
                    .content(preview, search_content)
                    .tool(&tool_name, "use", &summary)
                    .usage(input_tokens, output_tokens, cache_read, cache_create);

            if let Some(ref m) = model {
                builder = builder.model(m);
            }

            return Some(builder.build());
        }

        // Regular assistant message
        let content = self.extract_assistant_text(event);
        let has_code = self.detector.has_code(&content);

        let mut builder =
            ParsedEventBuilder::new(sequence, "assistant", timestamp, byte_offset, byte_length)
                .content(sanitize_preview(&content, 200), content)
                .usage(input_tokens, output_tokens, cache_read, cache_create)
                .flags(has_code, false, false);

        if let Some(ref m) = model {
            builder = builder.model(m);
        }

        Some(builder.build())
    }

    fn parse_system_event(
        &self,
        event: &Value,
        sequence: usize,
        byte_offset: i64,
        byte_length: i64,
        timestamp: &str,
    ) -> Option<ParsedEvent> {
        let content = extract_text_content(event);
        let fallback = if content.is_empty() {
            serde_json::to_string(event).unwrap_or_default()
        } else {
            content
        };

        Some(
            ParsedEventBuilder::new(sequence, "system", timestamp, byte_offset, byte_length)
                .content(sanitize_preview(&fallback, 200), fallback)
                .build(),
        )
    }

    /// Extract text content from assistant message, handling content blocks.
    fn extract_assistant_text(&self, event: &Value) -> String {
        if let Some(content) = event.get("message").and_then(|m| m.get("content")) {
            if let Some(arr) = content.as_array() {
                let mut parts = Vec::new();
                for block in arr {
                    let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                parts.push(text.to_string());
                            }
                        }
                        "thinking" => {
                            let thinking = block
                                .get("thinking")
                                .or_else(|| block.get("text"))
                                .and_then(|t| t.as_str())
                                .unwrap_or("");
                            if !thinking.is_empty() {
                                parts.push(format!("Thinking...\n\n{}", thinking));
                            }
                        }
                        _ => {} // Skip tool_use blocks — handled separately
                    }
                }
                return parts.join("\n\n");
            }
            if let Some(s) = content.as_str() {
                return s.to_string();
            }
        }
        // Fallback: try direct content field
        event
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }
}

impl Default for OpenClawParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionParser for OpenClawParser {
    fn parse(&self, lines: &[String]) -> ParseResult {
        let mut events = Vec::new();
        let mut events_by_uuid: HashMap<String, Value> = HashMap::new();
        let mut byte_offset: i64 = 0;
        let mut errors = Vec::new();

        // First pass: index events by UUID (for parent-child linking if available)
        for line in lines {
            if let Ok(event) = serde_json::from_str::<Value>(line) {
                if let Some(uuid) = event.get("uuid").and_then(|u| u.as_str()) {
                    events_by_uuid.insert(uuid.to_string(), event);
                }
            }
        }

        // Second pass: parse events
        for (sequence, line) in lines.iter().enumerate() {
            match self.parse_line(line, sequence, byte_offset, &events_by_uuid) {
                Some(event) => events.push(event),
                None => {
                    if serde_json::from_str::<Value>(line).is_err() {
                        errors.push(format!("Failed to parse line {}", sequence));
                    }
                }
            }
            byte_offset += line.len() as i64 + 1; // +1 for newline
        }

        let metadata = extract_metadata(&events);
        let stats = calculate_stats(&events);

        ParseResult {
            events,
            metadata,
            stats,
            errors,
        }
    }

    fn name(&self) -> &'static str {
        "openclaw"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user_message() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"content":[{"type":"text","text":"Hello world"}]}}"#.to_string(),
        ];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "user");
        assert!(result.events[0].search_content.contains("Hello world"));
    }

    #[test]
    fn test_parse_user_message_string_content() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","content":"Hello world"}"#
                .to_string(),
        ];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "user");
        assert!(result.events[0].search_content.contains("Hello world"));
    }

    #[test]
    fn test_parse_assistant_message() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:00Z","message":{"content":[{"type":"text","text":"I can help with that"}],"usage":{"input_tokens":10,"output_tokens":5},"model":"claude-opus-4-6"}}"#.to_string(),
        ];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "assistant");
        assert_eq!(result.events[0].input_tokens, Some(10));
        assert_eq!(result.events[0].output_tokens, Some(5));
        assert_eq!(result.events[0].model.as_deref(), Some("claude-opus-4-6"));
    }

    #[test]
    fn test_parse_assistant_tool_use() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:00Z","message":{"content":[{"type":"tool_use","id":"tool_1","name":"Read","input":{"file_path":"/src/main.rs"}}],"usage":{"input_tokens":20,"output_tokens":10}}}"#.to_string(),
        ];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "assistant");
        assert_eq!(result.events[0].event_type.as_deref(), Some("tool_use"));
        assert_eq!(result.events[0].tool_name.as_deref(), Some("Read"));
        assert_eq!(result.events[0].tool_type.as_deref(), Some("use"));
        assert!(result.events[0]
            .tool_summary
            .as_deref()
            .unwrap()
            .contains("main.rs"));
    }

    #[test]
    fn test_parse_tool_result_with_parent() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"assistant","uuid":"abc123","timestamp":"2024-01-01T00:00:00Z","message":{"content":[{"type":"tool_use","id":"tool_1","name":"Read","input":{"file_path":"/src/main.rs"}}],"usage":{"input_tokens":20,"output_tokens":10}}}"#.to_string(),
            r#"{"type":"user","parentUuid":"abc123","timestamp":"2024-01-01T00:00:01Z","message":{"content":[{"type":"tool_result","tool_use_id":"tool_1","content":"fn main() {}"}]}}"#.to_string(),
        ];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 2);

        // Tool result should link to parent
        let tool_result = &result.events[1];
        assert_eq!(tool_result.role, "user");
        assert_eq!(tool_result.event_type.as_deref(), Some("tool_result"));
        assert_eq!(tool_result.tool_name.as_deref(), Some("Read"));
        assert!(tool_result.search_content.contains("fn main()"));
    }

    #[test]
    fn test_parse_system_event() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"system","timestamp":"2024-01-01T00:00:00Z","content":"System initialized"}"#
                .to_string(),
        ];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "system");
        assert!(result.events[0].search_content.contains("initialized"));
    }

    #[test]
    fn test_parse_mixed_session() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"content":[{"type":"text","text":"Fix the bug"}]}}"#.to_string(),
            r#"{"type":"assistant","timestamp":"2024-01-01T00:01:00Z","message":{"content":[{"type":"text","text":"I'll look into it"}],"usage":{"input_tokens":50,"output_tokens":20},"model":"claude-opus-4-6"}}"#.to_string(),
            r#"{"type":"assistant","timestamp":"2024-01-01T00:02:00Z","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}],"usage":{"input_tokens":60,"output_tokens":30}}}"#.to_string(),
        ];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 3);

        // Stats
        assert_eq!(result.stats.human_messages, 1);
        assert_eq!(result.stats.assistant_messages, 1);
        assert_eq!(result.stats.tool_uses, 1);
        assert_eq!(result.stats.total_input_tokens, 110);
        assert_eq!(result.stats.total_output_tokens, 50);

        // Metadata
        assert_eq!(result.metadata.title.as_deref(), Some("Fix the bug"));
        assert_eq!(result.metadata.model.as_deref(), Some("claude-opus-4-6"));
        assert!(result.metadata.duration_ms.is_some());
    }

    #[test]
    fn test_parse_empty_lines() {
        let parser = OpenClawParser::new();
        let lines: Vec<String> = Vec::new();

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 0);
        assert_eq!(result.stats.total_events, 0);
    }

    #[test]
    fn test_parse_invalid_json() {
        let parser = OpenClawParser::new();
        let lines = vec!["not valid json".to_string()];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 0);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_error_detection_in_tool_result() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"Error: file not found","is_error":true}]}}"#.to_string(),
        ];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert!(result.events[0].has_error);
    }

    #[test]
    fn test_name() {
        let parser = OpenClawParser::new();
        assert_eq!(parser.name(), "openclaw");
    }
}
