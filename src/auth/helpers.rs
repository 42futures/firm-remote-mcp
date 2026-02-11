use axum::body::Body;
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::Rng;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub(super) fn generate_token(len: usize) -> String {
    let bytes: Vec<u8> = (0..len).map(|_| rand::rng().random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

pub(super) fn verify_pkce_s256(code_verifier: &str, code_challenge: &str) -> bool {
    let hash = Sha256::digest(code_verifier.as_bytes());
    let computed = URL_SAFE_NO_PAD.encode(hash);
    computed.as_bytes().ct_eq(code_challenge.as_bytes()).into()
}

pub(super) fn json_response(status: u16, body: serde_json::Value) -> Response {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap()
}

pub(super) fn token_error(status: u16, error: &str, description: &str) -> Response {
    json_response(
        status,
        serde_json::json!({
            "error": error,
            "error_description": description,
        }),
    )
}

pub(super) fn error_redirect(redirect_uri: &str, error: &str, description: &str, state: Option<&str>) -> Response {
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

pub(super) fn www_authenticate(server_url: &str) -> String {
    format!(
        "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
        server_url
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_s256_valid_pair() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let hash = Sha256::digest(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(hash);
        assert!(verify_pkce_s256(verifier, &challenge));
    }

    #[test]
    fn pkce_s256_wrong_verifier() {
        let verifier = "correct-verifier";
        let hash = Sha256::digest(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(hash);
        assert!(!verify_pkce_s256("wrong-verifier", &challenge));
    }

    #[test]
    fn www_authenticate_format() {
        let result = www_authenticate("https://example.com");
        assert_eq!(
            result,
            "Bearer resource_metadata=\"https://example.com/.well-known/oauth-protected-resource\""
        );
    }
}
