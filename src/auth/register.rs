use axum::response::Json;

use super::OAuthState;

pub async fn register(
    axum::extract::State(state): axum::extract::State<OAuthState>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let redirect_uris = body
        .get("redirect_uris")
        .cloned()
        .unwrap_or(serde_json::json!([]));

    Json(serde_json::json!({
        "client_id": state.client_id,
        "client_secret": state.client_secret,
        "client_id_issued_at": 0,
        "client_secret_expires_at": 0,
        "redirect_uris": redirect_uris,
        "token_endpoint_auth_method": "client_secret_post",
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
    }))
}
