//! HTTP API module for Yolog Core
//!
//! Provides REST API endpoints for sessions, projects, memories, and search.

mod auth;
mod config_routes;
mod context_routes;
pub mod routes;
mod sse;

use crate::ai::queue::AiTaskQueue;
use crate::ai::types::AiEvent;
use crate::config::{Config, Storage};
use crate::db::Database;
use crate::ephemeral::EphemeralIndex;
use crate::error::Result;
use crate::watcher::WatcherEvent;

use axum::{
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// Application state shared across all handlers
#[derive(Clone)]
pub struct AppState {
    /// Database connection (None in ephemeral mode)
    pub db: Option<Arc<Database>>,
    /// Ephemeral in-memory index (None in db mode)
    pub ephemeral: Option<Arc<EphemeralIndex>>,
    /// Active storage mode
    pub storage: Storage,
    pub api_key: Option<String>,
    /// Broadcast channel for SSE events from watcher
    pub event_tx: broadcast::Sender<WatcherEvent>,
    /// Broadcast channel for AI-related SSE events
    pub ai_event_tx: broadcast::Sender<AiEvent>,
    /// AI task queue for concurrency control
    pub ai_task_queue: AiTaskQueue,
    /// Path to the config file (for config API)
    pub config_path: std::path::PathBuf,
}

/// Start the HTTP API server
#[allow(clippy::too_many_arguments)]
pub async fn serve(
    addr: SocketAddr,
    db: Option<Arc<Database>>,
    ephemeral: Option<Arc<EphemeralIndex>>,
    config: &Config,
    config_path: std::path::PathBuf,
    event_tx: broadcast::Sender<WatcherEvent>,
    ai_event_tx: broadcast::Sender<AiEvent>,
    ai_task_queue: AiTaskQueue,
) -> Result<()> {
    let state = AppState {
        db: db.clone(),
        ephemeral,
        storage: config.storage.clone(),
        api_key: config.server.api_key.clone(),
        event_tx,
        ai_event_tx,
        ai_task_queue,
        config_path,
    };

    let app = create_router(state);

    // DB-specific initialization (instance UUID, instance name)
    if let Some(db) = &db {
        if let Err(e) = db
            .with_conn(crate::db::schema::get_or_create_instance_uuid)
            .await
        {
            tracing::warn!("Failed to initialize instance UUID: {}", e);
        }

        let instance_name = config.server.instance_name.clone();
        if let Err(e) = db
            .with_conn(move |conn| {
                crate::db::schema::set_instance_name(conn, instance_name.as_deref())
            })
            .await
        {
            tracing::warn!("Failed to set instance name: {}", e);
        }
    }

    // Check if port is already in use (another yocore instance running)
    if tokio::net::TcpStream::connect(addr).await.is_ok() {
        tracing::error!(
            "Port {} is already in use — another yocore instance may be running. \
             Use `curl http://{}/health` to check.",
            addr.port(),
            addr
        );
        return Err(crate::error::CoreError::Api(format!(
            "Port {} already in use",
            addr.port()
        )));
    }

    // Start mDNS service discovery if enabled (requires DB for persistent UUID)
    let _mdns_service = match (&db, config.server.should_enable_mdns()) {
        (Some(db), true) => match start_mdns_service(db, config, addr.port()).await {
            Ok(service) => {
                tracing::info!("mDNS service discovery enabled on local network");
                Some(service)
            }
            Err(e) => {
                tracing::warn!("mDNS discovery unavailable: {} (continuing without it)", e);
                None
            }
        },
        _ => {
            if config.server.mdns_enabled {
                tracing::info!(
                    "mDNS disabled for localhost-only binding (set host = \"0.0.0.0\" to enable)"
                );
            }
            None
        }
    };

    tracing::info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| crate::error::CoreError::Api(e.to_string()))?;

    // _mdns_service is dropped here, which calls unregister() via Drop

    Ok(())
}

/// Middleware that rejects requests when storage != "db".
/// Used to guard routes that require SQLite (search, memories, skills, AI, etc.).
async fn require_db_storage(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if state.db.is_some() {
        next.run(request).await
    } else {
        (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "error": "This endpoint requires storage = \"db\"",
                "storage": "ephemeral"
            })),
        )
            .into_response()
    }
}

