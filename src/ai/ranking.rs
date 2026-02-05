//! Memory ranking module
//!
//! Automatically ranks memories based on quality and usage patterns.
//! Promotes valuable memories to `high` state and demotes/removes low-value ones.

use crate::db::Database;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// Configuration for memory ranking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingConfig {
    /// Score threshold for promoting new memories to high state
    pub high_threshold: f64,
    /// Minimum access count for promotion to high state
    pub min_access_for_high: i64,
    /// Score threshold below which memories are demoted to low
    pub demotion_threshold: f64,
    /// Score threshold below which memories can be removed
    pub removal_threshold: f64,
    /// Days without access before considering memory stale
    pub stale_days: i64,
    /// Days before new low-score memories can be demoted
    pub demotion_age_days: i64,
    /// Days before unused memories can be removed
    pub removal_age_days: i64,
}

impl Default for RankingConfig {
    fn default() -> Self {
        RankingConfig {
            high_threshold: 0.7,
            min_access_for_high: 3,
            demotion_threshold: 0.4,  // Moderate: demote below 0.4
            removal_threshold: 0.3,   // Moderate: remove below 0.3
            stale_days: 90,
            demotion_age_days: 14,    // Moderate: demote after 14 days
            removal_age_days: 30,     // Moderate: remove after 30 days
        }
    }
}

/// Weights for score calculation
#[derive(Debug, Clone)]
pub struct ScoreWeights {
    pub access: f64,
    pub confidence: f64,
    pub recency: f64,
    pub validated: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        ScoreWeights {
            access: 0.35,
            confidence: 0.25,
            recency: 0.25,
            validated: 0.15,
        }
    }
}

/// Memory data needed for ranking calculation
#[derive(Debug, Clone)]
pub struct MemoryForRanking {
    pub id: i64,
    pub state: String,
    pub confidence: f64,
    pub is_validated: bool,
    pub access_count: i64,
    pub extracted_at: DateTime<Utc>,
    pub last_accessed_at: Option<DateTime<Utc>>,
}

/// Represents a state transition for a memory
#[derive(Debug, Clone, Serialize)]
pub struct StateTransition {
    pub memory_id: i64,
    pub from_state: String,
    pub to_state: String,
    pub score: f64,
    pub reason: String,
}

/// Result of a ranking operation
#[derive(Debug, Clone, Serialize)]
pub struct RankingResult {
    pub project_id: String,
    pub memories_evaluated: usize,
    pub promoted: usize,
    pub demoted: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub transitions: Vec<StateTransition>,
}

/// Calculate the ranking score for a memory
pub fn calculate_memory_score(
    memory: &MemoryForRanking,
    weights: &ScoreWeights,
    now: DateTime<Utc>,
) -> f64 {
    // Access score: min(1.0, access_count / 10)
    let access_score = (memory.access_count as f64 / 10.0).min(1.0);

    // Recency score based on last access (or extraction if never accessed)
    let last_relevant_date = memory.last_accessed_at.unwrap_or(memory.extracted_at);
    let days_since = (now - last_relevant_date).num_days().max(0) as f64;
    let recency_score = (1.0 - days_since / 90.0).max(0.0);

    // Validated bonus
    let validated_score = if memory.is_validated { 1.0 } else { 0.0 };

    // Calculate weighted score
    (weights.access * access_score)
        + (weights.confidence * memory.confidence)
        + (weights.recency * recency_score)
        + (weights.validated * validated_score)
}

