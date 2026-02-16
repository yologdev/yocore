//! Title Generation
//!
//! Generates concise titles for sessions using AI.

use crate::db::Database;
use std::sync::Arc;

use super::cli::{detect_provider, run_cli, CliProvider, DetectedCli};
use super::types::TitleGenerationResult;

/// Maximum characters for title
const MAX_TITLE_LENGTH: usize = 60;

/// Maximum characters of input to send to AI
const MAX_INPUT_CHARS: usize = 4000;

/// Maximum user messages to include
const MAX_USER_MESSAGES: usize = 10;

/// Build the title generation prompt
fn build_title_prompt(first_messages: &str) -> String {
    format!(
        r#"Generate a concise title (maximum {} characters) for this AI coding session.

**Guidelines:**
- Focus on: main task + tech stack + outcome
- Be specific and descriptive
- Use active voice (e.g., "Fix React hydration in Next.js dashboard")
- Avoid generic titles like "debugging" or "code review"

**Good examples:**
- "Fix React hydration in Next.js dashboard"
- "Add PostgreSQL full-text search to API"
- "Refactor auth middleware for JWT validation"

**Bad examples:**
- "Claude Code session" (too generic)
- "Debugging" (not specific)
- "Working on code" (not descriptive)

Output ONLY the title text, nothing else.

Session conversation:
{}"#,
        MAX_TITLE_LENGTH, first_messages
    )
}

/// Extract first messages from a session for title generation
pub async fn get_first_messages(db: &Arc<Database>, session_id: &str) -> Result<String, String> {
    let session_id = session_id.to_string();

    db.with_conn(move |conn| {
        let mut stmt = conn
            .prepare(
                "SELECT role, content_preview
                 FROM session_messages
                 WHERE session_id = ? AND role = 'user'
                 ORDER BY sequence_num ASC
                 LIMIT ?",
            )
            .map_err(|e| e.to_string())?;

        let messages: Vec<String> = stmt
            .query_map(
                rusqlite::params![session_id, MAX_USER_MESSAGES as i64],
                |row| {
                    let role: String = row.get(0)?;
                    let preview: Option<String> = row.get(1)?;
                    Ok(format!("{}: {}", role, preview.unwrap_or_default()))
                },
            )
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        if messages.is_empty() {
            return Err("No user messages found in session".to_string());
        }

        let combined = messages.join("\n\n");

        // Truncate if too long
        if combined.len() > MAX_INPUT_CHARS {
            Ok(combined[..MAX_INPUT_CHARS].to_string())
        } else {
            Ok(combined)
        }
    })
    .await
}

/// Generate a title for a session
pub async fn generate_title(
    db: &Arc<Database>,
    session_id: &str,
    cli: Option<DetectedCli>,
    provider: CliProvider,
) -> TitleGenerationResult {
    // Detect CLI if not provided
    let cli = match cli {
        Some(c) => c,
        None => detect_provider(provider).await,
    };

    if !cli.installed {
        return TitleGenerationResult {
            session_id: session_id.to_string(),
            title: None,
            error: Some(format!("{} CLI not installed", cli.provider.display_name())),
        };
    }

    // Get first messages
    let first_messages = match get_first_messages(db, session_id).await {
        Ok(m) => m,
        Err(e) => {
            return TitleGenerationResult {
                session_id: session_id.to_string(),
                title: None,
                error: Some(e),
            }
        }
    };

    // Build prompt
    let prompt = build_title_prompt(&first_messages);

    // Run CLI
    let timeout = cli.provider.title_timeout();
    match run_cli(&cli, &prompt, timeout).await {
        Ok(output) => {
            // Clean and truncate title
            let title = clean_title(&output);
            TitleGenerationResult {
                session_id: session_id.to_string(),
                title: Some(title),
                error: None,
            }
        }
        Err(e) => TitleGenerationResult {
            session_id: session_id.to_string(),
            title: None,
            error: Some(e),
        },
    }
}

/// Clean and truncate title output
fn clean_title(raw: &str) -> String {
    // Remove quotes if present
    let title = raw.trim().trim_matches('"').trim_matches('\'');

    // Remove any markdown formatting
    let title = title.trim_start_matches('#').trim();

    // Truncate if too long
    if title.len() > MAX_TITLE_LENGTH {
        let truncated = &title[..MAX_TITLE_LENGTH - 3];
        // Try to truncate at a word boundary
        if let Some(last_space) = truncated.rfind(' ') {
            format!("{}...", &truncated[..last_space])
        } else {
            format!("{}...", truncated)
        }
    } else {
        title.to_string()
    }
}

/// Store generated title in database
pub async fn store_title(db: &Arc<Database>, session_id: &str, title: &str) -> Result<(), String> {
    let session_id = session_id.to_string();
    let title = title.to_string();
    let now = chrono::Utc::now().to_rfc3339();

    db.with_conn(move |conn| {
        conn.execute(
            "UPDATE sessions SET title = ?, title_ai_generated = 1, indexed_at = ? WHERE id = ?",
            rusqlite::params![title, now, session_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
}

/// Generate a title from pre-formatted message text (for ephemeral mode).
/// Does not require a database — takes the message context directly.
pub async fn generate_title_from_text(
    session_id: &str,
    first_messages: &str,
    cli: Option<DetectedCli>,
    provider: CliProvider,
) -> TitleGenerationResult {
    let cli = match cli {
        Some(c) => c,
        None => detect_provider(provider).await,
    };

    if !cli.installed {
        return TitleGenerationResult {
            session_id: session_id.to_string(),
            title: None,
            error: Some(format!("{} CLI not installed", cli.provider.display_name())),
        };
    }

    let prompt = build_title_prompt(first_messages);
    let timeout = cli.provider.title_timeout();
    match run_cli(&cli, &prompt, timeout).await {
        Ok(output) => TitleGenerationResult {
            session_id: session_id.to_string(),
            title: Some(clean_title(&output)),
            error: None,
        },
        Err(e) => TitleGenerationResult {
            session_id: session_id.to_string(),
            title: None,
            error: Some(e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_title() {
        assert_eq!(clean_title("  Fix bug  "), "Fix bug");
        assert_eq!(clean_title("\"Add feature\""), "Add feature");
        assert_eq!(clean_title("# Title"), "Title");

        // Test truncation
        let long_title =
            "This is a very long title that exceeds the maximum allowed length for session titles";
        let cleaned = clean_title(long_title);
        assert!(cleaned.len() <= MAX_TITLE_LENGTH);
        assert!(cleaned.ends_with("..."));
    }
}
