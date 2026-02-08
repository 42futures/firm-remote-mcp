use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Query, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::Rng;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

const AUTH_CODE_TTL: Duration = Duration::from_secs(60);
const ACCESS_TOKEN_TTL_SECS: u64 = 3600;
const REFRESH_TOKEN_TTL_SECS: u64 = 7 * 86400;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

// -- Data structures --

#[derive(Clone)]
struct AuthCode {
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    created_at: Instant,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Claims {
    sub: String, // "access" or "refresh"
    exp: u64,
    iat: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
}

/// Shared OAuth state.
#[derive(Clone)]
pub struct OAuthState {
    client_id: String,
    client_secret: String,
    server_url: String,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    codes: Arc<Mutex<HashMap<String, AuthCode>>>,
}

impl OAuthState {
    pub fn new(
        client_id: String,
        client_secret: String,
        server_url: String,
        jwt_signing_key: String,
    ) -> Self {
        let encoding_key = EncodingKey::from_secret(jwt_signing_key.as_bytes());
        let decoding_key = DecodingKey::from_secret(jwt_signing_key.as_bytes());
        Self {
            client_id,
            client_secret,
            server_url,
            encoding_key,
            decoding_key,
            codes: Default::default(),
        }
    }

    fn issue_access_token(&self) -> Result<String, jsonwebtoken::errors::Error> {
        let now = jsonwebtoken::get_current_timestamp();
        let claims = Claims {
            sub: "access".to_string(),
            iat: now,
            exp: now + ACCESS_TOKEN_TTL_SECS,
            client_id: None,
        };
        encode(&Header::default(), &claims, &self.encoding_key)
    }

    fn issue_refresh_token(
        &self,
        client_id: &str,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let now = jsonwebtoken::get_current_timestamp();
        let claims = Claims {
            sub: "refresh".to_string(),
            iat: now,
            exp: now + REFRESH_TOKEN_TTL_SECS,
            client_id: Some(client_id.to_string()),
        };
        encode(&Header::default(), &claims, &self.encoding_key)
    }

    fn validate_access_token(&self, token: &str) -> bool {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_required_spec_claims(&["exp", "sub", "iat"]);
        match decode::<Claims>(token, &self.decoding_key, &validation) {
            Ok(data) => data.claims.sub == "access",
            Err(_) => false,
        }
    }

    fn validate_refresh_token(&self, token: &str) -> Option<String> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_required_spec_claims(&["exp", "sub", "iat"]);
        match decode::<Claims>(token, &self.decoding_key, &validation) {
            Ok(data) if data.claims.sub == "refresh" => data.claims.client_id,
            _ => None,
        }
    }
}

// -- Helpers --

fn generate_token(len: usize) -> String {
    let bytes: Vec<u8> = (0..len).map(|_| rand::rng().random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

fn verify_pkce_s256(code_verifier: &str, code_challenge: &str) -> bool {
    let hash = Sha256::digest(code_verifier.as_bytes());
    let computed = URL_SAFE_NO_PAD.encode(hash);
    computed == code_challenge
}

fn json_response(status: u16, body: serde_json::Value) -> Response {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap()
}

fn token_error(status: u16, error: &str, description: &str) -> Response {
    json_response(
        status,
        serde_json::json!({
            "error": error,
            "error_description": description,
        }),
    )
}

fn error_redirect(redirect_uri: &str, error: &str, description: &str, state: Option<&str>) -> Response {
    let mut url = match url::Url::parse(redirect_uri) {
        Ok(u) => u,
        Err(_) => {
            return json_response(400, serde_json::json!({
                "error": "invalid_request",
                "error_description": "Invalid redirect_uri",
            }));
        }
    };
    url.query_pairs_mut()
        .append_pair("error", error)
        .append_pair("error_description", description);
    if let Some(s) = state {
        url.query_pairs_mut().append_pair("state", s);
    }
    Response::builder()
        .status(302)
        .header("Location", url.as_str())
        .header("Cache-Control", "no-store")
        .body(Body::empty())
        .unwrap()
}

fn www_authenticate(server_url: &str) -> String {
    format!(
        "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
        server_url
    )
}

// -- Discovery endpoints --

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

// -- /authorize --

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

// -- /token --

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
    // Validate client credentials
    let cid = params.client_id.as_deref().unwrap_or("");
    let csecret = params.client_secret.as_deref().unwrap_or("");
    if cid != state.client_id || csecret != state.client_secret {
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

// -- /register (dynamic client registration) --

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

// -- Bearer token middleware --

pub async fn bearer_auth_middleware(
    state: OAuthState,
    req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.len() > 7 && h[..7].eq_ignore_ascii_case("bearer ") => &h[7..],
        _ => {
            return Response::builder()
                .status(401)
                .header("WWW-Authenticate", www_authenticate(&state.server_url))
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    };

    if state.validate_access_token(token) {
        next.run(req).await
    } else {
        Response::builder()
            .status(401)
            .header("WWW-Authenticate", www_authenticate(&state.server_url))
            .body(Body::from("Unauthorized"))
            .unwrap()
    }
}

// -- Background cleanup (auth codes only — JWTs are self-expiring) --

pub fn spawn_token_cleanup(state: OAuthState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
        loop {
            interval.tick().await;
            state
                .codes
                .lock()
                .await
                .retain(|_, c| c.created_at.elapsed() < AUTH_CODE_TTL);
        }
    });
}
