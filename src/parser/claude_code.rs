//! Claude Code session parser
//!
//! Parses JSONL session files from Claude Code.

use super::types::*;
use super::SessionParser;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

/// Parser for Claude Code session files
pub struct ClaudeCodeParser {
    code_regex: Regex,
    error_regex: Regex,
}

impl ClaudeCodeParser {
    pub fn new() -> Self {
        ClaudeCodeParser {
            code_regex: Regex::new(r"```|`[^`]+`|function |class |const |let |var |import |export ")
                .unwrap(),
            error_regex: Regex::new(r"(?i)error|exception|failed|cannot|undefined|null is not")
                .unwrap(),
        }
    }

    /// Parse a single JSONL line into a ParsedEvent
    fn parse_event(
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

        let event_type = event.get("type").and_then(|v| v.as_str())?;

        match event_type {
            "user" => self.parse_user_event(&event, sequence, byte_offset, byte_length, &timestamp, events_by_uuid),
            "assistant" => self.parse_assistant_event(&event, sequence, byte_offset, byte_length, &timestamp),
            "system" => self.parse_system_event(&event, sequence, byte_offset, byte_length, &timestamp),
            "file-history-snapshot" => Some(ParsedEvent {
                sequence,
                role: "system".to_string(),
                event_type: None,
                content_preview: "File history snapshot".to_string(),
                search_content: "file history snapshot".to_string(),
                has_code: false,
                has_error: false,
                has_file_changes: true,
                tool_name: Some("file-history-snapshot".to_string()),
                tool_type: None,
                tool_summary: None,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                model: None,
                timestamp,
                byte_offset,
                byte_length,
            }),
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
        // Check if this is a meta/system prompt
        if event.get("isMeta").and_then(|v| v.as_bool()).unwrap_or(false) {
            let content = self.extract_user_content(event);
            let preview = self.sanitize_preview(&content, 200);
            return Some(ParsedEvent {
                sequence,
                role: "system".to_string(),
                event_type: None,
                content_preview: preview,
                search_content: content,
                has_code: false,
                has_error: false,
                has_file_changes: false,
                tool_name: Some("skill-prompt".to_string()),
                tool_type: None,
                tool_summary: None,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                model: None,
                timestamp: timestamp.to_string(),
                byte_offset,
                byte_length,
            });
        }

        // Check for task notification
        let raw_content = self.extract_user_content(event);
        if raw_content.contains("<task-notification>") {
            let notification_content = self.extract_task_notification(&raw_content);
            let preview = self.sanitize_preview(&notification_content, 200);
            return Some(ParsedEvent {
                sequence,
                role: "system".to_string(),
                event_type: None,
                content_preview: preview,
                search_content: notification_content,
                has_code: false,
                has_error: false,
                has_file_changes: false,
                tool_name: Some("task-notification".to_string()),
                tool_type: None,
                tool_summary: None,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                model: None,
                timestamp: timestamp.to_string(),
                byte_offset,
                byte_length,
            });
        }

        // Check if this is a tool result
        if let Some(tool_result) = self.extract_tool_result(event) {
            // Find parent tool call
            let parent_uuid = event.get("parentUuid").and_then(|v| v.as_str());
            let parent = parent_uuid.and_then(|uuid| events_by_uuid.get(uuid));
            let tool_call = parent.and_then(|p| self.extract_tool_call(p));

            let content = self.extract_tool_result_content(&tool_result);
            let tool_name = tool_call
                .as_ref()
                .and_then(|tc| tc.get("name").and_then(|n| n.as_str()))
                .map(|s| s.to_string())
                .or_else(|| self.infer_tool_name_from_result(event));

            let has_code = self.detect_code(&content);
            let has_error = self.detect_error(&content);
            let has_file_changes = self.detect_file_changes(&tool_call, &content);

            let tool_summary = self.generate_tool_summary(
                tool_name.as_deref().unwrap_or("Tool"),
                &tool_call,
                Some(&content),
            );

            let preview = self.sanitize_preview(&content, 200);

            return Some(ParsedEvent {
                sequence,
                role: "user".to_string(),
                event_type: Some("tool_result".to_string()),
                content_preview: preview,
                search_content: content,
                has_code,
                has_error,
                has_file_changes,
                tool_name,
                tool_type: Some("result".to_string()),
                tool_summary: Some(tool_summary),
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                model: None,
                timestamp: timestamp.to_string(),
                byte_offset,
                byte_length,
            });
        }

        // Regular user message
        let content = self.extract_user_content(event);
        let preview = self.sanitize_preview(&content, 200);
        let has_code = self.detect_code(&content);

        Some(ParsedEvent {
            sequence,
            role: "user".to_string(),
            event_type: None,
            content_preview: preview,
            search_content: content,
            has_code,
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
            timestamp: timestamp.to_string(),
            byte_offset,
            byte_length,
        })
    }

    fn parse_assistant_event(
        &self,
        event: &Value,
        sequence: usize,
        byte_offset: i64,
        byte_length: i64,
        timestamp: &str,
    ) -> Option<ParsedEvent> {
        // Extract token usage
        let usage = event.get("message").and_then(|m| m.get("usage"));
        let input_tokens = usage.and_then(|u| u.get("input_tokens")).and_then(|v| v.as_i64());
        let output_tokens = usage.and_then(|u| u.get("output_tokens")).and_then(|v| v.as_i64());
        let cache_read_tokens = usage
            .and_then(|u| u.get("cache_read_input_tokens"))
            .and_then(|v| v.as_i64());
        let cache_creation_tokens = usage
            .and_then(|u| u.get("cache_creation_input_tokens"))
            .and_then(|v| v.as_i64());

        // Extract model
        let model = event
            .get("message")
            .and_then(|m| m.get("model"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Check for tool call
        if let Some(tool_call) = self.extract_tool_call(event) {
            let tool_name = tool_call
                .get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());

            let text_content = self.extract_assistant_content(event);
            let tool_summary = self.generate_tool_summary(
                tool_name.as_deref().unwrap_or("Tool"),
                &Some(tool_call.clone()),
                None,
            );

            let preview = if text_content.is_empty() {
                self.generate_tool_preview(
                    tool_name.as_deref().unwrap_or("Tool"),
                    &tool_call,
                    &tool_summary,
                )
            } else {
                self.sanitize_preview(&text_content, 200)
            };

            let search_content = format!(
                "{} {}",
                text_content,
                serde_json::to_string(&tool_call).unwrap_or_default()
            );

            return Some(ParsedEvent {
                sequence,
                role: "assistant".to_string(),
                event_type: Some("tool_use".to_string()),
                content_preview: preview,
                search_content,
                has_code: false,
                has_error: false,
                has_file_changes: false,
                tool_name,
                tool_type: Some("use".to_string()),
                tool_summary: Some(tool_summary),
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                model,
                timestamp: timestamp.to_string(),
                byte_offset,
                byte_length,
            });
        }

        // Regular assistant message
        let content = self.extract_assistant_content(event);
        let preview = self.sanitize_preview(&content, 200);
        let has_code = self.detect_code(&content);

        Some(ParsedEvent {
            sequence,
            role: "assistant".to_string(),
            event_type: None,
            content_preview: preview,
            search_content: content,
            has_code,
            has_error: false,
            has_file_changes: false,
            tool_name: None,
            tool_type: None,
            tool_summary: None,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            model,
            timestamp: timestamp.to_string(),
            byte_offset,
            byte_length,
        })
    }

    fn parse_system_event(
        &self,
        event: &Value,
        sequence: usize,
        byte_offset: i64,
        byte_length: i64,
        timestamp: &str,
    ) -> Option<ParsedEvent> {
        let content = event
            .get("content")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(event).unwrap_or_default());

        let preview = self.sanitize_preview(&content, 200);

        Some(ParsedEvent {
            sequence,
            role: "system".to_string(),
            event_type: None,
            content_preview: preview,
            search_content: content,
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
            timestamp: timestamp.to_string(),
            byte_offset,
            byte_length,
        })
    }

    fn extract_user_content(&self, event: &Value) -> String {
        if let Some(content) = event.get("message").and_then(|m| m.get("content")) {
            if let Some(arr) = content.as_array() {
                return arr
                    .iter()
                    .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            if let Some(s) = content.as_str() {
                return s.to_string();
            }
        }
        event
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    fn extract_assistant_content(&self, event: &Value) -> String {
        if let Some(content) = event.get("message").and_then(|m| m.get("content")) {
            if let Some(arr) = content.as_array() {
                let mut parts = Vec::new();

                // Extract text blocks
                for block in arr {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            parts.push(text.to_string());
                        }
                    }
                    // Extract thinking blocks
                    if block.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                        let thinking = block
                            .get("thinking")
                            .or_else(|| block.get("text"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        if !thinking.is_empty() {
                            parts.push(format!("Thinking...\n\n{}", thinking));
                        }
                    }
                }

                return parts.join("\n\n");
            }
            if let Some(s) = content.as_str() {
                return s.to_string();
            }
        }
        event
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    fn extract_tool_result(&self, event: &Value) -> Option<Value> {
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

    fn extract_tool_result_content(&self, tool_result: &Value) -> String {
        if let Some(content) = tool_result.get("content") {
            if let Some(s) = content.as_str() {
                return s.to_string();
            }
            if let Some(arr) = content.as_array() {
                return arr
                    .iter()
                    .filter(|item| item.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            return serde_json::to_string(content).unwrap_or_default();
        }
        String::new()
    }

    fn extract_tool_call(&self, event: &Value) -> Option<Value> {
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

    fn extract_task_notification(&self, content: &str) -> String {
        let re = Regex::new(r"<task-notification>([\s\S]*?)</task-notification>").unwrap();
        if let Some(caps) = re.captures(content) {
            caps.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_else(|| content.to_string())
        } else {
            content.to_string()
        }
    }

    fn infer_tool_name_from_result(&self, event: &Value) -> Option<String> {
        let tool_result = event.get("toolUseResult")?;

        // Read tool: has file.filePath structure
        if tool_result.get("file").and_then(|f| f.get("filePath")).is_some() {
            return Some("Read".to_string());
        }

        // Bash tool: has exitCode
        if tool_result.get("exitCode").is_some() {
            return Some("Bash".to_string());
        }

        // Write/Edit tool: has success or error flags
        if tool_result.get("success").is_some() || tool_result.get("error").is_some() {
            return Some("Write".to_string());
        }

        None
    }

    fn detect_code(&self, content: &str) -> bool {
        self.code_regex.is_match(content)
    }

    fn detect_error(&self, content: &str) -> bool {
        self.error_regex.is_match(content)
    }

    fn detect_file_changes(&self, tool_call: &Option<Value>, _content: &str) -> bool {
        if let Some(tc) = tool_call {
            let name = tc.get("name").and_then(|n| n.as_str()).unwrap_or("");
            matches!(name, "Write" | "Edit" | "NotebookEdit")
        } else {
            false
        }
    }

    fn generate_tool_summary(&self, tool_name: &str, tool_call: &Option<Value>, tool_output: Option<&str>) -> String {
        let input = tool_call.as_ref().and_then(|tc| tc.get("input"));

        match tool_name {
            "Bash" => {
                if let Some(cmd) = input.and_then(|i| i.get("command")).and_then(|c| c.as_str()) {
                    if cmd.len() > 50 {
                        format!("{}...", &cmd[..50])
                    } else {
                        cmd.to_string()
                    }
                } else {
                    "Bash command".to_string()
                }
            }
            "Write" => {
                if let Some(path) = input.and_then(|i| i.get("file_path")).and_then(|p| p.as_str()) {
                    let file_name = path.split('/').last().unwrap_or(path);
                    if tool_output.is_some() {
                        format!("File created successfully at: {}", file_name)
                    } else {
                        format!("Writing to {}...", file_name)
                    }
                } else {
                    "Writing file".to_string()
                }
            }
            "Edit" => {
                if let Some(path) = input.and_then(|i| i.get("file_path")).and_then(|p| p.as_str()) {
                    let file_name = path.split('/').last().unwrap_or(path);
                    if tool_output.is_some() {
                        format!("Successfully edited {}", file_name)
                    } else {
                        format!("Editing {}...", file_name)
                    }
                } else {
                    "Editing file".to_string()
                }
            }
            "Read" => {
                if let Some(path) = input.and_then(|i| i.get("file_path")).and_then(|p| p.as_str()) {
                    let file_name = path.split('/').last().unwrap_or(path);
                    format!("Read {}", file_name)
                } else {
                    "Reading file".to_string()
                }
            }
            "Grep" => {
                if let Some(pattern) = input.and_then(|i| i.get("pattern")).and_then(|p| p.as_str()) {
                    if pattern.len() > 30 {
                        format!("Search: {}...", &pattern[..30])
                    } else {
                        format!("Search: {}", pattern)
                    }
                } else {
                    "Grep search".to_string()
                }
            }
            "Glob" => {
                if let Some(pattern) = input.and_then(|i| i.get("pattern")).and_then(|p| p.as_str()) {
                    format!("Files: {}", pattern)
                } else {
                    "File glob".to_string()
                }
            }
            "Task" => {
                if let Some(desc) = input.and_then(|i| i.get("description")).and_then(|d| d.as_str()) {
                    if desc.len() > 50 {
                        format!("{}...", &desc[..50])
                    } else {
                        desc.to_string()
                    }
                } else {
                    "Task agent".to_string()
                }
            }
            _ => format!("Used {}", tool_name),
        }
    }

    fn generate_tool_preview(&self, tool_name: &str, tool_call: &Value, tool_summary: &str) -> String {
        let input = tool_call.get("input");

        match tool_name {
            "Bash" => {
                if let Some(cmd) = input.and_then(|i| i.get("command")).and_then(|c| c.as_str()) {
                    format!("$ {}", cmd)
                } else {
                    tool_summary.to_string()
                }
            }
            "Write" => {
                if let Some(path) = input.and_then(|i| i.get("file_path")).and_then(|p| p.as_str()) {
                    let file_name = path.split('/').last().unwrap_or(path);
                    if let Some(content) = input.and_then(|i| i.get("content")).and_then(|c| c.as_str()) {
                        let snippet: String = content.chars().take(150).collect();
                        let snippet = snippet.replace('\n', " ");
                        format!("Write {}: {}...", file_name, snippet)
                    } else {
                        format!("Write {}", file_name)
                    }
                } else {
                    tool_summary.to_string()
                }
            }
            "Read" => {
                if let Some(path) = input.and_then(|i| i.get("file_path")).and_then(|p| p.as_str()) {
                    format!("Read {}", path)
                } else {
                    tool_summary.to_string()
                }
            }
            _ => tool_summary.to_string(),
        }
    }

    fn sanitize_preview(&self, content: &str, max_len: usize) -> String {
        let sanitized = content
            // Remove ANSI escape sequences
            .replace(|c: char| c == '\x1b', "")
            // Remove line number prefixes (e.g., "    3→", "  123→")
            .split('\n')
            .map(|line| {
                let re = Regex::new(r"^\s*\d+→").unwrap();
                re.replace(line, "").to_string()
            })
            .collect::<Vec<_>>()
            .join(" ")
            // Normalize whitespace
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        if sanitized.len() > max_len {
            format!("{}...", &sanitized[..max_len])
        } else {
            sanitized
        }
    }

    fn calculate_stats(&self, events: &[ParsedEvent]) -> ParseStats {
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

    fn extract_metadata(&self, events: &[ParsedEvent]) -> SessionMetadata {
        let mut metadata = SessionMetadata::default();

        // Extract timestamps for duration calculation
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
            metadata.start_time = events
                .iter()
                .filter(|e| e.role != "system")
                .find_map(|e| {
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

        // Extract model from first assistant message with usage
        metadata.model = events
            .iter()
            .find_map(|e| e.model.clone());

        // Generate title from first user message
        metadata.title = events
            .iter()
            .find(|e| e.role == "user" && e.tool_type.is_none())
            .map(|e| {
                let content = &e.search_content;
                if content.len() > 80 {
                    format!("{}...", &content[..80])
                } else {
                    content.clone()
                }
            });

        metadata
    }
}

impl Default for ClaudeCodeParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionParser for ClaudeCodeParser {
    fn parse(&self, lines: &[String]) -> ParseResult {
        let mut events = Vec::new();
        let mut events_by_uuid: HashMap<String, Value> = HashMap::new();
        let mut byte_offset: i64 = 0;
        let mut errors = Vec::new();

        // First pass: collect events by UUID for parent-child linking
        for line in lines {
            if let Ok(event) = serde_json::from_str::<Value>(line) {
                if let Some(uuid) = event.get("uuid").and_then(|u| u.as_str()) {
                    events_by_uuid.insert(uuid.to_string(), event);
                }
            }
        }

        // Second pass: parse events
        for (sequence, line) in lines.iter().enumerate() {
            match self.parse_event(line, sequence, byte_offset, &events_by_uuid) {
                Some(event) => events.push(event),
                None => {
                    // Try to parse just for error logging
                    if serde_json::from_str::<Value>(line).is_err() {
                        errors.push(format!("Failed to parse line {}", sequence));
                    }
                }
            }
            byte_offset += line.len() as i64 + 1; // +1 for newline
        }

        let metadata = self.extract_metadata(&events);
        let stats = self.calculate_stats(&events);

        ParseResult {
            events,
            metadata,
            stats,
            errors,
        }
    }

    fn name(&self) -> &'static str {
        "claude_code"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user_message() {
        let parser = ClaudeCodeParser::new();
        let lines = vec![
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"content":[{"type":"text","text":"Hello world"}]}}"#.to_string(),
        ];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "user");
        assert!(result.events[0].search_content.contains("Hello world"));
    }

    #[test]
    fn test_parse_assistant_message() {
        let parser = ClaudeCodeParser::new();
        let lines = vec![
            r#"{"type":"assistant","timestamp":"2024-01-01T00:00:00Z","message":{"content":[{"type":"text","text":"I can help with that"}],"usage":{"input_tokens":10,"output_tokens":5}}}"#.to_string(),
        ];

        let result = parser.parse(&lines);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].role, "assistant");
        assert_eq!(result.events[0].input_tokens, Some(10));
        assert_eq!(result.events[0].output_tokens, Some(5));
    }

    #[test]
    fn test_detect_code() {
        let parser = ClaudeCodeParser::new();
        assert!(parser.detect_code("```python\nprint('hello')\n```"));
        assert!(parser.detect_code("function foo() {}"));
        assert!(!parser.detect_code("just plain text"));
    }
}
