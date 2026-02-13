//! AI Export route handlers
//!
//! Endpoints for generating AI-processed exports (Dev Notes, Blog Posts).
//! Delegates to `ai::export` for prompt construction and CLI invocation.

use super::AppState;
use crate::ai::export::{self, ExportFormat};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

/// Get AI export capabilities
pub async fn get_ai_export_capabilities() -> impl IntoResponse {
    Json(export::get_capabilities().await)
}

/// Generate AI export content
pub async fn generate_ai_export(
    State(state): State<AppState>,
    Json(req): Json<export::GenerateExportRequest>,
) -> impl IntoResponse {
    let format = match ExportFormat::from_str(&req.format) {
        Some(f) => f,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Unknown format: {}", req.format) })),
            )
                .into_response()
        }
    };

    // Raw format doesn't need AI
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
    let cli = match export::ensure_cli().await {
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

    tracing::info!("Starting AI export generation ({})", req.format);

    match export::generate_export(&req.raw_content, format, &cli).await {
        Ok(result) => Json(result).into_response(),
        Err(e) => {
            tracing::error!("Export generation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    }
}

/// Process a single chunk of content
pub async fn process_ai_export_chunk(
    State(state): State<AppState>,
    Json(req): Json<export::ChunkRequest>,
) -> impl IntoResponse {
    // Detect CLI
    let cli = match export::ensure_cli().await {
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
    // Detect CLI
    let cli = match export::ensure_cli().await {
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
