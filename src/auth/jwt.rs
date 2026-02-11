use jsonwebtoken::{Algorithm, Header, Validation, decode, encode};

use super::OAuthState;

pub(crate) const ACCESS_TOKEN_TTL_SECS: u64 = 3600;
const REFRESH_TOKEN_TTL_SECS: u64 = 7 * 86400;

#[derive(serde::Serialize, serde::Deserialize)]
struct Claims {
    sub: String, // "access" or "refresh"
    exp: u64,
    iat: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
}

impl OAuthState {
    pub(crate) fn issue_access_token(&self) -> Result<String, jsonwebtoken::errors::Error> {
        let now = jsonwebtoken::get_current_timestamp();
        let claims = Claims {
            sub: "access".to_string(),
            iat: now,
            exp: now + ACCESS_TOKEN_TTL_SECS,
            client_id: None,
        };
        encode(&Header::default(), &claims, &self.encoding_key)
    }

    pub(crate) fn issue_refresh_token(
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

    pub(crate) fn validate_access_token(&self, token: &str) -> bool {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_required_spec_claims(&["exp", "sub", "iat"]);
        match decode::<Claims>(token, &self.decoding_key, &validation) {
            Ok(data) => data.claims.sub == "access",
            Err(_) => false,
        }
    }

    pub(crate) fn validate_refresh_token(&self, token: &str) -> Option<String> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_required_spec_claims(&["exp", "sub", "iat"]);
        match decode::<Claims>(token, &self.decoding_key, &validation) {
            Ok(data) if data.claims.sub == "refresh" => data.claims.client_id,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(key: &str) -> OAuthState {
        OAuthState::new(
            "test-client".into(),
            "test-secret".into(),
            "https://example.com".into(),
            key.into(),
            vec![],
        )
    }

    #[test]
    fn access_token_validates_as_access() {
        let state = test_state("test-key-1");
        let token = state.issue_access_token().unwrap();
        assert!(state.validate_access_token(&token));
    }

    #[test]
    fn refresh_token_validates_as_refresh() {
        let state = test_state("test-key-2");
        let token = state.issue_refresh_token("my-client").unwrap();
        let client_id = state.validate_refresh_token(&token);
        assert_eq!(client_id, Some("my-client".to_string()));
    }

    #[test]
    fn access_token_fails_refresh_validation() {
        let state = test_state("test-key-3");
        let token = state.issue_access_token().unwrap();
        assert!(state.validate_refresh_token(&token).is_none());
    }

    #[test]
    fn refresh_token_fails_access_validation() {
        let state = test_state("test-key-4");
        let token = state.issue_refresh_token("c").unwrap();
        assert!(!state.validate_access_token(&token));
    }

    #[test]
    fn tampered_token_fails_validation() {
        let state = test_state("test-key-5");
        let mut token = state.issue_access_token().unwrap();
        // Flip a character in the signature portion
        let last = token.pop().unwrap();
        token.push(if last == 'A' { 'B' } else { 'A' });
        assert!(!state.validate_access_token(&token));
    }

    #[test]
    fn different_key_fails_validation() {
        let state_a = test_state("key-alpha");
        let state_b = test_state("key-beta");
        let token = state_a.issue_access_token().unwrap();
        assert!(!state_b.validate_access_token(&token));
    }
}
