//! Server-Sent Events for real-time updates

use super::AppState;
use crate::ai::types::AiEvent;
use crate::watcher::WatcherEvent;
use axum::{
    extract::State,
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

/// SSE event types sent to clients
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SseEvent {
    /// Heartbeat to keep connection alive
    Heartbeat { timestamp: String },
    /// New session file detected
    SessionNew {
        project_id: String,
        file_path: String,
        file_name: String,
    },
    /// Session file changed (grew)
    SessionChanged {
        session_id: String,
        file_path: String,
        previous_size: u64,
        new_size: u64,
    },
    /// Session parsing completed
    SessionParsed {
        session_id: String,
        message_count: usize,
    },
    /// Watcher error
    WatcherError { file_path: String, error: String },
    // AI Events
    /// Title generation started
    AiTitleStart { session_id: String },
    /// Title generation completed
    AiTitleComplete { session_id: String, title: String },
    /// Title generation failed
    AiTitleError { session_id: String, error: String },
    /// Memory extraction started
    AiMemoryStart { session_id: String },
    /// Memory extraction completed
    AiMemoryComplete { session_id: String, count: usize },
    /// Memory extraction failed
    AiMemoryError { session_id: String, error: String },
    /// Skill extraction started
    AiSkillStart { session_id: String },
    /// Skill extraction completed
    AiSkillComplete { session_id: String, count: usize },
    /// Skill extraction failed
    AiSkillError { session_id: String, error: String },
}

impl From<WatcherEvent> for SseEvent {
    fn from(event: WatcherEvent) -> Self {
        match event {
            WatcherEvent::NewSession {
                project_id,
                file_path,
                file_name,
            } => SseEvent::SessionNew {
                project_id,
                file_path,
                file_name,
            },
            WatcherEvent::SessionChanged {
                session_id,
                file_path,
                previous_size,
                new_size,
            } => SseEvent::SessionChanged {
                session_id,
                file_path,
                previous_size,
                new_size,
            },
            WatcherEvent::SessionParsed {
                session_id,
                message_count,
            } => SseEvent::SessionParsed {
                session_id,
                message_count,
            },
            WatcherEvent::Error { file_path, error } => {
                SseEvent::WatcherError { file_path, error }
            }
        }
    }
}

impl From<AiEvent> for SseEvent {
    fn from(event: AiEvent) -> Self {
        match event {
            AiEvent::TitleStart { session_id } => SseEvent::AiTitleStart { session_id },
            AiEvent::TitleComplete { session_id, title } => {
                SseEvent::AiTitleComplete { session_id, title }
            }
            AiEvent::TitleError { session_id, error } => {
                SseEvent::AiTitleError { session_id, error }
            }
            AiEvent::MemoryStart { session_id } => SseEvent::AiMemoryStart { session_id },
            AiEvent::MemoryComplete { session_id, count } => {
                SseEvent::AiMemoryComplete { session_id, count }
            }
            AiEvent::MemoryError { session_id, error } => {
                SseEvent::AiMemoryError { session_id, error }
            }
            AiEvent::SkillStart { session_id } => SseEvent::AiSkillStart { session_id },
            AiEvent::SkillComplete { session_id, count } => {
                SseEvent::AiSkillComplete { session_id, count }
            }
            AiEvent::SkillError { session_id, error } => {
                SseEvent::AiSkillError { session_id, error }
            }
        }
    }
}

/// Get the SSE event type name
fn get_event_type(event: &SseEvent) -> &'static str {
    match event {
        SseEvent::Heartbeat { .. } => "heartbeat",
        SseEvent::SessionNew { .. } => "session:new",
        SseEvent::SessionChanged { .. } => "session:changed",
        SseEvent::SessionParsed { .. } => "session:parsed",
        SseEvent::WatcherError { .. } => "watcher:error",
        // AI events
        SseEvent::AiTitleStart { .. } => "ai:title:start",
        SseEvent::AiTitleComplete { .. } => "ai:title:complete",
        SseEvent::AiTitleError { .. } => "ai:title:error",
        SseEvent::AiMemoryStart { .. } => "ai:memory:start",
        SseEvent::AiMemoryComplete { .. } => "ai:memory:complete",
        SseEvent::AiMemoryError { .. } => "ai:memory:error",
        SseEvent::AiSkillStart { .. } => "ai:skill:start",
        SseEvent::AiSkillComplete { .. } => "ai:skill:complete",
        SseEvent::AiSkillError { .. } => "ai:skill:error",
    }
}

/// SSE events handler
pub async fn events_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Subscribe to the watcher broadcast channel
    let watcher_rx = state.event_tx.subscribe();

    // Subscribe to the AI broadcast channel
    let ai_rx = state.ai_event_tx.subscribe();

    // Create stream from watcher broadcast receiver
    let watcher_stream = BroadcastStream::new(watcher_rx).filter_map(|result| {
        match result {
            Ok(watcher_event) => {
                let sse_event: SseEvent = watcher_event.into();
                let event_type = get_event_type(&sse_event);
                let data = serde_json::to_string(&sse_event).unwrap_or_default();
                Some(Ok(Event::default().event(event_type).data(data)))
            }
            Err(_) => None, // Lagged, skip
        }
    });

    // Create stream from AI broadcast receiver
    let ai_stream = BroadcastStream::new(ai_rx).filter_map(|result| {
        match result {
            Ok(ai_event) => {
                let sse_event: SseEvent = ai_event.into();
                let event_type = get_event_type(&sse_event);
                let data = serde_json::to_string(&sse_event).unwrap_or_default();
                Some(Ok(Event::default().event(event_type).data(data)))
            }
            Err(_) => None, // Lagged, skip
        }
    });

    // Merge watcher and AI streams
    let broadcast_stream = futures::stream::select(watcher_stream, ai_stream);

    // Create heartbeat stream
    let heartbeat_stream =
        tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(Duration::from_secs(30)))
            .map(|_| {
                let event = SseEvent::Heartbeat {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };
                Ok(Event::default()
                    .event("heartbeat")
                    .data(serde_json::to_string(&event).unwrap_or_default()))
            });

    // Merge both streams
    let merged_stream = futures::stream::select(broadcast_stream, heartbeat_stream);

    Sse::new(merged_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
