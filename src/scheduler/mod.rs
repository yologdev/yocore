//! General-purpose periodic task scheduler
//!
//! Manages background tasks that run at configurable intervals:
//! - **Ranking**: Evaluate and transition memory states (new → high, etc.)
//! - **Duplicate cleanup**: Find and soft-remove near-duplicate memories
//! - **Embedding refresh**: Backfill embeddings for memories missing them
//! - **Skill cleanup**: Find and hard-delete near-duplicate skills
//!
//! Each task declares its feature dependencies (e.g., requires AI + memory_extraction).
//! The scheduler checks these per-task — future tasks with different dependencies
//! won't be incorrectly skipped.
//!
//! Each task runs in its own tokio::spawn with independent interval timers.
//! Tasks are staggered to avoid simultaneous DB contention.

pub mod tasks;

use crate::config::Config;
use crate::db::Database;
use crate::watcher::WatcherEvent;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

/// Result of a single scheduler task execution
pub struct TaskResult {
    pub task_name: String,
    pub items_processed: usize,
    pub items_affected: usize,
    pub errors: usize,
    pub detail: String,
}

/// Feature dependencies a task can require
#[derive(Debug, Clone, Copy)]
enum Requires {
    Ai,
    MemoryExtraction,
    SkillsDiscovery,
}

/// Registered periodic tasks
#[derive(Clone)]
enum ScheduledTask {
    Ranking,
    DuplicateCleanup,
    EmbeddingRefresh,
    SkillCleanup,
}

impl ScheduledTask {
    fn name(&self) -> &str {
        match self {
            ScheduledTask::Ranking => "ranking",
            ScheduledTask::DuplicateCleanup => "duplicate_cleanup",
            ScheduledTask::EmbeddingRefresh => "embedding_refresh",
            ScheduledTask::SkillCleanup => "skill_cleanup",
        }
    }

    /// Feature dependencies this task requires to run.
    /// All listed dependencies must be satisfied.
    fn requires(&self) -> &[Requires] {
        match self {
            ScheduledTask::Ranking => &[Requires::Ai, Requires::MemoryExtraction],
            ScheduledTask::DuplicateCleanup => &[Requires::Ai, Requires::MemoryExtraction],
            ScheduledTask::EmbeddingRefresh => &[Requires::Ai, Requires::MemoryExtraction],
            ScheduledTask::SkillCleanup => &[Requires::Ai, Requires::SkillsDiscovery],
        }
    }

    /// Check if all feature dependencies are met
    fn dependencies_met(&self, config: &Config) -> Result<(), String> {
        for req in self.requires() {
            let met = match req {
                Requires::Ai => config.ai.enabled,
                Requires::MemoryExtraction => config.ai.features.memory_extraction,
                Requires::SkillsDiscovery => config.ai.features.skills_discovery,
            };
            if !met {
                return Err(format!("{:?}", req));
            }
        }
        Ok(())
    }

    fn is_enabled(&self, config: &Config) -> bool {
        match self {
            ScheduledTask::Ranking => config.scheduler.ranking.enabled,
            ScheduledTask::DuplicateCleanup => config.scheduler.duplicate_cleanup.enabled,
            ScheduledTask::EmbeddingRefresh => config.scheduler.embedding_refresh.enabled,
            ScheduledTask::SkillCleanup => config.scheduler.skill_cleanup.enabled,
        }
    }

    fn interval_secs(&self, config: &Config) -> u64 {
        match self {
            ScheduledTask::Ranking => config.scheduler.ranking.interval_hours as u64 * 3600,
            ScheduledTask::DuplicateCleanup => {
                config.scheduler.duplicate_cleanup.interval_hours as u64 * 3600
            }
            ScheduledTask::EmbeddingRefresh => {
                config.scheduler.embedding_refresh.interval_hours as u64 * 3600
            }
            ScheduledTask::SkillCleanup => {
                config.scheduler.skill_cleanup.interval_hours as u64 * 3600
            }
        }
    }

