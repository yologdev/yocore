//! HTTP API module for Yolog Core
//!
//! Provides REST API endpoints for sessions, projects, memories, and search.

mod auth;
pub mod routes;
mod sse;

use crate::ai::queue::AiTaskQueue;
use crate::ai::types::AiEvent;
use crate::config::Config;
use crate::db::Database;
use crate::error::Result;
use crate::watcher::WatcherEvent;

use axum::{
    middleware,
    routing::{delete, get, patch, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// Application state shared across all handlers
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub api_key: Option<String>,
    /// Broadcast channel for SSE events from watcher
    pub event_tx: broadcast::Sender<WatcherEvent>,
    /// Broadcast channel for AI-related SSE events
    pub ai_event_tx: broadcast::Sender<AiEvent>,
    /// AI task queue for concurrency control
    pub ai_task_queue: AiTaskQueue,
}

/// Start the HTTP API server
pub async fn serve(
    addr: SocketAddr,
    db: Arc<Database>,
    config: &Config,
    event_tx: broadcast::Sender<WatcherEvent>,
    ai_event_tx: broadcast::Sender<AiEvent>,
    ai_task_queue: AiTaskQueue,
) -> Result<()> {
    let state = AppState {
        db,
        api_key: config.server.api_key.clone(),
        event_tx,
        ai_event_tx,
        ai_task_queue,
    };

    let app = create_router(state);

    tracing::info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| crate::error::CoreError::Api(e.to_string()))?;

    Ok(())
}

/// Create the API router with all routes
fn create_router(state: AppState) -> Router {
    // CORS configuration - allow all origins for development
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Protected API routes (require auth if api_key is configured)
    let api_routes = Router::new()
        // Projects
        .route("/projects", get(routes::list_projects))
        .route("/projects", post(routes::create_project))
        .route("/projects/:id", get(routes::get_project))
        .route("/projects/:id", patch(routes::update_project))
        .route("/projects/:id", delete(routes::delete_project))
        // Sessions
        .route("/sessions", get(routes::list_sessions))
        .route("/sessions/:id", get(routes::get_session))
        .route("/sessions/:id", patch(routes::update_session))
        .route("/sessions/:id", delete(routes::delete_session))
        .route("/sessions/:id/messages", get(routes::get_session_messages))
        .route(
            "/sessions/:id/messages/:seq/content",
            get(routes::get_message_content),
        )
        .route("/sessions/:id/markers", get(routes::get_session_markers))
        .route("/sessions/:id/search", get(routes::search_session))
        // Search
        .route("/search", post(routes::search))
        // Memories
        .route("/memories", get(routes::list_memories))
        .route("/memories/search", post(routes::search_memories))
        .route("/memories/:id", get(routes::get_memory))
        .route("/memories/:id", patch(routes::update_memory))
        .route("/memories/:id", delete(routes::delete_memory))
        // Memory Stats & Tags
        .route("/projects/:id/memory-stats", get(routes::get_memory_stats))
        .route("/projects/:id/memory-tags", get(routes::get_memory_tags))
        // Markers
        .route("/markers/:id", delete(routes::delete_marker))
        // AI Features
        .route("/ai/sessions/:id/title", post(routes::trigger_title_generation))
        .route("/ai/sessions/:id/memories", post(routes::trigger_memory_extraction))
        .route("/ai/sessions/:id/skills", post(routes::trigger_skill_extraction))
        .route("/ai/sessions/:id/markers", post(routes::trigger_marker_detection))
        .route("/ai/cli/status", get(routes::get_ai_cli_status))
        // AI Settings
        .route("/ai/settings", get(routes::get_ai_settings))
        .route("/ai/settings", patch(routes::update_ai_settings))
        .route("/ai/privacy/accept", post(routes::accept_ai_privacy))
        // Memory Ranking
        .route(
            "/projects/:id/rank-memories",
            post(routes::rank_project_memories),
        )
        .route(
            "/projects/:id/ranking-stats",
            get(routes::get_ranking_stats),
        )
        // Skills
        .route("/projects/:id/skills", get(routes::list_project_skills))
        .route("/projects/:id/skills/stats", get(routes::get_skill_stats))
        .route("/skills/:id", delete(routes::delete_skill_by_id))
        // Server-Sent Events
        .route("/events", get(sse::events_handler))
        // Apply auth middleware to all API routes
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ));

    Router::new()
        // Health check (public, no auth required)
        .route("/health", get(routes::health))
        // Nest protected routes under /api
        .nest("/api", api_routes)
        // Global middleware
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

/// Graceful shutdown signal handler
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown");
}