/// Determine state transition for a memory based on its score and current state
fn determine_transition(
    memory: &MemoryForRanking,
    score: f64,
    config: &RankingConfig,
    now: DateTime<Utc>,
) -> Option<StateTransition> {
    let age_days = (now - memory.extracted_at).num_days();
    let stale_days = memory
        .last_accessed_at
        .map(|la| (now - la).num_days())
        .unwrap_or(age_days);

    match memory.state.as_str() {
        "new" => {
            // Promote to high if score is good and has been accessed
            if score >= config.high_threshold && memory.access_count >= config.min_access_for_high {
                return Some(StateTransition {
                    memory_id: memory.id,
                    from_state: "new".to_string(),
                    to_state: "high".to_string(),
                    score,
                    reason: format!(
                        "Score {:.2} >= {:.2} with {} accesses",
                        score, config.high_threshold, memory.access_count
                    ),
                });
            }
            // Remove if very low score, old enough, and never accessed
            // Check removal BEFORE demotion (removal is more severe)
            if score < config.removal_threshold
                && age_days > config.removal_age_days
                && memory.access_count == 0
            {
                return Some(StateTransition {
                    memory_id: memory.id,
                    from_state: "new".to_string(),
                    to_state: "removed".to_string(),
                    score,
                    reason: format!(
                        "Score {:.2} < {:.2} after {} days with 0 accesses",
                        score, config.removal_threshold, age_days
                    ),
                });
            }
            // Demote to low if score is poor and old enough
            if score < config.demotion_threshold && age_days > config.demotion_age_days {
                return Some(StateTransition {
                    memory_id: memory.id,
                    from_state: "new".to_string(),
                    to_state: "low".to_string(),
                    score,
                    reason: format!(
                        "Score {:.2} < {:.2} after {} days",
                        score, config.demotion_threshold, age_days
                    ),
                });
            }
        }
        "low" => {
            // Promote to high if score improved significantly
            if score >= 0.6 && memory.access_count >= 5 {
                return Some(StateTransition {
                    memory_id: memory.id,
                    from_state: "low".to_string(),
                    to_state: "high".to_string(),
                    score,
                    reason: format!(
                        "Score improved to {:.2} with {} accesses",
                        score, memory.access_count
                    ),
                });
            }
        }
        "high" => {
            // Demote if stale and not validated
            if score < config.demotion_threshold
                && stale_days > config.stale_days
                && !memory.is_validated
            {
                return Some(StateTransition {
                    memory_id: memory.id,
                    from_state: "high".to_string(),
                    to_state: "low".to_string(),
                    score,
                    reason: format!(
                        "Score dropped to {:.2}, stale for {} days",
                        score, stale_days
                    ),
                });
            }
        }
        _ => {}
    }

    None
}

