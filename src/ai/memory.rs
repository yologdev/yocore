//! Memory Extraction
//!
//! Extracts memories from sessions using AI.
//! Memories are structured knowledge items (decisions, facts, preferences, etc.)

use crate::db::Database;
use std::sync::Arc;

use super::cli::{detect_claude_code, run_cli, DetectedCli};
use super::types::MemoryExtractionResult;

/// Maximum characters of input to send to AI
const MAX_INPUT_CHARS: usize = 150_000;

/// Minimum messages required for memory extraction
const MIN_MESSAGES_FOR_EXTRACTION: usize = 25;

/// Minimum confidence threshold for storing memories
const MIN_CONFIDENCE_THRESHOLD: f64 = 0.70;

/// System prompt for memory extraction
const EXTRACTION_SYSTEM_PROMPT: &str = r#"You are analyzing a session transcript to extract important knowledge that should be remembered for future sessions.

QUALITY REQUIREMENTS (CRITICAL):
- Extract AT MOST 10-15 memories per session chunk
- Only extract memories where you have HIGH CONFIDENCE (>= 0.7)
- Each memory must be genuinely actionable or informative for future work
- Skip anything routine, obvious, or easily discoverable

Extract memories that would be valuable to recall in future sessions. Focus on:

1. **Decisions**: Choices made with reasoning - why this approach was chosen over alternatives
2. **Facts**: Learned information, discoveries, how things work, issues found and fixed
3. **Preferences**: User preferences, style choices, workflow preferences
4. **Context**: Background information, domain knowledge, project situation
5. **Tasks**: Work items, action items, things to do or remember

For each memory, provide:
- type: One of [decision, fact, preference, context, task]
- title: Brief descriptive title (max 80 chars)
- content: The actual knowledge to remember (1-3 sentences, be specific)
- context: Optional context about when/why this applies
- tags: Relevant keywords for search (max 5)
- confidence: How confident are you this is worth remembering? (0.0-1.0)
- file_reference: If applicable, which file(s) this relates to

SKIP THESE (return empty array if only these exist):
- Trivial or routine operations
- Anything that looks like secrets (API keys, passwords, tokens)
- Generic knowledge that's easily discoverable
- Temporary notes or workarounds

QUALITY OVER QUANTITY: It's better to return 3 excellent memories than 20 mediocre ones.
If nothing is genuinely worth remembering, return an empty array.

Respond with ONLY a JSON array of memories, no markdown."#;

/// Raw memory from AI extraction
#[derive(Debug, Clone, serde::Deserialize)]
struct RawMemory {
    #[serde(rename = "type")]
    memory_type: String,
    title: String,
    content: String,
    context: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_confidence")]
    confidence: f64,
    file_reference: Option<String>,
}

fn default_confidence() -> f64 {
    0.7
}

/// Build the memory extraction prompt
fn build_extraction_prompt(session_content: &str) -> String {
    format!(
        "{}\n\n<session_content>\n{}\n</session_content>\n\nRespond with a JSON array of memories:",
        EXTRACTION_SYSTEM_PROMPT, session_content
    )
}

