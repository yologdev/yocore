//! OpenClaw session parser
//!
//! Parses JSONL session files from OpenClaw (AI agent gateway).
//!
//! OpenClaw JSONL format (discovered from live sessions):
//! - `type: "message"` — wraps all conversation messages, role in `message.role`
//! - `type: "session"` — session metadata (id, cwd, version)
//! - `type: "model_change"` — model switch events
//! - `type: "thinking_level_change"` — thinking level events
//! - `type: "custom"` — custom events (model-snapshot, cache-ttl, etc.)
//!
//! Message roles: `"user"`, `"assistant"`, `"toolResult"`
//! Tool calls: content blocks with `type: "toolCall"` (not `"tool_use"`)
//! Tool results: `message.role = "toolResult"` with `toolCallId`, `toolName`, `content`
//! Usage: `message.usage` with fields `input`, `output`, `cacheRead`, `cacheWrite`
//! Parent linking: `id`/`parentId` (not `uuid`/`parentUuid`)

use super::common::{
    calculate_stats, content_to_string, extract_metadata, generate_tool_summary, sanitize_preview,
    truncate_str, ContentDetector, ParsedEventBuilder,
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
        events_by_id: &HashMap<String, Value>,
    ) -> Option<ParsedEvent> {
        let event: Value = serde_json::from_str(line).ok()?;
        let byte_length = line.len() as i64;

        let timestamp = event
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let event_type = event.get("type").and_then(|v| v.as_str())?;

        match event_type {
            "message" => {
                let role = event
                    .get("message")
                    .and_then(|m| m.get("role"))
                    .and_then(|r| r.as_str())?;

                match role {
                    "user" => self.parse_user_message(
                        &event,
                        sequence,
                        byte_offset,
                        byte_length,
                        &timestamp,
                    ),
                    "assistant" => self.parse_assistant_message(
                        &event,
                        sequence,
                        byte_offset,
                        byte_length,
                        &timestamp,
                    ),
                    "toolResult" => self.parse_tool_result(
                        &event,
                        sequence,
                        byte_offset,
                        byte_length,
                        &timestamp,
                        events_by_id,
                    ),
                    _ => None,
                }
            }
            "session" => {
                // Session metadata — extract as system event
                let session_id = event.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let cwd = event.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
                let content = format!("Session started: {} (cwd: {})", session_id, cwd);
                Some(
                    ParsedEventBuilder::new(
                        sequence,
                        "system",
                        &timestamp,
                        byte_offset,
                        byte_length,
                    )
                    .content(sanitize_preview(&content, 200), content)
                    .build(),
                )
            }
            // Skip metadata events that don't contain conversation content
            "model_change" | "thinking_level_change" | "custom" => None,
            _ => None,
        }
    }

    fn parse_user_message(
        &self,
        event: &Value,
        sequence: usize,
        byte_offset: i64,
        byte_length: i64,
        timestamp: &str,
    ) -> Option<ParsedEvent> {
        let content = self.extract_message_text(event);
        let has_code = self.detector.has_code(&content);

        Some(
            ParsedEventBuilder::new(sequence, "user", timestamp, byte_offset, byte_length)
                .content(sanitize_preview(&content, 200), content)
                .flags(has_code, false, false)
                .build(),
        )
    }

    fn parse_assistant_message(
        &self,
        event: &Value,
        sequence: usize,
        byte_offset: i64,
        byte_length: i64,
        timestamp: &str,
    ) -> Option<ParsedEvent> {
        let (input_tokens, output_tokens, cache_read, cache_create) =
            self.extract_openclaw_usage(event);
        let model = self.extract_openclaw_model(event);

        // Check for toolCall content block
        if let Some(tool_call) = self.find_tool_call_block(event) {
            let tool_name = tool_call
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("tool")
                .to_string();

            // OpenClaw uses "arguments" instead of "input"
            let tool_input = tool_call
                .get("arguments")
                .or_else(|| tool_call.get("input"));
            let summary = generate_tool_summary(&tool_name, tool_input);

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

        // Regular assistant text message
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

    fn parse_tool_result(
        &self,
        event: &Value,
        sequence: usize,
        byte_offset: i64,
        byte_length: i64,
        timestamp: &str,
        events_by_id: &HashMap<String, Value>,
    ) -> Option<ParsedEvent> {
        let msg = event.get("message")?;

        // Extract content from tool result
        let content = msg
            .get("content")
            .map(content_to_string)
            .unwrap_or_default();

        // Get tool name directly from the toolResult message
        let tool_name_direct = msg
            .get("toolName")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());

        // Fallback: find parent tool call via parentId
        let parent_id = event.get("parentId").and_then(|v| v.as_str());
        let parent_tool = parent_id
            .and_then(|id| events_by_id.get(id))
            .and_then(|parent| self.find_tool_call_block(parent));

        let tool_name = tool_name_direct
            .or_else(|| {
                parent_tool
                    .as_ref()
                    .and_then(|tc| tc.get("name").and_then(|n| n.as_str()))
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "tool".to_string());

        let tool_input = parent_tool
            .as_ref()
            .and_then(|tc| tc.get("arguments").or_else(|| tc.get("input")));

        let has_code = self.detector.has_code(&content);
        let has_error = self.detector.has_error(&content)
            || msg
                .get("isError")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        let has_file_changes = matches!(
            tool_name.as_str(),
            "Write" | "Edit" | "NotebookEdit" | "write" | "edit"
        );
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

    // ── Content extraction helpers ───────────────────────────────────────────

    /// Extract text content from a user message.
    fn extract_message_text(&self, event: &Value) -> String {
        if let Some(content) = event.get("message").and_then(|m| m.get("content")) {
            return content_to_string(content);
        }
        String::new()
    }

    /// Extract text content from assistant message, skipping toolCall and thinking blocks.
    fn extract_assistant_text(&self, event: &Value) -> String {
        if let Some(content) = event.get("message").and_then(|m| m.get("content")) {
            if let Some(arr) = content.as_array() {
                let mut parts = Vec::new();
                for block in arr {
                    let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                let trimmed = text.trim();
                                if !trimmed.is_empty() {
                                    parts.push(trimmed.to_string());
                                }
                            }
                        }
                        "thinking" => {
                            let thinking = block
                                .get("thinking")
                                .or_else(|| block.get("text"))
                                .and_then(|t| t.as_str())
                                .unwrap_or("");
                            if !thinking.is_empty() {
                                parts.push(format!("Thinking: {}", truncate_str(thinking, 200)));
                            }
                        }
                        _ => {} // Skip toolCall blocks
                    }
                }
                return parts.join("\n\n");
            }
            if let Some(s) = content.as_str() {
                return s.to_string();
            }
        }
        String::new()
    }

    /// Find a toolCall content block in an event's message.content array.
    /// OpenClaw uses `type: "toolCall"` (not `"tool_use"`).
    fn find_tool_call_block(&self, event: &Value) -> Option<Value> {
        event
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|block| {
                        let t = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        t == "toolCall" || t == "tool_use"
                    })
                    .cloned()
            })
    }

    /// Extract token usage from OpenClaw's format.
    /// OpenClaw uses: `message.usage.{input, output, cacheRead, cacheWrite}`
    fn extract_openclaw_usage(
        &self,
        event: &Value,
    ) -> (Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
        let usage = event.get("message").and_then(|m| m.get("usage"));

        let input = usage.and_then(|u| {
            u.get("input")
                .or_else(|| u.get("input_tokens"))
                .and_then(|v| v.as_i64())
        });
        let output = usage.and_then(|u| {
            u.get("output")
                .or_else(|| u.get("output_tokens"))
                .and_then(|v| v.as_i64())
        });
        let cache_read = usage.and_then(|u| {
            u.get("cacheRead")
                .or_else(|| u.get("cache_read_input_tokens"))
                .and_then(|v| v.as_i64())
        });
        let cache_create = usage.and_then(|u| {
            u.get("cacheWrite")
                .or_else(|| u.get("cache_creation_input_tokens"))
                .and_then(|v| v.as_i64())
        });

        (input, output, cache_read, cache_create)
    }

    /// Extract model from OpenClaw's format: `message.model`
    fn extract_openclaw_model(&self, event: &Value) -> Option<String> {
        event
            .get("message")
            .and_then(|m| m.get("model"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
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
        let mut events_by_id: HashMap<String, Value> = HashMap::new();
        let mut byte_offset: i64 = 0;
        let mut errors = Vec::new();

        // First pass: index events by id (for parent-child linking)
        for line in lines {
            if let Ok(event) = serde_json::from_str::<Value>(line) {
                if let Some(id) = event.get("id").and_then(|u| u.as_str()) {
                    events_by_id.insert(id.to_string(), event);
                }
            }
        }

        // Second pass: parse events
        for (sequence, line) in lines.iter().enumerate() {
            match self.parse_line(line, sequence, byte_offset, &events_by_id) {
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

    // ── Real OpenClaw format tests ───────────────────────────────────────────

    #[test]
    fn test_parse_session_event() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"session","version":3,"id":"abc123","timestamp":"2026-02-16T09:00:00Z","cwd":"/data/workspace"}"#.to_string(),
        ];
        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "system");
        assert!(result.events[0].search_content.contains("abc123"));
    }

    #[test]
    fn test_parse_user_message() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"message","id":"msg1","parentId":"p1","timestamp":"2026-02-16T09:00:00Z","message":{"role":"user","content":[{"type":"text","text":"Hello world"}],"timestamp":1771232550544}}"#.to_string(),
        ];
        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "user");
        assert!(result.events[0].search_content.contains("Hello world"));
    }

    #[test]
    fn test_parse_assistant_text_message() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"message","id":"msg2","parentId":"msg1","timestamp":"2026-02-16T09:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"I can help with that"}],"model":"claude-opus-4-6","usage":{"input":100,"output":50,"cacheRead":80,"cacheWrite":10,"totalTokens":240},"stopReason":"stop"}}"#.to_string(),
        ];
        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "assistant");
        assert_eq!(result.events[0].input_tokens, Some(100));
        assert_eq!(result.events[0].output_tokens, Some(50));
        assert_eq!(result.events[0].cache_read_tokens, Some(80));
        assert_eq!(result.events[0].cache_creation_tokens, Some(10));
        assert_eq!(result.events[0].model.as_deref(), Some("claude-opus-4-6"));
    }

    #[test]
    fn test_parse_assistant_tool_call() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"message","id":"msg3","parentId":"msg1","timestamp":"2026-02-16T09:00:02Z","message":{"role":"assistant","content":[{"type":"text","text":"\n\n"},{"type":"toolCall","id":"toolu_01X","name":"web_fetch","arguments":{"url":"https://example.com"}}],"model":"claude-opus-4-6","usage":{"input":50,"output":30},"stopReason":"toolUse"}}"#.to_string(),
        ];
        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "assistant");
        assert_eq!(result.events[0].event_type.as_deref(), Some("tool_use"));
        assert_eq!(result.events[0].tool_name.as_deref(), Some("web_fetch"));
        assert_eq!(result.events[0].tool_type.as_deref(), Some("use"));
    }

    #[test]
    fn test_parse_tool_result() {
        let parser = OpenClawParser::new();
        let lines = vec![
            // Parent: assistant with toolCall
            r#"{"type":"message","id":"msg3","parentId":"msg1","timestamp":"2026-02-16T09:00:02Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"toolu_01X","name":"web_fetch","arguments":{"url":"https://example.com"}}],"model":"claude-opus-4-6","usage":{"input":50,"output":30},"stopReason":"toolUse"}}"#.to_string(),
            // Tool result referencing parent
            r#"{"type":"message","id":"msg4","parentId":"msg3","timestamp":"2026-02-16T09:00:03Z","message":{"role":"toolResult","toolCallId":"toolu_01X","toolName":"web_fetch","content":[{"type":"text","text":"Page content here"}],"isError":false}}"#.to_string(),
        ];
        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 2);

        let tool_result = &result.events[1];
        assert_eq!(tool_result.role, "user");
        assert_eq!(tool_result.event_type.as_deref(), Some("tool_result"));
        assert_eq!(tool_result.tool_name.as_deref(), Some("web_fetch"));
        assert!(tool_result.search_content.contains("Page content"));
        assert!(!tool_result.has_error);
    }

    #[test]
    fn test_parse_tool_result_error() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"message","id":"msg5","parentId":"msg3","timestamp":"2026-02-16T09:00:03Z","message":{"role":"toolResult","toolCallId":"toolu_01X","toolName":"bash","content":[{"type":"text","text":"Error: command not found"}],"isError":true}}"#.to_string(),
        ];
        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert!(result.events[0].has_error);
        assert_eq!(result.events[0].tool_name.as_deref(), Some("bash"));
    }

    #[test]
    fn test_skips_metadata_events() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"model_change","id":"mc1","parentId":null,"timestamp":"2026-02-16T09:00:00Z","provider":"anthropic","modelId":"claude-opus-4-6"}"#.to_string(),
            r#"{"type":"thinking_level_change","id":"tl1","parentId":"mc1","timestamp":"2026-02-16T09:00:00Z","thinkingLevel":"low"}"#.to_string(),
            r#"{"type":"custom","customType":"model-snapshot","data":{},"id":"cs1","parentId":"tl1","timestamp":"2026-02-16T09:00:00Z"}"#.to_string(),
        ];
        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn test_parse_full_session() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"session","version":3,"id":"sess1","timestamp":"2026-02-16T09:00:00Z","cwd":"/data/workspace"}"#.to_string(),
            r#"{"type":"model_change","id":"mc1","parentId":null,"timestamp":"2026-02-16T09:00:00Z","provider":"anthropic","modelId":"claude-opus-4-6"}"#.to_string(),
            r#"{"type":"message","id":"m1","parentId":"mc1","timestamp":"2026-02-16T09:00:01Z","message":{"role":"user","content":[{"type":"text","text":"Fix the login bug"}],"timestamp":1771232550544}}"#.to_string(),
            r#"{"type":"message","id":"m2","parentId":"m1","timestamp":"2026-02-16T09:00:05Z","message":{"role":"assistant","content":[{"type":"text","text":"I'll investigate the login issue."}],"model":"claude-opus-4-6","usage":{"input":100,"output":20},"stopReason":"stop"}}"#.to_string(),
            r#"{"type":"message","id":"m3","parentId":"m2","timestamp":"2026-02-16T09:00:10Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"t1","name":"Read","arguments":{"file_path":"/src/login.rs"}}],"model":"claude-opus-4-6","usage":{"input":120,"output":30},"stopReason":"toolUse"}}"#.to_string(),
            r#"{"type":"message","id":"m4","parentId":"m3","timestamp":"2026-02-16T09:00:11Z","message":{"role":"toolResult","toolCallId":"t1","toolName":"Read","content":[{"type":"text","text":"fn login() { /* code */ }"}],"isError":false}}"#.to_string(),
        ];

        let result = parser.parse(&lines);

        // session (system) + user + assistant text + assistant tool_use + tool_result = 5
        assert_eq!(result.events.len(), 5);

        // Stats (system events don't count as human/assistant)
        assert_eq!(result.stats.human_messages, 1);
        assert_eq!(result.stats.assistant_messages, 1);
        assert_eq!(result.stats.tool_uses, 2); // tool_use + tool_result
        assert_eq!(result.stats.total_input_tokens, 220);
        assert_eq!(result.stats.total_output_tokens, 50);

        // Metadata
        assert_eq!(result.metadata.title.as_deref(), Some("Fix the login bug"));
        assert_eq!(result.metadata.model.as_deref(), Some("claude-opus-4-6"));
    }

    #[test]
    fn test_parse_assistant_with_thinking() {
        let parser = OpenClawParser::new();
        let lines = vec![
            r#"{"type":"message","id":"m1","parentId":"p1","timestamp":"2026-02-16T09:00:00Z","message":{"role":"assistant","content":[{"type":"text","text":"\n\n"},{"type":"thinking","thinking":"Let me check the API.","thinkingSignature":"abc"},{"type":"toolCall","id":"t1","name":"web_fetch","arguments":{"url":"https://api.example.com"}}],"model":"claude-opus-4-6","usage":{"input":50,"output":30},"stopReason":"toolUse"}}"#.to_string(),
        ];
        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].event_type.as_deref(), Some("tool_use"));
        // Thinking content should not appear in preview (it's in search_content)
        assert_eq!(result.events[0].tool_name.as_deref(), Some("web_fetch"));
    }

    #[test]
    fn test_parse_empty() {
        let parser = OpenClawParser::new();
        let result = parser.parse(&[]);
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn test_parse_invalid_json() {
        let parser = OpenClawParser::new();
        let result = parser.parse(&["not valid json".to_string()]);
        assert_eq!(result.events.len(), 0);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_name() {
        let parser = OpenClawParser::new();
        assert_eq!(parser.name(), "openclaw");
    }
}
