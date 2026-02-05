//! Skill Extraction
//!
//! Extracts workflow patterns (skills) from sessions using AI.
//! Skills are reusable procedures that can be applied in future sessions.

use crate::db::Database;
use std::sync::Arc;

use super::cli::{detect_claude_code, run_cli, DetectedCli};
use super::types::SkillExtractionResult;

/// Maximum characters of input to send to AI
const MAX_INPUT_CHARS: usize = 100_000;

/// Minimum messages required for skill extraction
const MIN_MESSAGES_FOR_EXTRACTION: usize = 25;

/// Timeout for skill extraction
const EXTRACTION_TIMEOUT_SECS: u64 = 120;

/// Raw skill from AI extraction
#[derive(Debug, Clone, serde::Deserialize)]
struct RawSkill {
    name: String,
    description: String,
    steps: Vec<String>,
    #[serde(default = "default_confidence")]
    confidence: f64,
}

fn default_confidence() -> f64 {
    0.9
}

/// Build the prompt for skill discovery
fn build_discovery_prompt(condensed_content: &str) -> String {
    format!(
        r#"Analyze this coding session and identify 1-3 SIGNIFICANT workflow patterns that could become reusable Claude Code skills.

QUALITY REQUIREMENTS:
- Only extract patterns you're highly confident about (>= 0.9)
- Pattern must be clearly repeatable with 3-8 distinct steps
- Must solve a specific, recurring development task
- Skip generic patterns like "debugging" or "testing"

OUTPUT FORMAT (JSON array):
[
  {{
    "name": "reviewing-pull-requests",
    "description": "Reviews pull request changes for code quality, security issues, and adherence to project conventions. Provides actionable feedback with specific file and line references.",
    "steps": [
      "Fetch PR diff and changed files",
      "Analyze code changes for bugs and security issues",
      "Check adherence to project style guide",
      "Provide specific feedback with line references"
    ],
    "confidence": 0.92
  }}
]

NAMING RULES:
- Use gerund form: verb-ing + object (e.g., "reviewing-pull-requests", "deploying-releases")
- Lowercase with hyphens only
- Be specific, not generic

DESCRIPTION RULES:
- Write in THIRD PERSON (e.g., "Deploys..." not "Deploy..." or "Use this to...")
- First sentence: what it does
- Second sentence: context/when to use it
- Max 150 characters

Return [] if no high-confidence patterns found.
Output ONLY valid JSON, no markdown or explanation.

Session:
{}"#,
        condensed_content
    )
}

/// Get session content for skill extraction
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

        // Get messages with condensed format for skill extraction
        let mut stmt = conn
            .prepare(
                "SELECT sequence_num, role, content_preview, tool_name, tool_type
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
                let tool_name: Option<String> = row.get(3)?;
                let tool_type: Option<String> = row.get(4)?;

                // Condensed format for skill extraction
                let role_marker = match role.as_str() {
                    "user" => "U",
                    "assistant" => "A",
                    _ => return Ok(String::new()),
                };

                // Tool usage is important for skill patterns
                if let Some(ref tool) = tool_name {
                    if tool_type.as_deref() == Some("use") {
                        return Ok(format!("[{}] {} → {}", seq, role_marker, tool));
                    } else if tool_type.as_deref() == Some("result") {
                        // Include abbreviated result
                        let result_preview = preview
                            .as_ref()
                            .map(|p| {
                                if p.len() > 100 {
                                    format!("{}...", &p[..100])
                                } else {
                                    p.clone()
                                }
                            })
                            .unwrap_or_default();
                        return Ok(format!("[{}] {} ← {} {}", seq, role_marker, tool, result_preview));
                    }
                }

                // Regular message
                let content = preview
                    .map(|p| {
                        if p.len() > 200 {
                            format!("{}...", &p[..200])
                        } else {
                            p
                        }
                    })
                    .unwrap_or_default();

                Ok(format!("[{}] {}: {}", seq, role_marker, content))
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

        let combined = messages.join("\n");

        // Truncate if too long
        let content = if combined.len() > MAX_INPUT_CHARS {
            format!(
                "{}...\n[Content truncated]",
                &combined[..MAX_INPUT_CHARS]
            )
        } else {
            combined
        };

        Ok((content, project_id))
    })
    .await
}

/// Check if skill with same name already exists
async fn find_duplicate(
    db: &Arc<Database>,
    project_id: &str,
    name: &str,
) -> Result<Option<i64>, String> {
    let project_id = project_id.to_string();
    let name = name.to_string();

    db.with_conn(move |conn| {
        let result: Result<i64, _> = conn.query_row(
            "SELECT id FROM skills WHERE project_id = ? AND name = ?",
            rusqlite::params![project_id, name],
            |row| row.get(0),
        );

        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })
    .await
}