/// Get session messages for extraction
async fn get_session_content(
    db: &Arc<Database>,
    session_id: &str,
) -> Result<(String, String), String> {
    let session_id = session_id.to_string();

    db.with_conn(move |conn| {
        // First get the project_id
        let project_id: String = conn
            .query_row(
                "SELECT project_id FROM sessions WHERE id = ?",
                [&session_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Session not found: {}", e))?;

        // Get messages
        let mut stmt = conn
            .prepare(
                "SELECT sequence_num, role, content_preview, tool_name
                 FROM session_messages
                 WHERE session_id = ?
                 ORDER BY sequence_num ASC",
            )
            .map_err(|e| e.to_string())?;

        let messages: Vec<String> = stmt
            .query_map([&session_id], |row| {
                let seq: i64 = row.get(0)?;
                let role: String = row.get(1)?;
                let preview: Option<String> = row.get(2)?;
                let tool: Option<String> = row.get(3)?;

                let role_display = match role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    _ => return Ok(String::new()),
                };

                let mut msg = format!(
                    "[{}] {}\n{}",
                    seq,
                    role_display,
                    preview.unwrap_or_default()
                );

                if let Some(tool_name) = tool {
                    msg.push_str(&format!("\nTool: {}", tool_name));
                }

                Ok(msg)
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .filter(|s| !s.is_empty())
            .collect();

        if messages.len() < MIN_MESSAGES_FOR_EXTRACTION {
            return Err(format!(
                "Not enough messages for extraction ({} < {})",
                messages.len(),
                MIN_MESSAGES_FOR_EXTRACTION
            ));
        }

        let combined = messages.join("\n\n");

        // Truncate if too long
        let content = if combined.len() > MAX_INPUT_CHARS {
            combined[..MAX_INPUT_CHARS].to_string()
        } else {
            combined
        };

        Ok((content, project_id))
    })
    .await
}

/// Check if a similar memory already exists (exact match or semantic similarity)
async fn find_similar_memory(
    db: &Arc<Database>,
    project_id: &str,
    title: &str,
    content: &str,
) -> Result<bool, String> {
    let project_id = project_id.to_string();
    let title = title.to_string();
    let content = content.to_string();

    db.with_conn(move |conn| {
        // Fast path: exact title match
        let exact_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE project_id = ? AND title = ?",
                rusqlite::params![project_id, title],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        if exact_count > 0 {
            return Ok(true);
        }

        // Similarity check against recent memories
        let mut stmt = conn
            .prepare(
                "SELECT title, content FROM memories
                 WHERE project_id = ? AND state != 'removed'
                 ORDER BY extracted_at DESC LIMIT 200",
            )
            .map_err(|e| e.to_string())?;

        let existing: Vec<(String, String)> = stmt
            .query_map(rusqlite::params![project_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        for (existing_title, existing_content) in &existing {
            if super::similarity::is_similar_memory(
                &title,
                &content,
                existing_title,
                existing_content,
                super::similarity::MEMORY_SIMILARITY_THRESHOLD,
            ) {
                tracing::debug!(
                    "Similar memory found: \"{}\" ≈ \"{}\"",
                    &title[..title.len().min(50)],
                    &existing_title[..existing_title.len().min(50)]
                );
                return Ok(true);
            }
        }

        Ok(false)
    })
    .await
}

/// Store a memory in the database
async fn store_memory(
    db: &Arc<Database>,
    session_id: &str,
    project_id: &str,
    memory: &RawMemory,
) -> Result<i64, String> {
    let session_id = session_id.to_string();
    let project_id = project_id.to_string();
    let memory_type = memory.memory_type.clone();
    let title = memory.title.clone();
    let content = memory.content.clone();
    let context = memory.context.clone();
    let tags = serde_json::to_string(&memory.tags).unwrap_or_else(|_| "[]".to_string());
    let confidence = memory.confidence;
    let file_reference = memory.file_reference.clone();
    let now = chrono::Utc::now().to_rfc3339();

    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO memories (project_id, session_id, memory_type, title, content, context, tags, confidence, is_validated, extracted_at, file_reference, state)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, 'new')",
            rusqlite::params![
                project_id,
                session_id,
                memory_type,
                title,
                content,
                context,
                tags,
                confidence,
                now,
                file_reference,
            ],
        )
        .map_err(|e| format!("Failed to insert memory: {}", e))?;

        Ok(conn.last_insert_rowid())
    })
    .await
}

/// Extract memories from a session
/// If `force` is false and the session has already been extracted, returns early with 0 extracted
pub async fn extract_memories(
    db: &Arc<Database>,
    session_id: &str,
    cli: Option<DetectedCli>,
    force: bool,
) -> MemoryExtractionResult {
    // Check if already extracted and no significant new content (unless force)
    if !force {
        let session_id_check = session_id.to_string();
        let should_skip = db
            .with_conn(move |conn| {
                // Get current message count and last extracted count
                conn.query_row(
                    "SELECT message_count, COALESCE(memories_extracted_count, 0) as extracted_count, memories_extracted_at
                     FROM sessions WHERE id = ?",
                    [&session_id_check],
                    |row| {
                        let current_count: i64 = row.get(0)?;
                        let extracted_count: i64 = row.get(1)?;
                        let extracted_at: Option<String> = row.get(2)?;

                        // Skip if already extracted AND no significant new messages (< 25 new)
                        let new_messages = current_count - extracted_count;
                        let should_skip = extracted_at.is_some() && new_messages < MIN_MESSAGES_FOR_EXTRACTION as i64;
                        Ok(should_skip)
                    },
                )
                .unwrap_or(false)
            })
            .await;

        if should_skip {
            tracing::debug!("Session {} already extracted with no significant new content, skipping", session_id);
            return MemoryExtractionResult {
                session_id: session_id.to_string(),
                memories_extracted: 0,
                memories_skipped: 0,
                error: None,
            };
        }
    }

    // Detect CLI if not provided
    let cli = match cli {
        Some(c) => c,
        None => detect_claude_code().await,
    };

    if !cli.installed {
        return MemoryExtractionResult {
            session_id: session_id.to_string(),
            memories_extracted: 0,
            memories_skipped: 0,
            error: Some("Claude Code CLI not installed".to_string()),
        };
    }

    // Get session content and project_id
    let (session_content, project_id) = match get_session_content(db, session_id).await {
        Ok(c) => c,
        Err(e) => {
            return MemoryExtractionResult {
                session_id: session_id.to_string(),
                memories_extracted: 0,
                memories_skipped: 0,
                error: Some(e),
            }
        }
    };

    // Build prompt
    let prompt = build_extraction_prompt(&session_content);

    // Run CLI (longer timeout for memory extraction)
    let timeout = std::time::Duration::from_secs(120);
    let output = match run_cli(&cli, &prompt, timeout).await {
        Ok(o) => o,
        Err(e) => {
            return MemoryExtractionResult {
                session_id: session_id.to_string(),
                memories_extracted: 0,
                memories_skipped: 0,
                error: Some(e),
            }
        }
    };

    // Parse memories from response
    let memories = match parse_memories(&output) {
        Ok(m) => m,
        Err(e) => {
            return MemoryExtractionResult {
                session_id: session_id.to_string(),
                memories_extracted: 0,
                memories_skipped: 0,
                error: Some(format!("Failed to parse memories: {}", e)),
            }
        }
    };

    // Store memories
    let mut extracted = 0;
    let mut skipped = 0;

    for memory in memories {
        // Skip low confidence
        if memory.confidence < MIN_CONFIDENCE_THRESHOLD {
            skipped += 1;
            continue;
        }

        // Check for duplicates (exact match + semantic similarity)
        match find_similar_memory(db, &project_id, &memory.title, &memory.content).await {
            Ok(true) => {
                skipped += 1;
                continue;
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!("Duplicate check failed: {}", e);
            }
        }

        // Store memory
        match store_memory(db, session_id, &project_id, &memory).await {
            Ok(_) => extracted += 1,
            Err(e) => {
                tracing::warn!("Failed to store memory: {}", e);
                skipped += 1;
            }
        }
    }

    // Update session extraction state - store message_count at extraction time for delta tracking
    let session_id_update = session_id.to_string();
    let _ = db
        .with_conn(move |conn| {
            // Get current message count and store it as the extraction baseline
            let current_message_count: i64 = conn
                .query_row(
                    "SELECT message_count FROM sessions WHERE id = ?",
                    [&session_id_update],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            conn.execute(
                "UPDATE sessions SET memories_extracted_at = datetime('now'), memories_extracted_count = ? WHERE id = ?",
                rusqlite::params![current_message_count, &session_id_update],
            )
        })
        .await;

    MemoryExtractionResult {
        session_id: session_id.to_string(),
        memories_extracted: extracted,
        memories_skipped: skipped,
        error: None,
    }
}

/// Parse memories from AI response
fn parse_memories(response: &str) -> Result<Vec<RawMemory>, String> {
    // Extract JSON from markdown code block if present
    let json_str = if response.contains("```") {
        let lines: Vec<&str> = response.lines().collect();
        let mut in_block = false;
        let mut json_lines = Vec::new();

        for line in lines {
            if line.starts_with("```json") || (line.starts_with("```") && !in_block) {
                in_block = true;
                continue;
            }
            if line.starts_with("```") && in_block {
                break;
            }
            if in_block {
                json_lines.push(line);
            }
        }
        json_lines.join("\n")
    } else {
        response.to_string()
    };

    // Try parsing as array
    if let Ok(memories) = serde_json::from_str::<Vec<RawMemory>>(&json_str) {
        return Ok(memories);
    }

    // Try parsing as object with "memories" field
    #[derive(serde::Deserialize)]
    struct Wrapper {
        memories: Vec<RawMemory>,
    }

    if let Ok(wrapper) = serde_json::from_str::<Wrapper>(&json_str) {
        return Ok(wrapper.memories);
    }

    Err("Failed to parse memories JSON".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_memories_array() {
        let response = r#"[{"type": "decision", "title": "Use React", "content": "Decided to use React"}]"#;
        let memories = parse_memories(response).unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].title, "Use React");
    }

    #[test]
    fn test_parse_memories_object() {
        let response = r#"{"memories": [{"type": "fact", "title": "API endpoint", "content": "Found the API"}]}"#;
        let memories = parse_memories(response).unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].title, "API endpoint");
    }

    #[test]
    fn test_parse_memories_markdown() {
        let response = r#"Here are the memories:
```json
[{"type": "preference", "title": "Tab size", "content": "Use 2 spaces"}]
```"#;
        let memories = parse_memories(response).unwrap();
        assert_eq!(memories.len(), 1);
    }
}