    async fn execute(
        &self,
        db: Arc<Database>,
        config: &Config,
        event_tx: broadcast::Sender<WatcherEvent>,
    ) -> TaskResult {
        match self {
            ScheduledTask::Ranking => tasks::ranking::execute(db, config, event_tx).await,
            ScheduledTask::DuplicateCleanup => {
                tasks::duplicate_cleanup::execute(db, config, event_tx).await
            }
            ScheduledTask::EmbeddingRefresh => {
                tasks::embedding_refresh::execute(db, config, event_tx).await
            }
            ScheduledTask::SkillCleanup => {
                tasks::skill_cleanup::execute(db, config, event_tx).await
            }
        }
    }
}

/// Start a periodic WAL checkpoint task.
///
/// SQLite's `wal_autocheckpoint` can fail to trigger under high write contention
/// (single Mutex connection). This safety net runs every 5 minutes to force a
/// checkpoint, preventing the WAL from growing unbounded.
fn start_wal_checkpoint_task(db: Arc<Database>) {
    tokio::spawn(async move {
        let interval = Duration::from_secs(300); // 5 minutes
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await; // Skip immediate tick

        loop {
            ticker.tick().await;
            let result = db
                .with_conn(|conn| conn.execute("PRAGMA wal_checkpoint(PASSIVE)", []))
                .await;
            match result {
                Ok(_) => tracing::debug!("WAL checkpoint completed"),
                Err(e) => tracing::warn!("WAL checkpoint failed: {}", e),
            }
        }
    });
}

/// Start all enabled periodic tasks.
///
/// Each task declares its feature dependencies (AI, memory_extraction, etc.).
/// Tasks whose dependencies aren't met are skipped individually.
///
/// Each enabled task runs in its own tokio::spawn with an independent interval timer.
/// Tasks are staggered by 10 seconds to avoid simultaneous DB contention.
pub fn start_scheduler(
    config: Config,
    db: Arc<Database>,
    event_tx: broadcast::Sender<WatcherEvent>,
) {
    // Always run WAL checkpoint regardless of AI settings
    start_wal_checkpoint_task(db.clone());

    let all_tasks = [
        ScheduledTask::Ranking,
        ScheduledTask::DuplicateCleanup,
        ScheduledTask::EmbeddingRefresh,
        ScheduledTask::SkillCleanup,
    ];

    for (idx, task) in all_tasks.into_iter().enumerate() {
        // Check feature dependencies first
        if let Err(missing) = task.dependencies_met(&config) {
            tracing::info!(
                "Scheduler: task '{}' skipped (requires {} to be enabled)",
                task.name(),
                missing
            );
            continue;
        }

        // Then check the task's own enabled flag
        if !task.is_enabled(&config) {
            tracing::info!("Scheduler: task '{}' is disabled", task.name());
            continue;
        }

        let interval_secs = task.interval_secs(&config);
        tracing::info!(
            "Scheduler: starting task '{}' (every {} hours)",
            task.name(),
            interval_secs / 3600
        );

        let config = config.clone();
        let db = db.clone();
        let event_tx = event_tx.clone();
        let stagger = Duration::from_secs(idx as u64 * 10);

        tokio::spawn(async move {
            // Stagger start to avoid simultaneous execution
            tokio::time::sleep(stagger).await;

            let interval = Duration::from_secs(interval_secs);
            let mut ticker = tokio::time::interval(interval);

            // Skip the first immediate tick (tasks run after the interval, not immediately)
            ticker.tick().await;

            loop {
                ticker.tick().await;
                tracing::info!("Scheduler: running task '{}'", task.name());

                let result = task.execute(db.clone(), &config, event_tx.clone()).await;

                if result.errors > 0 {
                    tracing::warn!(
                        "Scheduler: task '{}' completed with {} errors: {}",
                        task.name(),
                        result.errors,
                        result.detail
                    );
                } else if result.items_affected > 0 {
                    tracing::info!(
                        "Scheduler: task '{}' completed: {}",
                        task.name(),
                        result.detail
                    );
                } else {
                    tracing::debug!("Scheduler: task '{}' completed (no changes)", task.name());
                }
            }
        });
    }
}
