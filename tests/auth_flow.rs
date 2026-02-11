#[allow(dead_code)]
mod common;

use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{auth_router, body_json, pkce_pair, test_oauth_state};

/// Helper: perform the full OAuth flow and return (access_token, refresh_token).
async fn do_full_oauth_flow(
    app: &axum::Router,
) -> (String, String) {
    let (verifier, challenge) = pkce_pair();

    // 1. Register
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/register")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"redirect_uris":["https://callback.example.com/cb"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let reg = body_json(resp).await;
    let client_id = reg["client_id"].as_str().unwrap().to_string();
    let client_secret = reg["client_secret"].as_str().unwrap().to_string();

    // 2. Authorize
    let auth_uri = format!(
        "/authorize?response_type=code&client_id={}&code_challenge={}&code_challenge_method=S256&redirect_uri=https://callback.example.com/cb",
        client_id, challenge
    );
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(&auth_uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 302);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    let redirect_url = url::Url::parse(location).unwrap();
    let code = redirect_url
        .query_pairs()
        .find(|(k, _)| k == "code")
        .unwrap()
        .1
        .to_string();

    // 3. Token exchange
    let token_body = format!(
        "grant_type=authorization_code&code={}&code_verifier={}&redirect_uri=https://callback.example.com/cb&client_id={}&client_secret={}",
        code, verifier, client_id, client_secret
    );
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/token")
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from(token_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let tokens = body_json(resp).await;
    let access_token = tokens["access_token"].as_str().unwrap().to_string();
    let refresh_token = tokens["refresh_token"].as_str().unwrap().to_string();

    (access_token, refresh_token)
}

// ── Happy path: full OAuth flow ──

#[tokio::test]
async fn full_oauth_flow_happy_path() {
    let state = test_oauth_state();
    let app = auth_router(state);
    let (access_token, _refresh_token) = do_full_oauth_flow(&app).await;

    // Use access token on protected endpoint
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/mcp/test")
                .header("Authorization", format!("Bearer {}", access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"ok");
}

// ── Token refresh flow ──

#[tokio::test]
async fn token_refresh_flow() {
    let state = test_oauth_state();
    let app = auth_router(state);
    let (_, refresh_token) = do_full_oauth_flow(&app).await;

    // Refresh
    let token_body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id=test-client-id&client_secret=test-client-secret",
        refresh_token
    );
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/token")
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from(token_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let tokens = body_json(resp).await;
    let new_access = tokens["access_token"].as_str().unwrap();

    // New access token works
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/mcp/test")
                .header("Authorization", format!("Bearer {}", new_access))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

// ── Middleware rejection ──

#[tokio::test]
async fn missing_auth_header_returns_401() {
    let state = test_oauth_state();
    let app = auth_router(state);

    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/mcp/test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    let www_auth = resp.headers().get("www-authenticate").unwrap().to_str().unwrap();
    assert!(www_auth.contains("oauth-protected-resource"));
}

#[tokio::test]
async fn invalid_token_returns_401() {
    let state = test_oauth_state();
    let app = auth_router(state);

    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/mcp/test")
                .header("Authorization", "Bearer invalid.jwt.token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

// ── Error paths ──

#[tokio::test]
async fn authorize_wrong_client_id_redirects_with_error() {
    let state = test_oauth_state();
    let app = auth_router(state);
    let (_, challenge) = pkce_pair();

    let uri = format!(
        "/authorize?response_type=code&client_id=wrong-id&code_challenge={}&code_challenge_method=S256&redirect_uri=https://callback.example.com/cb",
        challenge
    );
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri(&uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 302);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert!(location.contains("error=invalid_client"));
}

#[tokio::test]
async fn authorize_invalid_challenge_method_redirects_with_error() {
    let state = test_oauth_state();
    let app = auth_router(state);
    let (_, challenge) = pkce_pair();

    let uri = format!(
        "/authorize?response_type=code&client_id=test-client-id&code_challenge={}&code_challenge_method=plain&redirect_uri=https://callback.example.com/cb",
        challenge
    );
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri(&uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 302);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert!(location.contains("error=invalid_request"));
}

#[tokio::test]
async fn token_invalid_code_returns_400() {
    let state = test_oauth_state();
    let app = auth_router(state);

    let body = "grant_type=authorization_code&code=bogus&code_verifier=abc&redirect_uri=https://callback.example.com/cb&client_id=test-client-id&client_secret=test-client-secret";
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/token")
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let json = body_json(resp).await;
    assert_eq!(json["error"], "invalid_grant");
}

#[tokio::test]
async fn token_pkce_mismatch_returns_400() {
    let state = test_oauth_state();
    let app = auth_router(state);
    let (_, challenge) = pkce_pair();

    // Get a valid auth code
    let auth_uri = format!(
        "/authorize?response_type=code&client_id=test-client-id&code_challenge={}&code_challenge_method=S256&redirect_uri=https://callback.example.com/cb",
        challenge
    );
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(&auth_uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    let redirect_url = url::Url::parse(location).unwrap();
    let code = redirect_url
        .query_pairs()
        .find(|(k, _)| k == "code")
        .unwrap()
        .1
        .to_string();

    // Use wrong verifier
    let body = format!(
        "grant_type=authorization_code&code={}&code_verifier=wrong-verifier&redirect_uri=https://callback.example.com/cb&client_id=test-client-id&client_secret=test-client-secret",
        code
    );
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/token")
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let json = body_json(resp).await;
    assert_eq!(json["error"], "invalid_grant");
}

#[tokio::test]
async fn token_wrong_client_credentials_returns_401() {
    let state = test_oauth_state();
    let app = auth_router(state);

    let body = "grant_type=authorization_code&code=any&code_verifier=any&redirect_uri=https://x.com&client_id=wrong&client_secret=wrong";
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/token")
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    let json = body_json(resp).await;
    assert_eq!(json["error"], "invalid_client");
}

// ── Discovery endpoints ──

#[tokio::test]
async fn protected_resource_metadata_returns_correct_json() {
    let state = test_oauth_state();
    let app = auth_router(state);

    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/.well-known/oauth-protected-resource")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json = body_json(resp).await;
    assert_eq!(json["resource"], "https://test.example.com");
    assert_eq!(json["authorization_servers"][0], "https://test.example.com");
}

#[tokio::test]
async fn authorization_server_metadata_returns_correct_json() {
    let state = test_oauth_state();
    let app = auth_router(state);

    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/.well-known/oauth-authorization-server")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json = body_json(resp).await;
    assert_eq!(json["issuer"], "https://test.example.com");
    assert_eq!(json["authorization_endpoint"], "https://test.example.com/authorize");
    assert_eq!(json["token_endpoint"], "https://test.example.com/token");
    assert_eq!(json["registration_endpoint"], "https://test.example.com/register");
    assert_eq!(json["response_types_supported"][0], "code");
    assert_eq!(json["grant_types_supported"][0], "authorization_code");
    assert_eq!(json["grant_types_supported"][1], "refresh_token");
    assert_eq!(json["code_challenge_methods_supported"][0], "S256");
}
