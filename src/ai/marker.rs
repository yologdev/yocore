//! AI Marker Detection Module
//!
//! Analyzes sessions to find key moments: breakthroughs, deployments, decisions, bugs, etc.
//! Uses two-phase detection:
//! - Phase 1: Find important moment indices (fast, no labels)
//! - Phase 2: Generate labels for detected moments (accurate, small input)

use crate::ai::cli::{
    call_cli_with_prompt, detect_provider, parse_json_response, CliProvider, DetectedCli,
};
use crate::db::Database;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read as IoRead, Seek, SeekFrom};
use std::sync::Arc;
use tokio::sync::Semaphore;

// ============================================================================
// Types
// ============================================================================

/// Supported marker types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkerType {
    Breakthrough,
    Ship,
    Decision,
    Bug,
    Stuck,
}

impl MarkerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MarkerType::Breakthrough => "breakthrough",
            MarkerType::Ship => "ship",
            MarkerType::Decision => "decision",
            MarkerType::Bug => "bug",
            MarkerType::Stuck => "stuck",
        }
    }
}

/// Session marker stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMarker {
    pub id: i64,
    pub session_id: String,
    pub event_index: i32,
    pub marker_type: String,
    pub label: String,
    pub description: Option<String>,
    pub created_at: String,
}

/// Marker data from AI detection (before storage)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkerData {
    pub event_index: i32,
    pub label: String,
    pub description: String,
}

/// Phase 1 result: indices grouped by marker type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase1Result {
    pub markers: MarkerIndicesByType,
}

/// Marker indices organized by type
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarkerIndicesByType {
    #[serde(default)]
    pub breakthrough: Vec<i32>,
    #[serde(default)]
    pub ship: Vec<i32>,
    #[serde(default)]
    pub decision: Vec<i32>,
    #[serde(default)]
    pub bug: Vec<i32>,
    #[serde(default)]
    pub stuck: Vec<i32>,
}

impl MarkerIndicesByType {
    /// Get all indices with their marker types
    pub fn flatten(&self) -> Vec<(MarkerType, i32)> {
        let mut result = Vec::new();
        for &idx in &self.breakthrough {
            result.push((MarkerType::Breakthrough, idx));
        }
        for &idx in &self.ship {
            result.push((MarkerType::Ship, idx));
        }
        for &idx in &self.decision {
            result.push((MarkerType::Decision, idx));
        }
        for &idx in &self.bug {
            result.push((MarkerType::Bug, idx));
        }
        for &idx in &self.stuck {
            result.push((MarkerType::Stuck, idx));
        }
        result
    }

    pub fn total_count(&self) -> usize {
        self.breakthrough.len()
            + self.ship.len()
            + self.decision.len()
            + self.bug.len()
            + self.stuck.len()
    }
}

/// Phase 2 label for a single message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase2Label {
    pub idx: i32,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Phase 2 result: labels for detected messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase2Result {
    pub labels: Vec<Phase2Label>,
}

/// Session message for marker detection
#[derive(Debug, Clone)]
pub struct SessionMessage {
    pub sequence_num: i32,
    pub role: String,
    pub content_preview: Option<String>,
    pub has_error: bool,
    pub tool_type: Option<String>,
    pub byte_offset: i64,
    pub byte_length: i64,
    pub timestamp: String,
}

/// Result of marker detection
#[derive(Debug, Clone, Serialize)]
pub struct MarkerDetectionResult {
    pub session_id: String,
    pub markers_detected: usize,
}

// ============================================================================
// Prompts
// ============================================================================

const PHASE1_DETECTION_PROMPT: &str = r#"Find important moments in this AI coding session.
Return ONLY the idx values grouped by marker type. No labels or descriptions.

Output JSON:
{"markers":{"breakthrough":[idx,...],"ship":[idx,...],"decision":[idx,...],"bug":[idx,...],"stuck":[idx,...]}}

Marker types:
- breakthrough: "it works!", tests passing, feature complete, major blocker resolved
- ship: git commit/push, deployed, PR created/merged
- decision: chose X over Y, architecture choice, "going with"
- bug: "found the bug", "the issue was", root cause identified
- stuck: blocked, confused, "not working", debugging struggles

Rules:
- Use idx values from message data
- Empty array [] if none
- Target ~{} markers total (top 10% most significant)
- Only mark KEY turning points

Output ONLY JSON, no explanation."#;

