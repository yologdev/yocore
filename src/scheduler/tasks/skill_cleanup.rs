//! Periodic duplicate skill cleanup task
//!
//! Scans skills per project and hard-deletes near-duplicates using Jaccard similarity.
//! Uses a stricter threshold (0.80) than extraction-time dedup (0.70) to minimize
//! false positives on retroactive cleanup.
//!
//! Unlike memories (which use soft-delete via state='removed'), skills have no state
//! column — duplicates are permanently deleted.

use crate::ai::similarity;
use crate::config::Config;
use crate::db::Database;
use crate::scheduler::TaskResult;
use crate::watcher::WatcherEvent;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

pub async fn execute(
    db: Arc<Database>,
    config: &Config,
    event_tx: broadcast::Sender<WatcherEvent>,
) -> TaskResult {
    let threshold = config.ai.features.skill_cleanup.similarity_threshold;
    let batch_size = config.ai.features.skill_cleanup.batch_size;

    // Get all project IDs
    let db_clone = db.clone();
    let project_ids: Vec<String> = match tokio::task::spawn_blocking(move || {
        #[allow(deprecated)]
        let conn = db_clone.conn();
        conn.prepare("SELECT id FROM projects")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get(0))
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    })
    .await
    {
        Ok(ids) => ids,
        Err(e) => {
            return TaskResult {
                task_name: "skill_cleanup".to_string(),
                items_processed: 0,
                items_affected: 0,
                errors: 1,
                detail: format!("Failed to get project IDs: {}", e),
            };
        }
    };

    let mut total_scanned = 0usize;
    let mut total_removed = 0usize;
    let mut total_errors = 0usize;

    for project_id in project_ids {
        let _ = event_tx.send(WatcherEvent::SchedulerTaskStart {
            task_name: "skill_cleanup".to_string(),
            project_id: project_id.clone(),
        });

        let db_clone = db.clone();
        let pid = project_id.clone();
        let cleanup_future = tokio::task::spawn_blocking(move || {
            cleanup_project_skills(&db_clone, &pid, threshold, batch_size)
        });

        // Timeout after 120 seconds per project
        let result = tokio::time::timeout(Duration::from_secs(120), cleanup_future).await;

        match result {
            Ok(Ok(Ok((scanned, removed)))) => {
                if removed > 0 {
                    tracing::info!(
                        "Skill cleanup for project {}: {} scanned, {} removed",
                        project_id,
                        scanned,
                        removed
                    );
                }
                total_scanned += scanned;
                total_removed += removed;

                let _ = event_tx.send(WatcherEvent::SchedulerTaskComplete {
                    task_name: "skill_cleanup".to_string(),
                    project_id: project_id.clone(),
                    detail: format!("{} scanned, {} duplicates removed", scanned, removed),
                });
            }
            Ok(Ok(Err(e))) => {
                tracing::error!("Skill cleanup failed for project {}: {}", project_id, e);
                total_errors += 1;
                let _ = event_tx.send(WatcherEvent::SchedulerTaskError {
                    task_name: "skill_cleanup".to_string(),
                    project_id: project_id.clone(),
                    error: e,
                });
            }
            Ok(Err(e)) => {
                tracing::error!("Skill cleanup panicked for project {}: {}", project_id, e);
                total_errors += 1;
                let _ = event_tx.send(WatcherEvent::SchedulerTaskError {
                    task_name: "skill_cleanup".to_string(),
                    project_id: project_id.clone(),
                    error: format!("Task panicked: {}", e),
                });
            }
            Err(_) => {
                tracing::error!("Skill cleanup timed out for project {}", project_id);
                total_errors += 1;
                let _ = event_tx.send(WatcherEvent::SchedulerTaskError {
                    task_name: "skill_cleanup".to_string(),
                    project_id: project_id.clone(),
                    error: "Timed out after 120 seconds".to_string(),
                });
            }
        }

        tokio::task::yield_now().await;
    }

    TaskResult {
        task_name: "skill_cleanup".to_string(),
        items_processed: total_scanned,
        items_affected: total_removed,
        errors: total_errors,
        detail: format!(
            "{} skills scanned, {} duplicates removed",
            total_scanned, total_removed
        ),
    }
}

/// Scan a project's skills for duplicates and hard-delete them.
///
/// Orders by extracted_at ASC (oldest first) so we keep the older, established skill
/// and delete the newer duplicate. Skills have no state column, so this is a hard delete.
fn cleanup_project_skills(
    db: &Database,
    project_id: &str,
    threshold: f64,
    batch_size: usize,
) -> Result<(usize, usize), String> {
    #[allow(deprecated)]
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT id, name, description FROM skills
             WHERE project_id = ?
             ORDER BY extracted_at ASC
             LIMIT ?",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let skills: Vec<(i64, String, String)> = stmt
        .query_map(rusqlite::params![project_id, batch_size], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|e| format!("Failed to query skills: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    let scanned = skills.len();
    let mut seen: Vec<(i64, String, String)> = Vec::new();
    let mut duplicate_ids: Vec<i64> = Vec::new();

    for (id, name, description) in &skills {
        let is_dup = seen
            .iter()
            .any(|(_, sn, sd)| similarity::is_similar_skill(name, description, sn, sd, threshold));
        if is_dup {
            duplicate_ids.push(*id);
        } else {
            seen.push((*id, name.clone(), description.clone()));
        }
    }

    // Hard-delete duplicates (skills have no state column)
    for id in &duplicate_ids {
        conn.execute("DELETE FROM skills WHERE id = ?", rusqlite::params![id])
            .map_err(|e| format!("Failed to delete duplicate skill {}: {}", id, e))?;

        // Also clean up skill embeddings
        let _ = conn.execute(
            "DELETE FROM skill_embeddings WHERE skill_id = ?",
            rusqlite::params![id],
        );
    }

    Ok((scanned, duplicate_ids.len()))
}
