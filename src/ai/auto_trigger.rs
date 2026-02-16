//! AI Auto-Trigger
//!
//! Automatically triggers AI tasks (title, memory, skills) after session parsing.
//! Replaces the Desktop-side background-sync.ts logic — yocore now owns the full pipeline.

use crate::ai::cli::CliProvider;
use crate::ai::title::{generate_title, store_title};
use crate::ai::types::AiEvent;
use crate::ai::AiTaskQueue;
use crate::config::Config;
use crate::db::Database;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Minimum message count before triggering AI extraction
const EXTRACTION_THRESHOLD: usize = 10;

/// Messages between periodic extraction triggers
const EXTRACTION_INTERVAL: usize = 50;

/// Minimum messages for title generation
const MIN_MESSAGES_FOR_TITLE: usize = 25;

/// Handles automatic AI task triggering after session parsing
pub struct AiAutoTrigger {
    config_path: PathBuf,
    db: Arc<Database>,
    ai_event_tx: broadcast::Sender<AiEvent>,
    ai_task_queue: AiTaskQueue,
    /// Track message count at last extraction per session
    extraction_tracker: HashMap<String, usize>,
    /// Configured AI CLI provider
    provider: CliProvider,
}

impl AiAutoTrigger {
    pub fn new(
        config_path: PathBuf,
        db: Arc<Database>,
        ai_event_tx: broadcast::Sender<AiEvent>,
        ai_task_queue: AiTaskQueue,
        provider: CliProvider,
    ) -> Self {
        Self {
            config_path,
            db,
            ai_event_tx,
            ai_task_queue,
            extraction_tracker: HashMap::new(),
            provider,
        }
    }

    /// Check config and trigger appropriate AI tasks after a session parse
    pub async fn on_session_parsed(&mut self, session_id: &str, message_count: usize) {
        // Read config to check which features are enabled
        let config = match Config::from_file(&self.config_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("Auto-trigger: failed to read config: {}", e);
                return;
            }
        };

        if !config.is_ai_active() {
            return;
        }

        // Title generation: check if session needs one
        if config.is_feature_active(crate::config::AiFeature::TitleGeneration)
            && message_count >= MIN_MESSAGES_FOR_TITLE
        {
            self.maybe_trigger_title(session_id).await;
        }