const PHASE2_LABELING_PROMPT: &str = r#"Label these coding session moments.

For each, generate:
- label: 2-5 words describing THIS message's content
- description: Why significant (max 80 chars)

CRITICAL: Label must describe the MESSAGE content, not session theme.
Example: Message "I'll create a git commit" → label "Creating git commit"

Output JSON:
{"labels":[{"idx":number,"label":"string","description":"string"},...]}"#;

fn build_phase1_prompt(events_json: &str, target_markers: usize) -> String {
    format!(
        "{}\n\nMessages:\n{}",
        PHASE1_DETECTION_PROMPT.replace("{}", &target_markers.to_string()),
        events_json
    )
}

fn build_phase2_prompt(messages_json: &str) -> String {
    format!(
        "{}\n\nMessages to label:\n{}",
        PHASE2_LABELING_PROMPT, messages_json
    )
}

// ============================================================================
// Content Processing
// ============================================================================

/// Read full message content from JSONL file using byte offset
fn read_full_content(
    file_path: &str,
    byte_offset: i64,
    byte_length: i64,
) -> Result<String, String> {
    let mut file = File::open(file_path).map_err(|e| format!("Failed to open file: {}", e))?;

    file.seek(SeekFrom::Start(byte_offset as u64))
        .map_err(|e| format!("Failed to seek: {}", e))?;

    let mut buffer = vec![0u8; byte_length as usize];
    file.read_exact(&mut buffer)
        .map_err(|e| format!("Failed to read: {}", e))?;

    String::from_utf8(buffer).map_err(|e| format!("Failed to decode UTF-8: {}", e))
}

/// Check if message content contains thinking blocks
fn has_thinking_block(full_content: &str) -> bool {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(full_content) {
        if let Some(message) = parsed.get("message") {
            if let Some(content_array) = message.get("content").and_then(|c| c.as_array()) {
                return content_array.iter().any(|block| {
                    block
                        .get("type")
                        .and_then(|t| t.as_str())
                        .map(|s| s == "thinking")
                        .unwrap_or(false)
                });
            }
        }
    }
    false
}

/// Summarize message content to reduce tokens
fn summarize_content(content: &str, role: &str, has_error: bool) -> String {
    let max_len = if has_error {
        800
    } else if role == "user" {
        300
    } else {
        400
    };

    if content.len() <= max_len {
        return content.to_string();
    }

    let half = max_len / 2;
    let start: String = content.chars().take(half).collect();
    let end: String = content
        .chars()
        .rev()
        .take(half)
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    let omitted = content.len() - start.len() - end.len();
    format!("{}...[{} chars omitted]...{}", start, omitted, end)
}

/// Sample events for AI processing (user + assistant only, no tools/thinking)
fn sample_events_for_ai(messages: &[SessionMessage], file_path: &str) -> Vec<SessionMessage> {
    let mut sampled = Vec::new();

    for msg in messages {
        let is_conversation = (msg.role == "user" || msg.role == "assistant")
            && msg.tool_type.is_none()
            && msg.role != "system";

        if is_conversation {
            if msg.role == "assistant" {
                if let Ok(full_content) =
                    read_full_content(file_path, msg.byte_offset, msg.byte_length)
                {
                    if has_thinking_block(&full_content) {
                        continue;
                    }
                }
            }
            sampled.push(msg.clone());
        }
    }

    sampled
}

/// Convert messages to compact JSON for AI
fn events_to_compact_json(events: &[SessionMessage], file_path: &str) -> Result<String, String> {
    let compact: Result<Vec<_>, String> = events
        .iter()
        .map(|msg| {
            let full_content = read_full_content(file_path, msg.byte_offset, msg.byte_length)?;
            let content = summarize_content(&full_content, &msg.role, msg.has_error);

            Ok(serde_json::json!({
                "idx": msg.sequence_num,
                "role": msg.role,
                "time": &msg.timestamp,
                "content": content,
            }))
        })
        .collect();

    serde_json::to_string(&compact?).map_err(|e| format!("Failed to serialize JSON: {}", e))
}