/// Fetch memories for ranking from the database
pub fn get_memories_for_ranking(
    db: &Database,
    project_id: &str,
    batch_size: usize,
) -> Result<Vec<MemoryForRanking>, String> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT id, state, confidence, is_validated, access_count,
                    extracted_at, last_accessed_at
             FROM memories
             WHERE project_id = ? AND state != 'removed'
             ORDER BY extracted_at DESC
             LIMIT ?",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let memories = stmt
        .query_map(params![project_id, batch_size as i64], |row| {
            let extracted_at_str: String = row.get(5)?;
            let last_accessed_str: Option<String> = row.get(6)?;

            Ok(MemoryForRanking {
                id: row.get(0)?,
                state: row.get(1)?,
                confidence: row.get(2)?,
                is_validated: row.get(3)?,
                access_count: row.get(4)?,
                extracted_at: DateTime::parse_from_rfc3339(&extracted_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                last_accessed_at: last_accessed_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
            })
        })
        .map_err(|e| format!("Failed to query memories: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(memories)
}

/// Apply state transitions to the database
fn apply_transitions(db: &Database, transitions: &[StateTransition]) -> Result<(), String> {
    let conn = db.conn();

    for transition in transitions {
        conn.execute(
            "UPDATE memories SET state = ? WHERE id = ?",
            params![transition.to_state, transition.memory_id],
        )
        .map_err(|e| format!("Failed to update memory {}: {}", transition.memory_id, e))?;
    }

    Ok(())
}

/// Rank all memories for a project
pub fn rank_project_memories(
    db: &Database,
    project_id: &str,
    batch_size: usize,
) -> Result<RankingResult, String> {
    let config = RankingConfig::default();
    let weights = ScoreWeights::default();
    let now = Utc::now();

    // Fetch memories
    let memories = get_memories_for_ranking(db, project_id, batch_size)?;
    let memories_evaluated = memories.len();

    // Calculate transitions
    let mut transitions = Vec::new();
    for memory in &memories {
        let score = calculate_memory_score(memory, &weights, now);
        if let Some(transition) = determine_transition(memory, score, &config, now) {
            transitions.push(transition);
        }
    }

    // Count by type
    let promoted = transitions
        .iter()
        .filter(|t| t.to_state == "high")
        .count();
    let demoted = transitions
        .iter()
        .filter(|t| t.to_state == "low")
        .count();
    let removed = transitions
        .iter()
        .filter(|t| t.to_state == "removed")
        .count();
    let unchanged = memories_evaluated - transitions.len();

    // Apply transitions
    apply_transitions(db, &transitions)?;

    Ok(RankingResult {
        project_id: project_id.to_string(),
        memories_evaluated,
        promoted,
        demoted,
        removed,
        unchanged,
        transitions,
    })
}

/// Rank memories for all projects
pub fn rank_all_projects(db: &Database, batch_size: usize) -> Vec<RankingResult> {
    let conn = db.conn();

    // Get all project IDs
    let project_ids: Vec<String> = conn
        .prepare("SELECT id FROM projects")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| row.get(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();

    let mut results = Vec::new();
    for project_id in project_ids {
        match rank_project_memories(db, &project_id, batch_size) {
            Ok(result) => {
                if result.memories_evaluated > 0 {
                    tracing::info!(
                        "Ranked project {}: {} evaluated, {} promoted, {} demoted, {} removed",
                        project_id,
                        result.memories_evaluated,
                        result.promoted,
                        result.demoted,
                        result.removed
                    );
                }
                results.push(result);
            }
            Err(e) => {
                tracing::error!("Failed to rank project {}: {}", project_id, e);
            }
        }
    }

    results
}

/// Get ranking statistics for a project without applying changes
pub fn get_ranking_stats(
    db: &Database,
    project_id: &str,
) -> Result<serde_json::Value, String> {
    let conn = db.conn();

    // Count by state
    let counts: Vec<(String, i64)> = conn
        .prepare(
            "SELECT state, COUNT(*)
             FROM memories
             WHERE project_id = ?
             GROUP BY state",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .map_err(|e| format!("Failed to get counts: {}", e))?;

    // Get average scores by state
    let avg_confidence: f64 = conn
        .query_row(
            "SELECT AVG(confidence) FROM memories WHERE project_id = ? AND state != 'removed'",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    // Build response
    let mut state_counts = serde_json::Map::new();
    for (state, count) in counts {
        state_counts.insert(state, serde_json::Value::Number(count.into()));
    }

    Ok(serde_json::json!({
        "project_id": project_id,
        "state_counts": state_counts,
        "avg_confidence": avg_confidence,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_calculate_score_new_high_value() {
        let weights = ScoreWeights::default();
        let now = Utc::now();

        let memory = MemoryForRanking {
            id: 1,
            state: "new".to_string(),
            confidence: 0.9,
            is_validated: true,
            access_count: 10,
            extracted_at: now - Duration::days(5),
            last_accessed_at: Some(now - Duration::days(1)),
        };

        let score = calculate_memory_score(&memory, &weights, now);
        assert!(score > 0.7, "High-value memory should have score > 0.7");
    }

    #[test]
    fn test_calculate_score_stale_memory() {
        let weights = ScoreWeights::default();
        let now = Utc::now();

        let memory = MemoryForRanking {
            id: 1,
            state: "new".to_string(),
            confidence: 0.5,
            is_validated: false,
            access_count: 0,
            extracted_at: now - Duration::days(100),
            last_accessed_at: None,
        };

        let score = calculate_memory_score(&memory, &weights, now);
        assert!(score < 0.3, "Stale unused memory should have low score");
    }

    #[test]
    fn test_transition_new_to_high() {
        let config = RankingConfig::default();
        let now = Utc::now();

        let memory = MemoryForRanking {
            id: 1,
            state: "new".to_string(),
            confidence: 0.9,
            is_validated: true,
            access_count: 5,
            extracted_at: now - Duration::days(10),
            last_accessed_at: Some(now - Duration::days(1)),
        };

        let score = 0.8;
        let transition = determine_transition(&memory, score, &config, now);

        assert!(transition.is_some());
        assert_eq!(transition.unwrap().to_state, "high");
    }

    #[test]
    fn test_validated_protected_from_demotion() {
        let config = RankingConfig::default();
        let now = Utc::now();

        let memory = MemoryForRanking {
            id: 1,
            state: "high".to_string(),
            confidence: 0.3,
            is_validated: true, // Protected
            access_count: 1,
            extracted_at: now - Duration::days(180),
            last_accessed_at: Some(now - Duration::days(100)),
        };

        let score = 0.2;
        let transition = determine_transition(&memory, score, &config, now);

        assert!(transition.is_none(), "Validated memories should not be demoted");
    }
}
