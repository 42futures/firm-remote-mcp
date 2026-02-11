use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};

pub async fn register() -> impl IntoResponse {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(serde_json::json!({
            "error": "invalid_request",
            "error_description": "Dynamic client registration is not supported. Configure client credentials via environment variables."
        })),
    )
}
