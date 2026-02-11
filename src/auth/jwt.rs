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
