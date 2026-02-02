//! Authentication middleware for API key validation

use super::AppState;
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

/// Authentication middleware
///
/// If `api_key` is configured in AppState, validates the Authorization header.
/// Expected format: `Authorization: Bearer <api_key>`
///
/// If no `api_key` is configured, all requests are allowed (local mode).
pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    // If no API key is configured, allow all requests
    let Some(expected_key) = &state.api_key else {
        return next.run(request).await;
    };

    // Check Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let provided_key = &header[7..]; // Skip "Bearer "

            if provided_key == expected_key {
                // Valid API key, proceed with request
                next.run(request).await
            } else {
                // Invalid API key
                (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "error": "Invalid API key"
                    })),
                )
                    .into_response()
            }
        }
        Some(_) => {
            // Authorization header exists but wrong format
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "Invalid Authorization header format. Expected: Bearer <api_key>"
                })),
            )
                .into_response()
        }
        None => {
            // No Authorization header
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "API key required. Set Authorization: Bearer <api_key>"
                })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bearer_token_extraction() {
        let header = "Bearer my-secret-key";
        assert!(header.starts_with("Bearer "));
        let key = &header[7..];
        assert_eq!(key, "my-secret-key");
    }
}
