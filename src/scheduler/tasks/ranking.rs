//! Periodic memory ranking task
//!
//! Migrated from Core::start_periodic_ranking() in lib.rs.
//! Evaluates memories across all projects and transitions their state
//! (new → high, new → low, high → low, etc.) based on access patterns.

use crate::ai;
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
    let batch_size = config.ai.features.ranking.batch_size;

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
                task_name: "ranking".to_string(),
                items_processed: 0,
                items_affected: 0,
                errors: 1,
                detail: format!("Failed to get project IDs: {}", e),
            };
        }
    };

    let mut total_evaluated = 0usize;
    let mut total_affected = 0usize;
    let mut total_errors = 0usize;

    for project_id in project_ids {
        // Emit start event (backward-compatible)
        let _ = event_tx.send(WatcherEvent::RankingStart {
            project_id: project_id.clone(),
        });

        let db_clone = db.clone();
        let pid = project_id.clone();
        let ranking_future = tokio::task::spawn_blocking(move || {
            ai::ranking::rank_project_memories(&db_clone, &pid, batch_size)
        });

        // Timeout after 60 seconds per project
        let result = tokio::time::timeout(Duration::from_secs(60), ranking_future).await;

        match result {
            Ok(Ok(Ok(ranking_result))) => {
                if ranking_result.memories_evaluated > 0 {
                    tracing::info!(
                        "Ranked project {}: {} evaluated, {} promoted, {} demoted, {} removed",
                        project_id,
                        ranking_result.memories_evaluated,
                        ranking_result.promoted,
                        ranking_result.demoted,
                        ranking_result.removed
                    );
                }

                total_evaluated += ranking_result.memories_evaluated;
                total_affected +=
                    ranking_result.promoted + ranking_result.demoted + ranking_result.removed;

                let _ = event_tx.send(WatcherEvent::RankingComplete {
                    project_id: project_id.clone(),
                    promoted: ranking_result.promoted,
                    demoted: ranking_result.demoted,
                    removed: ranking_result.removed,
                });
            }
            Ok(Ok(Err(e))) => {
                tracing::error!("Failed to rank project {}: {}", project_id, e);
                total_errors += 1;
                let _ = event_tx.send(WatcherEvent::RankingError {
                    project_id: project_id.clone(),
                    error: e,
                });
            }
            Ok(Err(e)) => {
                tracing::error!("Ranking task panicked for project {}: {}", project_id, e);
                total_errors += 1;
                let _ = event_tx.send(WatcherEvent::RankingError {
                    project_id: project_id.clone(),
                    error: format!("Task panicked: {}", e),
                });
            }
            Err(_) => {
                tracing::error!("Ranking timed out for project {}", project_id);
                total_errors += 1;
                let _ = event_tx.send(WatcherEvent::RankingError {
                    project_id: project_id.clone(),
                    error: "Ranking timed out after 60 seconds".to_string(),
                });
            }
        }

        tokio::task::yield_now().await;
    }

    TaskResult {
        task_name: "ranking".to_string(),
        items_processed: total_evaluated,
        items_affected: total_affected,
        errors: total_errors,
        detail: format!(
            "{} memories evaluated, {} state changes",
            total_evaluated, total_affected
        ),
    }
}