        // Memory & Skills extraction: threshold-based
        if self.should_trigger_extraction(session_id, message_count) {
            self.record_extraction(session_id, message_count);

            if config.is_feature_active(crate::config::AiFeature::MemoryExtraction) {
                self.trigger_memory_extraction(session_id).await;
            }
            if config.is_feature_active(crate::config::AiFeature::SkillsDiscovery) {
                self.trigger_skill_extraction(session_id).await;
            }
        }
    }

    /// Check if we should trigger extraction based on message count thresholds
    fn should_trigger_extraction(&self, session_id: &str, message_count: usize) -> bool {
        let last_count = self
            .extraction_tracker
            .get(session_id)
            .copied()
            .unwrap_or(0);

        // First extraction: cross the threshold
        if last_count < EXTRACTION_THRESHOLD && message_count >= EXTRACTION_THRESHOLD {
            return true;
        }

        // Periodic extraction: every EXTRACTION_INTERVAL messages
        if message_count >= EXTRACTION_THRESHOLD
            && message_count - last_count >= EXTRACTION_INTERVAL
        {
            return true;
        }

        false
    }

    fn record_extraction(&mut self, session_id: &str, message_count: usize) {
        self.extraction_tracker
            .insert(session_id.to_string(), message_count);
    }

    /// Trigger title generation if session doesn't have an AI-generated or user-edited title
    async fn maybe_trigger_title(&self, session_id: &str) {
        let db = self.db.clone();
        let sid = session_id.to_string();

        // Check if title is needed
        let needs_title = match db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT COALESCE(title_ai_generated, 0), COALESCE(title_edited, 0) FROM sessions WHERE id = ?",
                    [&sid],
                    |row| {
                        let ai_gen: bool = row.get(0)?;
                        let edited: bool = row.get(1)?;
                        Ok(!ai_gen && !edited)
                    },
                )
            })
            .await
        {
            Ok(needs) => needs,
            Err(_) => return,
        };

        if !needs_title {
            return;
        }

        // Acquire task queue permit
        let permit = match self.ai_task_queue.acquire().await {
            Ok(p) => p,
            Err(_) => return,
        };

        let db = self.db.clone();
        let ai_event_tx = self.ai_event_tx.clone();
        let session_id = session_id.to_string();
        let provider = self.provider;

        tokio::spawn(async move {
            let _permit = permit;
            let sid = session_id.clone();

            let _ = ai_event_tx.send(AiEvent::TitleStart {
                session_id: sid.clone(),
            });

            let result = generate_title(&db, &sid, None, provider).await;

            if let Some(ref title) = result.title {
                if let Err(e) = store_title(&db, &sid, title).await {
                    tracing::error!(
                        "Auto-trigger: failed to store title for {}: {}",
                        &sid[..8],
                        e
                    );
                    let _ = ai_event_tx.send(AiEvent::TitleError {
                        session_id: sid,
                        error: format!("Failed to store title: {}", e),
                    });
                    return;
                }
                tracing::info!("Auto-trigger: title generated for {}", &sid[..8]);
                let _ = ai_event_tx.send(AiEvent::TitleComplete {
                    session_id: sid,
                    title: title.clone(),
                });
            } else if let Some(error) = result.error {
                tracing::warn!(
                    "Auto-trigger: title generation failed for {}: {}",
                    &sid[..8],
                    error
                );
                let _ = ai_event_tx.send(AiEvent::TitleError {
                    session_id: sid,
                    error,
                });
            }
        });
    }

    async fn trigger_memory_extraction(&self, session_id: &str) {
        let permit = match self.ai_task_queue.acquire().await {
            Ok(p) => p,
            Err(_) => return,
        };

        let db = self.db.clone();
        let ai_event_tx = self.ai_event_tx.clone();
        let session_id = session_id.to_string();
        let provider = self.provider;

        tokio::spawn(async move {
            let _permit = permit;
            let sid = session_id.clone();

            let _ = ai_event_tx.send(AiEvent::MemoryStart {
                session_id: sid.clone(),
            });

            let result = crate::ai::extract_memories(&db, &sid, None, false, provider).await;

            if let Some(error) = result.error {
                tracing::warn!(
                    "Auto-trigger: memory extraction failed for {}: {}",
                    &sid[..8],
                    error
                );
                let _ = ai_event_tx.send(AiEvent::MemoryError {
                    session_id: sid,
                    error,
                });
            } else {
                tracing::info!(
                    "Auto-trigger: extracted {} memories for {}",
                    result.memories_extracted,
                    &sid[..8]
                );
                let _ = ai_event_tx.send(AiEvent::MemoryComplete {
                    session_id: sid,
                    count: result.memories_extracted,
                });
            }
        });
    }

    async fn trigger_skill_extraction(&self, session_id: &str) {
        let permit = match self.ai_task_queue.acquire().await {
            Ok(p) => p,
            Err(_) => return,
        };

        let db = self.db.clone();
        let ai_event_tx = self.ai_event_tx.clone();
        let session_id = session_id.to_string();
        let provider = self.provider;

        tokio::spawn(async move {
            let _permit = permit;
            let sid = session_id.clone();

            let _ = ai_event_tx.send(AiEvent::SkillStart {
                session_id: sid.clone(),
            });

            let result = crate::ai::extract_skills(&db, &sid, None, false, provider).await;

            if let Some(error) = result.error {
                tracing::warn!(
                    "Auto-trigger: skill extraction failed for {}: {}",
                    &sid[..8],
                    error
                );
                let _ = ai_event_tx.send(AiEvent::SkillError {
                    session_id: sid,
                    error,
                });
            } else {
                tracing::info!(
                    "Auto-trigger: extracted {} skills for {}",
                    result.skills_extracted,
                    &sid[..8]
                );
                let _ = ai_event_tx.send(AiEvent::SkillComplete {
                    session_id: sid,
                    count: result.skills_extracted,
                });
            }
        });
    }
}