/// Link a session to an existing skill
async fn link_session_to_skill(
    db: &Arc<Database>,
    skill_id: i64,
    session_id: &str,
) -> Result<(), String> {
    let session_id = session_id.to_string();

    db.with_conn(move |conn| {
        // Check if link already exists
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM skill_sessions WHERE skill_id = ? AND session_id = ?",
                rusqlite::params![skill_id, session_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if exists == 0 {
            conn.execute(
                "INSERT INTO skill_sessions (skill_id, session_id) VALUES (?, ?)",
                rusqlite::params![skill_id, session_id],
            )
            .map_err(|e| format!("Failed to link session to skill: {}", e))?;
        }

        Ok(())
    })
    .await
}

/// Store a skill in the database
async fn store_skill(
    db: &Arc<Database>,
    session_id: &str,
    project_id: &str,
    skill: &RawSkill,
) -> Result<i64, String> {
    let session_id = session_id.to_string();
    let project_id = project_id.to_string();
    let name = skill.name.clone();
    let description = skill.description.clone();
    let steps = serde_json::to_string(&skill.steps).unwrap_or_else(|_| "[]".to_string());
    let confidence = skill.confidence;
    let now = chrono::Utc::now().to_rfc3339();

    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO skills (project_id, session_id, name, description, steps, confidence, extracted_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                project_id,
                session_id,
                name,
                description,
                steps,
                confidence,
                now,
            ],
        )
        .map_err(|e| format!("Failed to insert skill: {}", e))?;

        Ok(conn.last_insert_rowid())
    })
    .await
}

/// Extract skills from a session
pub async fn extract_skills(
    db: &Arc<Database>,
    session_id: &str,
    cli: Option<DetectedCli>,
) -> SkillExtractionResult {
    // Detect CLI if not provided
    let cli = match cli {
        Some(c) => c,
        None => detect_claude_code().await,
    };

    if !cli.installed {
        return SkillExtractionResult {
            session_id: session_id.to_string(),
            skills_extracted: 0,
            duplicates_found: 0,
            error: Some("Claude Code CLI not installed".to_string()),
        };
    }

    // Get session content and project_id
    let (session_content, project_id) = match get_session_content(db, session_id).await {
        Ok(c) => c,
        Err(e) => {
            return SkillExtractionResult {
                session_id: session_id.to_string(),
                skills_extracted: 0,
                duplicates_found: 0,
                error: Some(e),
            }
        }
    };

    // Build prompt
    let prompt = build_discovery_prompt(&session_content);

    // Run CLI
    let timeout = std::time::Duration::from_secs(EXTRACTION_TIMEOUT_SECS);
    let output = match run_cli(&cli, &prompt, timeout).await {
        Ok(o) => o,
        Err(e) => {
            return SkillExtractionResult {
                session_id: session_id.to_string(),
                skills_extracted: 0,
                duplicates_found: 0,
                error: Some(e),
            }
        }
    };

    // Parse skills from response
    let skills = match parse_skills(&output) {
        Ok(s) => s,
        Err(e) => {
            return SkillExtractionResult {
                session_id: session_id.to_string(),
                skills_extracted: 0,
                duplicates_found: 0,
                error: Some(format!("Failed to parse skills: {}", e)),
            }
        }
    };

    // Store skills
    let mut extracted = 0;
    let mut duplicates = 0;

    for skill in skills {
        // Check for duplicates
        match find_duplicate(db, &project_id, &skill.name).await {
            Ok(Some(existing_id)) => {
                // Link this session to the existing skill
                if let Err(e) = link_session_to_skill(db, existing_id, session_id).await {
                    tracing::warn!("Failed to link session to skill: {}", e);
                }
                duplicates += 1;
                continue;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("Duplicate check failed: {}", e);
            }
        }

        // Store new skill
        match store_skill(db, session_id, &project_id, &skill).await {
            Ok(skill_id) => {
                // Link session to the new skill
                if let Err(e) = link_session_to_skill(db, skill_id, session_id).await {
                    tracing::warn!("Failed to link session to new skill: {}", e);
                }
                extracted += 1;
            }
            Err(e) => {
                tracing::warn!("Failed to store skill: {}", e);
            }
        }
    }

    SkillExtractionResult {
        session_id: session_id.to_string(),
        skills_extracted: extracted,
        duplicates_found: duplicates,
        error: None,
    }
}

/// Parse skills from AI response
fn parse_skills(response: &str) -> Result<Vec<RawSkill>, String> {
    let trimmed = response.trim();

    // If response doesn't contain a JSON array, return empty
    if !trimmed.contains('[') {
        return Ok(vec![]);
    }

    // Extract JSON array from response
    let start = trimmed.find('[').unwrap();
    let end = match trimmed.rfind(']') {
        Some(e) => e,
        None => return Ok(vec![]),
    };

    let json_str = &trimmed[start..=end];

    // Handle empty array
    if json_str.trim() == "[]" {
        return Ok(vec![]);
    }

    serde_json::from_str(json_str).map_err(|e| format!("Failed to parse skills JSON: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skills_array() {
        let response = r#"[{"name": "reviewing-prs", "description": "Reviews PRs", "steps": ["Fetch diff", "Analyze"], "confidence": 0.95}]"#;
        let skills = parse_skills(response).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "reviewing-prs");
    }

    #[test]
    fn test_parse_skills_empty() {
        let response = "[]";
        let skills = parse_skills(response).unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn test_parse_skills_no_json() {
        let response = "No skills found in this session.";
        let skills = parse_skills(response).unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn test_parse_skills_with_text() {
        let response = r#"Here are the skills:
[{"name": "deploying-apps", "description": "Deploys apps", "steps": ["Build", "Push"], "confidence": 0.9}]
That's all!"#;
        let skills = parse_skills(response).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "deploying-apps");
    }
}