/// Create the API router with all routes
fn create_router(state: AppState) -> Router {
    // CORS configuration - allow all origins for development
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Routes that work in both db and ephemeral modes
    let common_routes = Router::new()
        // Projects (list, get, resolve work in both; create/update/delete need DB)
        .route("/projects", get(routes::list_projects))
        .route("/projects/resolve", get(routes::resolve_project))
        .route("/projects/:id", get(routes::get_project))
        // Sessions (list, get work in both; update/delete need DB)
        .route("/sessions", get(routes::list_sessions))
        .route("/sessions/:id", get(routes::get_session))
        .route("/sessions/:id/messages", get(routes::get_session_messages))
        .route(
            "/sessions/:id/messages/:seq/content",
            get(routes::get_message_content),
        )
        // Session byte streaming (reads from JSONL files)
        .route("/sessions/:id/bytes", get(routes::read_session_bytes))
        // Config API (file-based, no DB)
        .route("/config", get(config_routes::get_config))
        .route("/config", put(config_routes::update_config))
        .route("/config/ai", get(config_routes::get_ai_config))
        .route("/config/ai", put(config_routes::update_ai_config))
        .route("/config/watch", get(config_routes::list_watch_paths))
        .route("/config/watch", post(config_routes::add_watch_path))
        .route(
            "/config/watch/:index",
            delete(config_routes::remove_watch_path),
        )
        // Server-Sent Events
        .route("/events", get(sse::events_handler));

    // Routes that require storage = "db" (guarded by middleware)
    let db_only_routes = Router::new()
        // Projects (mutations)
        .route("/projects", post(routes::create_project))
        .route("/projects/:id", patch(routes::update_project))
        .route("/projects/:id", delete(routes::delete_project))
        // Project Analytics
        .route(
            "/projects/:id/analytics",
            get(routes::get_project_analytics),
        )
        // Sessions (mutations)
        .route("/sessions/:id", patch(routes::update_session))
        .route("/sessions/:id", delete(routes::delete_session))
        .route("/sessions/:id/markers", get(routes::get_session_markers))
        .route("/sessions/:id/search", get(routes::search_session))
        // Session mutations
        .route(
            "/sessions/:id/messages/append",
            post(routes::append_session_messages),
        )
        .route(
            "/sessions/:id/agent-summary",
            post(routes::update_agent_summary),
        )
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
        .route(
            "/ai/sessions/:id/title",
            post(routes::trigger_title_generation),
        )
        .route(
            "/ai/sessions/:id/memories",
            post(routes::trigger_memory_extraction),
        )
        .route(
            "/ai/sessions/:id/skills",
            post(routes::trigger_skill_extraction),
        )
        .route(
            "/ai/sessions/:id/markers",
            post(routes::trigger_marker_detection),
        )
        .route("/ai/cli/status", get(routes::get_ai_cli_status))
        .route("/ai/pending-sessions", get(routes::get_pending_ai_sessions))
        // AI Export
        .route(
            "/ai/export/capabilities",
            get(routes::get_ai_export_capabilities),
        )
        .route("/ai/export/generate", post(routes::generate_ai_export))
        .route("/ai/export/chunk", post(routes::process_ai_export_chunk))
        .route("/ai/export/merge", post(routes::merge_ai_export_chunks))
        // Session Limit
        .route("/sessions/limit", get(routes::get_session_limit_info))
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
        // Embeddings
        .route("/embeddings/backfill", post(routes::backfill_embeddings))
        // Context API (for LLM skills and hooks — requires DB)
        .route("/context/project", get(context_routes::get_project_context))
        .route(
            "/context/session",
            post(context_routes::get_session_context),
        )
        .route(
            "/context/recent-memories",
            get(context_routes::get_recent_memories),
        )
        .route("/context/lifeboat", post(context_routes::save_lifeboat))
        .route("/context/search", post(context_routes::search_context))
        // Apply DB guard middleware
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_db_storage,
        ));

    let api_routes = common_routes
        .merge(db_only_routes)
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

/// Start mDNS service announcement for LAN discovery.
async fn start_mdns_service(
    db: &Arc<Database>,
    config: &Config,
    port: u16,
) -> std::result::Result<crate::mdns::MdnsService, String> {
    // Get or create persistent instance UUID
    let uuid = db
        .with_conn(crate::db::schema::get_or_create_instance_uuid)
        .await
        .map_err(|e| format!("Failed to get instance UUID: {}", e))?;

    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    // Get project count for metadata
    let project_count: usize = db
        .with_read_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM projects", [], |row| {
                row.get::<_, usize>(0)
            })
            .unwrap_or(0)
        })
        .await;

    let instance_name = crate::mdns::generate_instance_name(
        &hostname,
        &uuid,
        config.server.instance_name.as_deref(),
    );

    let metadata = crate::mdns::MdnsMetadata {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uuid,
        hostname,
        api_key_required: config.server.api_key.is_some(),
        project_count,
        name: config.server.instance_name.clone(),
    };

    crate::mdns::MdnsService::register(&instance_name, port, metadata)
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
