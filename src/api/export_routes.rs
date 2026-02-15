//! AI Export route handlers
//!
//! Endpoints for generating AI-processed exports (Dev Notes, Blog Posts).
//! Uses fire-and-forget pattern: returns 202 immediately, delivers result via SSE.

use super::AppState;
use crate::ai::cli::CliProvider;
use crate::ai::export::{self, ExportFormat};
use crate::ai::types::AiEvent;
use crate::config::Config;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

/// Resolve the configured CLI provider from config
fn resolve_provider(state: &AppState) -> CliProvider {
    Config::from_file(&state.config_path)
        .ok()
        .and_then(|c| {
            c.ai.provider
                .as_deref()
                .and_then(CliProvider::from_config_str)
        })
        .unwrap_or(CliProvider::ClaudeCode)
}

/// Get AI export capabilities
pub async fn get_ai_export_capabilities() -> impl IntoResponse {
    Json(export::get_capabilities().await)
}

/// Generate AI export content (async — returns 202, result delivered via SSE)
pub async fn generate_ai_export(
    State(state): State<AppState>,
    Json(req): Json<export::GenerateExportRequest>,
) -> impl IntoResponse {
    let format = match ExportFormat::parse_format(&req.format) {
        Some(f) => f,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Unknown format: {}", req.format) })),
            )
                .into_response()
        }
    };

    // Raw format doesn't need AI — return immediately
    if format == ExportFormat::Raw {
        return Json(export::ExportResult {
            content: req.raw_content,
            format: req.format,
            provider: "none".to_string(),
            generation_time_ms: 0,
        })
        .into_response();
    }

    // Detect CLI
    let provider = resolve_provider(&state);
    let cli = match export::ensure_cli(provider).await {
        Ok(cli) => cli,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    // Acquire task queue permit
    let permit = match state.ai_task_queue.acquire().await {
        Ok(permit) => permit,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    let session_id = req.session_id.clone();
    let format_str = req.format.clone();
    let raw_content = req.raw_content;
    let ai_event_tx = state.ai_event_tx.clone();

    tracing::info!("Starting AI export generation ({})", format_str);

    tokio::spawn(async move {
        let _permit = permit;
        let _ = ai_event_tx.send(AiEvent::ExportStart {
            session_id: session_id.clone(),
            format: format_str.clone(),
        });

        match export::generate_export(&raw_content, format, &cli).await {
            Ok(result) => {
                let _ = ai_event_tx.send(AiEvent::ExportComplete {
                    session_id,
                    format: result.format,
                    content: result.content,
                    provider: result.provider,
                    generation_time_ms: result.generation_time_ms,
                });
            }
            Err(e) => {
                tracing::error!("Export generation failed: {}", e);
                let _ = ai_event_tx.send(AiEvent::ExportError {
                    session_id,
                    format: format_str,
                    error: e,
                });
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "started",
            "session_id": req.session_id,
            "message": "Export generation started. Listen to SSE for results."
        })),
    )
        .into_response()
}

/// Process a single chunk of content
pub async fn process_ai_export_chunk(
    State(state): State<AppState>,
    Json(req): Json<export::ChunkRequest>,
) -> impl IntoResponse {
    let provider = resolve_provider(&state);
    let cli = match export::ensure_cli(provider).await {
        Ok(cli) => cli,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    let _permit = match state.ai_task_queue.acquire().await {
        Ok(permit) => permit,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    tracing::info!(
        "Processing export chunk {}/{}",
        req.chunk_index + 1,
        req.total_chunks
    );

    match export::process_chunk(&req, &cli).await {
        Ok(result) => Json(result).into_response(),
        Err(e) => {
            tracing::error!("Chunk processing failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    }
}

/// Merge partial results from chunks
pub async fn merge_ai_export_chunks(
    State(state): State<AppState>,
    Json(req): Json<export::MergeRequest>,
) -> impl IntoResponse {
    let provider = resolve_provider(&state);
    let cli = match export::ensure_cli(provider).await {
        Ok(cli) => cli,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    let _permit = match state.ai_task_queue.acquire().await {
        Ok(permit) => permit,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    tracing::info!("Merging {} export chunks", req.partial_results.len());

    match export::merge_chunks(&req, &cli).await {
        Ok(result) => Json(result).into_response(),
        Err(e) => {
            tracing::error!("Chunk merge failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    }
}