/// Build dynamic chunks based on estimated token count
fn build_dynamic_chunks(sampled: &[SessionMessage], max_tokens: usize) -> Vec<Vec<usize>> {
    const MAX_SUMMARIZED_TOKENS: usize = 300;
    let mut chunks: Vec<Vec<usize>> = Vec::new();
    let mut current_chunk: Vec<usize> = Vec::new();
    let mut current_tokens: usize = 0;

    for (idx, msg) in sampled.iter().enumerate() {
        let content_size = msg.byte_length as usize;
        let estimated_tokens = (content_size + 100) / 3;
        let capped_tokens = estimated_tokens.min(MAX_SUMMARIZED_TOKENS);

        if current_tokens + capped_tokens > max_tokens && !current_chunk.is_empty() {
            chunks.push(current_chunk);
            current_chunk = Vec::new();
            current_tokens = 0;
        }

        current_chunk.push(idx);
        current_tokens += capped_tokens;
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}

// ============================================================================
// Database Operations
// ============================================================================

/// Load session messages from database
fn load_session_messages(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<SessionMessage>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT sequence_num, role, content_preview, has_error, tool_type,
                    byte_offset, byte_length, timestamp
             FROM session_messages
             WHERE session_id = ?1
             ORDER BY sequence_num ASC",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let messages = stmt
        .query_map([session_id], |row| {
            Ok(SessionMessage {
                sequence_num: row.get(0)?,
                role: row.get(1)?,
                content_preview: row.get(2)?,
                has_error: row.get(3)?,
                tool_type: row.get(4)?,
                byte_offset: row.get(5)?,
                byte_length: row.get(6)?,
                timestamp: row.get(7)?,
            })
        })
        .map_err(|e| format!("Failed to query messages: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect messages: {}", e))?;

    Ok(messages)
}

/// Get session file path (prefers archived path)
fn get_session_file_path(conn: &rusqlite::Connection, session_id: &str) -> Result<String, String> {
    let (file_path, archived_path): (String, Option<String>) = conn
        .query_row(
            "SELECT file_path, archived_file_path FROM sessions WHERE id = ?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("Session not found: {}", e))?;

    Ok(archived_path.unwrap_or(file_path))
}

/// Delete existing markers for a session
fn delete_markers(conn: &rusqlite::Connection, session_id: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM session_markers WHERE session_id = ?1",
        [session_id],
    )
    .map_err(|e| format!("Failed to delete old markers: {}", e))?;
    Ok(())
}

/// Store markers in database
fn store_markers(
    conn: &rusqlite::Connection,
    session_id: &str,
    markers: &[(MarkerType, MarkerData)],
) -> Result<usize, String> {
    let now = Utc::now().to_rfc3339();
    let mut saved_count = 0;

    for (marker_type, data) in markers {
        conn.execute(
            "INSERT INTO session_markers (session_id, event_index, marker_type, label, description, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session_id,
                data.event_index,
                marker_type.as_str(),
                &data.label,
                &data.description,
                &now,
            ],
        )
        .map_err(|e| format!("Failed to insert marker: {}", e))?;

        saved_count += 1;
    }

    Ok(saved_count)
}

