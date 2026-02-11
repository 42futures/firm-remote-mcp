mod authorize;
mod discovery;
mod helpers;
mod jwt;
mod middleware;
mod register;
mod token;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{DecodingKey, EncodingKey};
use tokio::sync::Mutex;

pub use authorize::authorize;
pub use discovery::{authorization_server_metadata, protected_resource_metadata};
pub use middleware::bearer_auth_middleware;
pub use register::register;
pub use token::token;

pub(crate) const AUTH_CODE_TTL: Duration = Duration::from_secs(60);
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct AuthCode {
    pub(crate) client_id: String,
    pub(crate) redirect_uri: String,
    pub(crate) code_challenge: String,
    pub(crate) created_at: Instant,
}

/// Shared OAuth state.
#[derive(Clone)]
pub struct OAuthState {
    pub(crate) client_id: String,
    pub(crate) client_secret: String,
    pub(crate) server_url: String,
    pub(crate) allowed_redirect_uris: Vec<String>,
    pub(crate) encoding_key: EncodingKey,
    pub(crate) decoding_key: DecodingKey,
    pub(crate) codes: Arc<Mutex<HashMap<String, AuthCode>>>,
}

impl OAuthState {
    pub fn new(
        client_id: String,
        client_secret: String,
        server_url: String,
        jwt_signing_key: String,
        allowed_redirect_uris: Vec<String>,
    ) -> Self {
        let encoding_key = EncodingKey::from_secret(jwt_signing_key.as_bytes());
        let decoding_key = DecodingKey::from_secret(jwt_signing_key.as_bytes());
        Self {
            client_id,
            client_secret,
            server_url,
            allowed_redirect_uris,
            encoding_key,
            decoding_key,
            codes: Default::default(),
        }
    }
}

/// Background cleanup (auth codes only -- JWTs are self-expiring).
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
