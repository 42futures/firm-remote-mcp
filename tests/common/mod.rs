use axum::body::Body;
use axum::response::Response;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use http_body_util::BodyExt;
use sha2::{Digest, Sha256};

use firm_remote_mcp::auth;
use firm_remote_mcp::auth::OAuthState;

// ── Auth helpers ──

pub fn test_oauth_state() -> OAuthState {
    OAuthState::new(
        "test-client-id".into(),
        "test-client-secret".into(),
        "https://test.example.com".into(),
        "test-jwt-signing-key-for-tests".into(),
        vec!["https://callback.example.com/cb".into()],
    )
}

pub fn auth_router(state: OAuthState) -> axum::Router {
    let oauth = state.clone();
    let protected = axum::Router::new()
        .route(
            "/mcp/test",
            axum::routing::get(|| async { "ok" }),
        )
        .layer(axum::middleware::from_fn(move |req, next| {
            let state = oauth.clone();
            auth::bearer_auth_middleware(state, req, next)
        }));

    let public = axum::Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            axum::routing::get(auth::protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            axum::routing::get(auth::authorization_server_metadata),
        )
        .route("/authorize", axum::routing::get(auth::authorize))
        .route("/token", axum::routing::post(auth::token))
        .route("/register", axum::routing::post(auth::register))
        .with_state(state);

    public.merge(protected)
}

pub fn pkce_pair() -> (String, String) {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let hash = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hash);
    (verifier.to_string(), challenge)
}

pub async fn body_json(response: Response<Body>) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ── Git helpers ──

use std::path::Path;

use firm_remote_mcp::git::GitConfig;
use git2::{Repository, Signature};
use tempfile::TempDir;

/// Create a bare repo with an initial commit on `main`.
pub fn create_bare_origin() -> (TempDir, Repository) {
    let dir = TempDir::new().unwrap();
    let repo = Repository::init_bare(dir.path()).unwrap();

    {
        let sig = Signature::now("Test", "test@test.com").unwrap();
        let blob_oid = repo.blob(b"# Test repo\n").unwrap();
        let mut tb = repo.treebuilder(None).unwrap();
        tb.insert("README.md", blob_oid, 0o100644).unwrap();
        let tree_oid = tb.write().unwrap();
        drop(tb);
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(
            Some("refs/heads/main"),
            &sig,
            &sig,
            "Initial commit",
            &tree,
            &[],
        )
        .unwrap();
    }

    repo.set_head("refs/heads/main").unwrap();

    (dir, repo)
}

/// Build a GitConfig pointing at a temp working directory with a file:// remote.
pub fn make_git_config(origin_path: &Path, branch: &str) -> (TempDir, GitConfig) {
    let work_dir = TempDir::new().unwrap();
    let config = GitConfig {
        repo_path: work_dir.path().to_path_buf(),
        remote_url: format!("file://{}", origin_path.display()),
        branch: branch.to_string(),
        token: String::new(),
    };
    (work_dir, config)
}

/// Add a commit to a bare repo on the given branch.
pub fn add_commit_to_bare(repo: &Repository, branch: &str, filename: &str, content: &str) {
    let parent = repo
        .find_reference(&format!("refs/heads/{}", branch))
        .unwrap()
        .peel_to_commit()
        .unwrap();

    let sig = Signature::now("Test", "test@test.com").unwrap();

    // Build a new tree with the file added
    let mut tree_builder = repo.treebuilder(Some(&parent.tree().unwrap())).unwrap();
    let blob_oid = repo.blob(content.as_bytes()).unwrap();
    tree_builder
        .insert(filename, blob_oid, 0o100644)
        .unwrap();
    let tree_oid = tree_builder.write().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();

    repo.commit(
        Some(&format!("refs/heads/{}", branch)),
        &sig,
        &sig,
        &format!("Add {}", filename),
        &tree,
        &[&parent],
    )
    .unwrap();
}
