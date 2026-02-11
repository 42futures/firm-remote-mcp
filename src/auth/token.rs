use axum::response::{IntoResponse, Json, Response};
use subtle::ConstantTimeEq;

use super::helpers::{token_error, verify_pkce_s256};
use super::jwt::ACCESS_TOKEN_TTL_SECS;
use super::{AUTH_CODE_TTL, OAuthState};

#[derive(serde::Deserialize)]
pub struct TokenRequest {
    grant_type: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    #[allow(dead_code)]
    resource: Option<String>,
}

pub async fn token(
    axum::extract::State(state): axum::extract::State<OAuthState>,
    axum::Form(params): axum::Form<TokenRequest>,
) -> Response {
    // Validate client credentials (constant-time to prevent timing attacks)
    let cid = params.client_id.as_deref().unwrap_or("");
    let csecret = params.client_secret.as_deref().unwrap_or("");
    let id_match = cid.as_bytes().ct_eq(state.client_id.as_bytes());
    let secret_match = csecret.as_bytes().ct_eq(state.client_secret.as_bytes());
    if !bool::from(id_match & secret_match) {
        return token_error(401, "invalid_client", "Invalid client credentials");
    }

    match params.grant_type.as_str() {
        "authorization_code" => handle_auth_code_grant(&state, &params).await,
        "refresh_token" => handle_refresh_grant(&state, &params).await,
        _ => token_error(400, "unsupported_grant_type", "Unsupported grant type"),
    }
}

async fn handle_auth_code_grant(state: &OAuthState, params: &TokenRequest) -> Response {
    let code_str = match &params.code {
        Some(c) => c,
        None => return token_error(400, "invalid_request", "Missing code"),
    };
    let verifier = match &params.code_verifier {
        Some(v) => v,
        None => return token_error(400, "invalid_request", "Missing code_verifier"),
    };
    let redirect_uri = match &params.redirect_uri {
        Some(r) => r,
        None => return token_error(400, "invalid_request", "Missing redirect_uri"),
    };

    // Consume auth code (single-use)
    let auth_code = state.codes.lock().await.remove(code_str);
    let auth_code = match auth_code {
        Some(c) => c,
        None => return token_error(400, "invalid_grant", "Invalid or expired code"),
    };

    if auth_code.created_at.elapsed() > AUTH_CODE_TTL {
        return token_error(400, "invalid_grant", "Authorization code expired");
    }
    if redirect_uri != &auth_code.redirect_uri {
        return token_error(400, "invalid_grant", "redirect_uri mismatch");
    }
    if !verify_pkce_s256(verifier, &auth_code.code_challenge) {
        return token_error(400, "invalid_grant", "PKCE verification failed");
    }

    // Issue JWT tokens
    let access = match state.issue_access_token() {
        Ok(t) => t,
        Err(e) => return token_error(500, "server_error", &format!("Token signing failed: {}", e)),
    };
    let refresh = match state.issue_refresh_token(&auth_code.client_id) {
        Ok(t) => t,
        Err(e) => return token_error(500, "server_error", &format!("Token signing failed: {}", e)),
    };

    Json(serde_json::json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ACCESS_TOKEN_TTL_SECS,
        "refresh_token": refresh,
    }))
    .into_response()
}

async fn handle_refresh_grant(state: &OAuthState, params: &TokenRequest) -> Response {
    let refresh_str = match &params.refresh_token {
        Some(r) => r,
        None => return token_error(400, "invalid_request", "Missing refresh_token"),
    };

    // Validate refresh JWT
    let client_id = match state.validate_refresh_token(refresh_str) {
        Some(id) => id,
        None => return token_error(400, "invalid_grant", "Invalid or expired refresh token"),
    };

    // Issue new JWT tokens
    let new_access = match state.issue_access_token() {
        Ok(t) => t,
        Err(e) => return token_error(500, "server_error", &format!("Token signing failed: {}", e)),
    };
    let new_refresh = match state.issue_refresh_token(&client_id) {
        Ok(t) => t,
        Err(e) => return token_error(500, "server_error", &format!("Token signing failed: {}", e)),
    };

    Json(serde_json::json!({
        "access_token": new_access,
        "token_type": "Bearer",
        "expires_in": ACCESS_TOKEN_TTL_SECS,
        "refresh_token": new_refresh,
    }))
    .into_response()
}
