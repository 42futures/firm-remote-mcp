use axum::response::Json;

use super::OAuthState;

pub async fn protected_resource_metadata(
    state: axum::extract::State<OAuthState>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "resource": state.server_url,
        "authorization_servers": [state.server_url],
        "scopes_supported": [],
        "bearer_methods_supported": ["header"],
    }))
}

pub async fn authorization_server_metadata(
    state: axum::extract::State<OAuthState>,
) -> Json<serde_json::Value> {
    let issuer = &state.server_url;
    Json(serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{}/authorize", issuer),
        "token_endpoint": format!("{}/token", issuer),
        "registration_endpoint": format!("{}/register", issuer),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["client_secret_post"],
        "code_challenge_methods_supported": ["S256"],
        "scopes_supported": [],
    }))
}
