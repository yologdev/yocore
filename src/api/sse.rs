//! Server-Sent Events for real-time updates

use super::AppState;
use axum::{
    extract::State,
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::StreamExt;

/// SSE events handler
pub async fn events_handler(
    State(_state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // For now, just send periodic heartbeat events
    // TODO: Implement actual event broadcasting for session updates, new files, etc.
    let stream =
        tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(Duration::from_secs(30)))
            .map(|_| {
                Ok(Event::default().event("heartbeat").data(
                    serde_json::json!({ "timestamp": chrono::Utc::now().to_rfc3339() }).to_string(),
                ))
            });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
