//! Configuration API routes
//!
//! Provides endpoints for reading and modifying the Yocore configuration.
//! Changes are persisted to config.toml.

use super::AppState;
use crate::config::{AiConfig, Config, WatchConfig};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// Response types
// ============================================================================

/// Full configuration response
#[derive(Serialize)]
pub struct ConfigResponse {
    /// Server configuration (may have env overrides)
    pub server: ServerConfigResponse,
    /// Watch paths
    pub watch: Vec<WatchConfigResponse>,
    /// AI configuration
    pub ai: AiConfigResponse,
    /// Data directory
    pub data_dir: String,
    /// Configuration metadata
    pub meta: ConfigMeta,
}

#[derive(Serialize)]
pub struct ServerConfigResponse {
    pub host: String,
    pub port: u16,
    pub has_api_key: bool,
}

#[derive(Serialize)]
pub struct WatchConfigResponse {
    pub index: usize,
    pub path: String,
    pub parser: String,
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct AiConfigResponse {
    pub provider: Option<String>,
    pub title_generation: bool,
    pub marker_detection: bool,
    pub memory_extraction: bool,
    pub skills_discovery: bool,
}

#[derive(Serialize)]
pub struct ConfigMeta {
    /// Path to the config file
    pub file_path: String,
    /// Whether config is read-only (YOLOG_CONFIG_READONLY=true)
    pub readonly: bool,
    /// List of active environment variable overrides
    pub env_overrides: Vec<String>,
}

// ============================================================================
// Request types
// ============================================================================

#[derive(Deserialize)]
pub struct UpdateAiConfigRequest {
    pub provider: Option<String>,
    pub title_generation: Option<bool>,
    pub marker_detection: Option<bool>,
    pub memory_extraction: Option<bool>,
    pub skills_discovery: Option<bool>,
}

#[derive(Deserialize)]
pub struct AddWatchPathRequest {
    pub path: String,
    pub parser: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateConfigRequest {
    pub ai: Option<UpdateAiConfigRequest>,
    // Note: server config is not updatable via API (requires restart)
    // Note: watch paths use dedicated endpoints
}

// ============================================================================
// Route handlers
// ============================================================================

/// GET /api/config - Get the full effective configuration
pub async fn get_config(State(state): State<AppState>) -> impl IntoResponse {
    match Config::from_file(&state.config_path) {
        Ok(mut config) => {
            // Apply env overrides to show effective values
            config.apply_env_overrides();

            let response = ConfigResponse {
                server: ServerConfigResponse {
                    host: config.server.host.clone(),
                    port: config.server.port,
                    has_api_key: config.server.api_key.is_some(),
                },
                watch: config
                    .watch
                    .iter()
                    .enumerate()
                    .map(|(i, w)| WatchConfigResponse {
                        index: i,
                        path: w.path.to_string_lossy().to_string(),
                        parser: w.parser.clone(),
                        enabled: w.enabled,
                    })
                    .collect(),
                ai: AiConfigResponse {
                    provider: config.ai.provider.clone(),
                    title_generation: config.ai.title_generation,
                    marker_detection: config.ai.marker_detection,
                    memory_extraction: config.ai.memory_extraction,
                    skills_discovery: config.ai.skills_discovery,
                },
                data_dir: config.data_dir().to_string_lossy().to_string(),
                meta: ConfigMeta {
                    file_path: state.config_path.to_string_lossy().to_string(),
                    readonly: Config::is_readonly(),
                    env_overrides: Config::active_env_overrides(),
                },
            };

            Json(response).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// PUT /api/config - Update configuration (currently only AI settings)
pub async fn update_config(
    State(state): State<AppState>,
    Json(req): Json<UpdateConfigRequest>,
) -> impl IntoResponse {
    // Check readonly mode
    if Config::is_readonly() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "Configuration is read-only (YOLOG_CONFIG_READONLY=true)"
            })),
        )
            .into_response();
    }

