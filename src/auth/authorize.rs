use std::time::Instant;

use axum::body::Body;
use axum::extract::Query;
use axum::response::Response;

use super::helpers::{error_redirect, generate_token, json_response};
use super::{AuthCode, OAuthState};

#[derive(serde::Deserialize)]
pub struct AuthorizeParams {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: String,
    state: Option<String>,
    #[allow(dead_code)]
    resource: Option<String>,
    #[allow(dead_code)]
    scope: Option<String>,
}

pub async fn authorize(
    axum::extract::State(state): axum::extract::State<OAuthState>,
    Query(params): Query<AuthorizeParams>,
) -> Response {
    if params.response_type != "code" {
        return error_redirect(
            &params.redirect_uri, "unsupported_response_type",
            "Only 'code' response type is supported", params.state.as_deref(),
        );
    }

    if params.client_id != state.client_id {
        return error_redirect(
            &params.redirect_uri, "invalid_client",
            "Unknown client_id", params.state.as_deref(),
        );
    }

    if params.code_challenge_method != "S256" {
        return error_redirect(
            &params.redirect_uri, "invalid_request",
            "Only S256 code challenge method is supported", params.state.as_deref(),
        );
    }

    // Generate and store auth code
    let code = generate_token(32);
    state.codes.lock().await.insert(
        code.clone(),
        AuthCode {
            client_id: params.client_id,
            redirect_uri: params.redirect_uri.clone(),
            code_challenge: params.code_challenge,
            created_at: Instant::now(),
        },
    );

    // Auto-approve: redirect back with code
    let mut redirect = match url::Url::parse(&params.redirect_uri) {
        Ok(u) => u,
        Err(_) => return json_response(400, serde_json::json!({"error": "invalid_redirect_uri"})),
    };
    redirect.query_pairs_mut().append_pair("code", &code);
    if let Some(ref s) = params.state {
        redirect.query_pairs_mut().append_pair("state", s);
    }

    Response::builder()
        .status(302)
        .header("Location", redirect.as_str())
        .header("Cache-Control", "no-store")
        .body(Body::empty())
        .unwrap()
}