/// Get markers for a session from database
pub fn get_markers(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<SessionMarker>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, event_index, marker_type, label, description, created_at
             FROM session_markers
             WHERE session_id = ?1
             ORDER BY event_index ASC",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let markers = stmt
        .query_map([session_id], |row| {
            Ok(SessionMarker {
                id: row.get(0)?,
                session_id: row.get(1)?,
                event_index: row.get(2)?,
                marker_type: row.get(3)?,
                label: row.get(4)?,
                description: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(|e| format!("Failed to query markers: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect markers: {}", e))?;

    Ok(markers)
}

/// Delete a single marker by ID
pub fn delete_marker_by_id(conn: &rusqlite::Connection, marker_id: i64) -> Result<(), String> {
    let rows = conn
        .execute("DELETE FROM session_markers WHERE id = ?1", [marker_id])
        .map_err(|e| format!("Failed to delete marker: {}", e))?;

    if rows == 0 {
        return Err("Marker not found".to_string());
    }

    Ok(())
}

// ============================================================================
// AI Detection
// ============================================================================

/// Phase 1: Detect marker indices only
async fn detect_phase1(
    sampled_events: &[SessionMessage],
    file_path: &str,
    target_markers: usize,
    cli: &DetectedCli,
) -> Result<Phase1Result, String> {
    let events_json = events_to_compact_json(sampled_events, file_path)?;
    let prompt = build_phase1_prompt(&events_json, target_markers);

    let response = call_cli_with_prompt(&prompt, cli, 90).await?;
    let result: Phase1Result = parse_json_response(&response)?;

    Ok(result)
}

/// Phase 2: Generate labels for detected messages
async fn detect_phase2(
    messages: &[SessionMessage],
    detected_indices: &[(MarkerType, i32)],
    file_path: &str,
    cli: &DetectedCli,
) -> Result<Phase2Result, String> {
    if detected_indices.is_empty() {
        return Ok(Phase2Result { labels: vec![] });
    }

    let msg_map: HashMap<i32, &SessionMessage> =
        messages.iter().map(|m| (m.sequence_num, m)).collect();

    let mut labeling_messages: Vec<serde_json::Value> = Vec::new();
    for (marker_type, idx) in detected_indices {
        if let Some(msg) = msg_map.get(idx) {
            let full_content = read_full_content(file_path, msg.byte_offset, msg.byte_length)
                .unwrap_or_else(|_| msg.content_preview.clone().unwrap_or_default());

            labeling_messages.push(serde_json::json!({
                "idx": idx,
                "type": marker_type.as_str(),
                "role": msg.role,
                "content": full_content.chars().take(500).collect::<String>(),
            }));
        }
    }

    let messages_json = serde_json::to_string(&labeling_messages)
        .map_err(|e| format!("Failed to serialize: {}", e))?;

    let prompt = build_phase2_prompt(&messages_json);
    let response = call_cli_with_prompt(&prompt, cli, 60).await?;
    let result: Phase2Result = parse_json_response(&response)?;

    Ok(result)
}

/// Combine Phase 1 indices with Phase 2 labels
fn combine_phase_results(
    phase1: &Phase1Result,
    phase2: &Phase2Result,
) -> Vec<(MarkerType, MarkerData)> {
    let label_map: HashMap<i32, &Phase2Label> = phase2.labels.iter().map(|l| (l.idx, l)).collect();

    let mut markers = Vec::new();

    for (marker_type, idx) in phase1.markers.flatten() {
        let (label, description) = if let Some(phase2_label) = label_map.get(&idx) {
            (
                phase2_label.label.clone(),
                phase2_label.description.clone().unwrap_or_default(),
            )
        } else {
            (format!("{:?} at {}", marker_type, idx), String::new())
        };

        markers.push((
            marker_type,
            MarkerData {
                event_index: idx,
                label,
                description,
            },
        ));
    }

    markers
}

// ============================================================================
// Main Entry Point
// ============================================================================

const MIN_MESSAGES_FOR_MARKERS: usize = 30;
const MAX_CHUNK_TOKENS: usize = 70_000;

/// Detect and store markers for a session
pub async fn detect_markers(
    db: &Arc<Database>,
    session_id: &str,
    cli: Option<DetectedCli>,
    provider: CliProvider,
) -> MarkerDetectionResult {
    let cli = match cli {
        Some(c) => c,
        None => {
            let detected = detect_provider(provider).await;
            if !detected.installed {
                return MarkerDetectionResult {
                    session_id: session_id.to_string(),
                    markers_detected: 0,
                };
            }
            detected
        }
    };

    let session_id_for_load = session_id.to_string();
    let result = db
        .with_conn(move |conn| {
            let file_path = get_session_file_path(conn, &session_id_for_load)?;
            let messages = load_session_messages(conn, &session_id_for_load)?;
            Ok::<_, String>((file_path, messages))
        })
        .await;

    let (file_path, messages) = match result {
        Ok((f, m)) => (f, m),
        Err(e) => {
            eprintln!("[markers] Failed to load session: {}", e);
            return MarkerDetectionResult {
                session_id: session_id.to_string(),
                markers_detected: 0,
            };
        }
    };

    if messages.is_empty() {
        return MarkerDetectionResult {
            session_id: session_id.to_string(),
            markers_detected: 0,
        };
    }

    let sampled = sample_events_for_ai(&messages, &file_path);

    if sampled.len() < MIN_MESSAGES_FOR_MARKERS {
        println!(
            "[markers] Session too small ({} messages), skipping",
            sampled.len()
        );
        return MarkerDetectionResult {
            session_id: session_id.to_string(),
            markers_detected: 0,
        };
    }

    // Delete existing markers
    let session_id_for_delete = session_id.to_string();
    if let Err(e) = db
        .with_conn(move |conn| delete_markers(conn, &session_id_for_delete))
        .await
    {
        eprintln!("[markers] Failed to delete old markers: {}", e);
    }

    let chunks = build_dynamic_chunks(&sampled, MAX_CHUNK_TOKENS);
    println!(
        "[markers] Processing {} messages in {} chunk(s)",
        sampled.len(),
        chunks.len()
    );

    let markers = if chunks.len() == 1 {
        // Single chunk: two-phase detection
        process_single_chunk(&sampled, &messages, &file_path, &cli).await
    } else {
        // Multiple chunks: parallel Phase 1, single Phase 2
        process_multiple_chunks(&sampled, &messages, &chunks, &file_path, &cli).await
    };

    let markers = match markers {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[markers] Detection failed: {}", e);
            return MarkerDetectionResult {
                session_id: session_id.to_string(),
                markers_detected: 0,
            };
        }
    };

    // Store markers
    let session_id_for_store = session_id.to_string();
    let markers_to_store = markers;
    let count = db
        .with_conn(move |conn| store_markers(conn, &session_id_for_store, &markers_to_store))
        .await
        .unwrap_or(0);

    println!(
        "[markers] Detected {} markers for session {}",
        count, session_id
    );

    MarkerDetectionResult {
        session_id: session_id.to_string(),
        markers_detected: count,
    }
}

async fn process_single_chunk(
    sampled: &[SessionMessage],
    all_messages: &[SessionMessage],
    file_path: &str,
    cli: &DetectedCli,
) -> Result<Vec<(MarkerType, MarkerData)>, String> {
    let target_markers = (sampled.len() / 40).clamp(5, 20);

    let phase1 = detect_phase1(sampled, file_path, target_markers, cli).await?;

    if phase1.markers.total_count() == 0 {
        return Ok(vec![]);
    }

    let detected_with_types = phase1.markers.flatten();
    let phase2 = detect_phase2(all_messages, &detected_with_types, file_path, cli).await?;

    Ok(combine_phase_results(&phase1, &phase2))
}

async fn process_multiple_chunks(
    sampled: &[SessionMessage],
    all_messages: &[SessionMessage],
    chunks: &[Vec<usize>],
    file_path: &str,
    cli: &DetectedCli,
) -> Result<Vec<(MarkerType, MarkerData)>, String> {
    let semaphore = Arc::new(Semaphore::new(3));

    let phase1_futures: Vec<_> = chunks
        .iter()
        .map(|chunk_indices| {
            let chunk: Vec<_> = chunk_indices
                .iter()
                .map(|&idx| sampled[idx].clone())
                .collect();

            let target_markers = (chunk.len() / 40).clamp(5, 20);
            let sem = semaphore.clone();
            let file_path = file_path.to_string();
            let cli = cli.clone();

            async move {
                let _permit = sem.acquire().await.ok()?;
                detect_phase1(&chunk, &file_path, target_markers, &cli)
                    .await
                    .ok()
            }
        })
        .collect();

    let phase1_results = futures::future::join_all(phase1_futures).await;

    let mut all_detected: Vec<(MarkerType, i32)> = Vec::new();
    for result in phase1_results.into_iter().flatten() {
        all_detected.extend(result.markers.flatten());
    }

    if all_detected.is_empty() {
        return Ok(vec![]);
    }

    let phase2_result = detect_phase2(all_messages, &all_detected, file_path, cli).await;

    match phase2_result {
        Ok(phase2) => {
            let mut aggregated_phase1 = MarkerIndicesByType::default();
            for (marker_type, idx) in &all_detected {
                match marker_type {
                    MarkerType::Breakthrough => aggregated_phase1.breakthrough.push(*idx),
                    MarkerType::Ship => aggregated_phase1.ship.push(*idx),
                    MarkerType::Decision => aggregated_phase1.decision.push(*idx),
                    MarkerType::Bug => aggregated_phase1.bug.push(*idx),
                    MarkerType::Stuck => aggregated_phase1.stuck.push(*idx),
                }
            }

            Ok(combine_phase_results(
                &Phase1Result {
                    markers: aggregated_phase1,
                },
                &phase2,
            ))
        }
        Err(e) => {
            println!("[markers] Phase 2 failed: {}, using fallback labels", e);
            Ok(all_detected
                .iter()
                .map(|(marker_type, idx)| {
                    (
                        *marker_type,
                        MarkerData {
                            event_index: *idx,
                            label: format!("{:?}", marker_type),
                            description: String::new(),
                        },
                    )
                })
                .collect())
        }
    }
}
