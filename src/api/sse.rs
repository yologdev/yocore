//! Server-Sent Events for real-time updates

use super::AppState;
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

/// SSE events handler
pub async fn events_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Subscribe to the broadcast channel
    let event_rx = state.event_tx.subscribe();

    // Create stream from broadcast receiver
    let broadcast_stream = BroadcastStream::new(event_rx).filter_map(|result| {
        match result {
            Ok(watcher_event) => {
                let sse_event: SseEvent = watcher_event.into();
                let event_type = match &sse_event {
                    SseEvent::Heartbeat { .. } => "heartbeat",
                    SseEvent::SessionNew { .. } => "session:new",
                    SseEvent::SessionChanged { .. } => "session:changed",
                    SseEvent::SessionParsed { .. } => "session:parsed",
                    SseEvent::WatcherError { .. } => "watcher:error",
                };
                let data = serde_json::to_string(&sse_event).unwrap_or_default();
                Some(Ok(Event::default().event(event_type).data(data)))
            }
            Err(_) => None, // Lagged, skip
        }
    });

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