    // Load current config
    let mut config = match Config::from_file(&state.config_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    // Apply AI config updates if provided
    if let Some(ai_update) = req.ai {
        apply_ai_update(&mut config.ai, ai_update);
    }

    // Save config
    match config.save_to_file(&state.config_path) {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/config/ai - Get AI configuration
pub async fn get_ai_config(State(state): State<AppState>) -> impl IntoResponse {
    match Config::from_file(&state.config_path) {
        Ok(config) => {
            let response = AiConfigResponse {
                provider: config.ai.provider.clone(),
                title_generation: config.ai.title_generation,
                marker_detection: config.ai.marker_detection,
                memory_extraction: config.ai.memory_extraction,
                skills_discovery: config.ai.skills_discovery,
            };
            Json(response).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// PUT /api/config/ai - Update AI configuration
pub async fn update_ai_config(
    State(state): State<AppState>,
    Json(req): Json<UpdateAiConfigRequest>,
) -> impl IntoResponse {
    // Check readonly mode
    if Config::is_readonly() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "Configuration is read-only (YOLOG_CONFIG_READONLY=true)"
            })),
        )
            .into_response();
    }

    // Load current config
    let mut config = match Config::from_file(&state.config_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    // Apply updates
    apply_ai_update(&mut config.ai, req);

    // Save config
    match config.save_to_file(&state.config_path) {
        Ok(()) => {
            let response = AiConfigResponse {
                provider: config.ai.provider.clone(),
                title_generation: config.ai.title_generation,
                marker_detection: config.ai.marker_detection,
                memory_extraction: config.ai.memory_extraction,
                skills_discovery: config.ai.skills_discovery,
            };
            Json(response).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/config/watch - List watch paths
pub async fn list_watch_paths(State(state): State<AppState>) -> impl IntoResponse {
    match Config::from_file(&state.config_path) {
        Ok(config) => {
            let watch_paths: Vec<WatchConfigResponse> = config
                .watch
                .iter()
                .enumerate()
                .map(|(i, w)| WatchConfigResponse {
                    index: i,
                    path: w.path.to_string_lossy().to_string(),
                    parser: w.parser.clone(),
                    enabled: w.enabled,
                })
                .collect();

            Json(serde_json::json!({ "watch_paths": watch_paths })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/config/watch - Add a watch path
pub async fn add_watch_path(
    State(state): State<AppState>,
    Json(req): Json<AddWatchPathRequest>,
) -> impl IntoResponse {
    // Check readonly mode
    if Config::is_readonly() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "Configuration is read-only (YOLOG_CONFIG_READONLY=true)"
            })),
        )
            .into_response();
    }

    // Load current config
    let mut config = match Config::from_file(&state.config_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    let new_path = PathBuf::from(&req.path);

    // Check if path already exists
    if config.watch.iter().any(|w| w.path == new_path) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "Watch path already exists" })),
        )
            .into_response();
    }

    // Add new watch path
    config.watch.push(WatchConfig {
        path: new_path,
        parser: req.parser.unwrap_or_else(|| "claude_code".to_string()),
        enabled: req.enabled.unwrap_or(true),
    });

    // Save config
    match config.save_to_file(&state.config_path) {
        Ok(()) => {
            let watch_paths: Vec<WatchConfigResponse> = config
                .watch
                .iter()
                .enumerate()
                .map(|(i, w)| WatchConfigResponse {
                    index: i,
                    path: w.path.to_string_lossy().to_string(),
                    parser: w.parser.clone(),
                    enabled: w.enabled,
                })
                .collect();

            (
                StatusCode::CREATED,
                Json(serde_json::json!({ "watch_paths": watch_paths })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/config/watch/:index - Remove a watch path by index
pub async fn remove_watch_path(
    State(state): State<AppState>,
    Path(index): Path<usize>,
) -> impl IntoResponse {
    // Check readonly mode
    if Config::is_readonly() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "Configuration is read-only (YOLOG_CONFIG_READONLY=true)"
            })),
        )
            .into_response();
    }

    // Load current config
    let mut config = match Config::from_file(&state.config_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    // Check if index is valid
    if index >= config.watch.len() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Watch path index out of range" })),
        )
            .into_response();
    }

    // Remove the watch path
    config.watch.remove(index);

    // Save config
    match config.save_to_file(&state.config_path) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ============================================================================
// Helper functions
// ============================================================================

fn apply_ai_update(ai: &mut AiConfig, update: UpdateAiConfigRequest) {
    if let Some(provider) = update.provider {
        ai.provider = if provider.is_empty() {
            None
        } else {
            Some(provider)
        };
    }
    if let Some(title_generation) = update.title_generation {
        ai.title_generation = title_generation;
    }
    if let Some(marker_detection) = update.marker_detection {
        ai.marker_detection = marker_detection;
    }
    if let Some(memory_extraction) = update.memory_extraction {
        ai.memory_extraction = memory_extraction;
    }
    if let Some(skills_discovery) = update.skills_discovery {
        ai.skills_discovery = skills_discovery;
    }
}
