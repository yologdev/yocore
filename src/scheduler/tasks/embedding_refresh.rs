//! Periodic embedding refresh task
//!
//! Backfills embeddings for memories that don't have them yet.
//! This handles cases where extraction happened before embeddings were enabled,
//! or where embedding generation failed during extraction.

use crate::config::Config;
use crate::db::Database;
use crate::embeddings;
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
    let batch_size = config.scheduler.embedding_refresh.batch_size;

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
                task_name: "embedding_refresh".to_string(),
                items_processed: 0,
                items_affected: 0,
                errors: 1,
                detail: format!("Failed to get project IDs: {}", e),
            };
        }
    };

    let mut total_found = 0usize;
    let mut total_embedded = 0usize;
    let mut total_errors = 0usize;

    for project_id in project_ids {
        let _ = event_tx.send(WatcherEvent::SchedulerTaskStart {
            task_name: "embedding_refresh".to_string(),
            project_id: project_id.clone(),
        });

        let db_clone = db.clone();
        let pid = project_id.clone();
        let embed_future = tokio::task::spawn_blocking(move || {
            refresh_project_embeddings(&db_clone, &pid, batch_size)
        });

        // 5-minute timeout per project (embeddings are CPU-intensive)
        let result = tokio::time::timeout(Duration::from_secs(300), embed_future).await;

        match result {
            Ok(Ok(Ok((found, embedded, failed)))) => {
                if embedded > 0 {
                    tracing::info!(
                        "Embedding refresh for project {}: {} missing, {} embedded, {} failed",
                        project_id,
                        found,
                        embedded,
                        failed
                    );
                }
                total_found += found;
                total_embedded += embedded;
                total_errors += failed;

                let _ = event_tx.send(WatcherEvent::SchedulerTaskComplete {
                    task_name: "embedding_refresh".to_string(),
                    project_id: project_id.clone(),
                    detail: format!(
                        "{} missing, {} embedded, {} failed",
                        found, embedded, failed
                    ),
                });
            }
            Ok(Ok(Err(e))) => {
                tracing::error!("Embedding refresh failed for project {}: {}", project_id, e);
                total_errors += 1;
                let _ = event_tx.send(WatcherEvent::SchedulerTaskError {
                    task_name: "embedding_refresh".to_string(),
                    project_id: project_id.clone(),
                    error: e,
                });
            }
            Ok(Err(e)) => {
                tracing::error!(
                    "Embedding refresh panicked for project {}: {}",
                    project_id,
                    e
                );
                total_errors += 1;
                let _ = event_tx.send(WatcherEvent::SchedulerTaskError {
                    task_name: "embedding_refresh".to_string(),
                    project_id: project_id.clone(),
                    error: format!("Task panicked: {}", e),
                });
            }
            Err(_) => {
                tracing::error!("Embedding refresh timed out for project {}", project_id);
                total_errors += 1;
                let _ = event_tx.send(WatcherEvent::SchedulerTaskError {
                    task_name: "embedding_refresh".to_string(),
                    project_id: project_id.clone(),
                    error: "Timed out after 300 seconds".to_string(),
                });
            }
        }

        tokio::task::yield_now().await;
    }

    TaskResult {
        task_name: "embedding_refresh".to_string(),
        items_processed: total_found,
        items_affected: total_embedded,
        errors: total_errors,
        detail: format!(
            "{} missing embeddings found, {} embedded",
            total_found, total_embedded
        ),
    }
}

/// Backfill embeddings for memories in a project that don't have them yet.
fn refresh_project_embeddings(
    db: &Database,
    project_id: &str,
    batch_size: usize,
) -> Result<(usize, usize, usize), String> {
    #[allow(deprecated)]
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT m.id, m.title, m.content FROM memories m
             LEFT JOIN memory_embeddings me ON m.id = me.memory_id
             WHERE m.project_id = ? AND m.state != 'removed' AND me.memory_id IS NULL
             LIMIT ?",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let memories: Vec<(i64, String, String)> = stmt
        .query_map(rusqlite::params![project_id, batch_size], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|e| format!("Failed to query memories: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    let found = memories.len();
    if found == 0 {
        return Ok((0, 0, 0));
    }

    let mut success = 0usize;
    let mut failed = 0usize;

    for (id, title, content) in &memories {
        let text = format!("{}\n{}", title, content);
        match embeddings::embed_text(&text) {
            Ok(embedding) => {
                let bytes = embeddings::embedding_to_bytes(&embedding);
                match conn.execute(
                    "INSERT OR REPLACE INTO memory_embeddings (memory_id, embedding) VALUES (?, ?)",
                    rusqlite::params![id, bytes],
                ) {
                    Ok(_) => success += 1,
                    Err(e) => {
                        tracing::warn!("Failed to store embedding for memory {}: {}", id, e);
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to embed memory {}: {}", id, e);
                failed += 1;
            }
        }
    }

    Ok((found, success, failed))
}
