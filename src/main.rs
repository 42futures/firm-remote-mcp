mod config;
mod git;
mod server;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use firm_mcp::FirmMcpServer;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
    session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use crate::git::GitConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // 1. Parse config
    let config = config::Config::from_env()?;
    log::info!(
        "Starting Firm Remote MCP server (branch: {}, port: {})",
        config.branch,
        config.port
    );

    // 2. Set up git config
    let repo_path = PathBuf::from("/tmp/workspace-repo");
    let git_config = GitConfig {
        repo_path: repo_path.clone(),
        remote_url: config.repo_url.clone(),
        branch: config.branch.clone(),
        token: config.github_token.clone(),
    };

    // 3. Clone or fetch the repo
    log::info!("Cloning/fetching repository...");
    git::clone_or_fetch(git_config.clone()).await?;

    // 4. Checkout the mcp branch (create from origin/main if needed)
    log::info!("Checking out branch '{}'...", config.branch);
    git::checkout_mcp_branch(git_config.clone()).await?;

    // 5. Merge origin/main into the mcp branch
    log::info!("Merging origin/main...");
    git::merge_main(git_config.clone()).await?;

    // 6. Determine workspace path
    let workspace_path = match &config.workspace_subdir {
        Some(subdir) => repo_path.join(subdir),
        None => repo_path.clone(),
    };

    // 7. Create FirmMcpServer
    log::info!("Loading workspace from {:?}...", workspace_path);
    let firm_server =
        FirmMcpServer::new(workspace_path).map_err(|e| format!("Workspace error: {:?}", e))?;
    log::info!("Workspace loaded successfully");

    // 8. Create RemoteFirmServer
    let remote_server = server::RemoteFirmServer::new(firm_server, git_config);

    // 9. Set up StreamableHttpService
    let ct = CancellationToken::new();
    let mcp_config = StreamableHttpServerConfig {
        stateful_mode: true,
        sse_keep_alive: Some(Duration::from_secs(15)),
        cancellation_token: ct.clone(),
        ..Default::default()
    };

    let server_clone = remote_server.clone();
    let mcp_service = StreamableHttpService::new(
        move || Ok(server_clone.clone()),
        Arc::new(LocalSessionManager::default()),
        mcp_config,
    );

    // 10. Set up Axum router with auth middleware
    let api_key = config.api_key.clone();
    let app = axum::Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(axum::middleware::from_fn(move |req, next| {
            let key = api_key.clone();
            auth_middleware(key, req, next)
        }));

    // 11. Start server
    let addr = format!("0.0.0.0:{}", config.port);
    log::info!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(ct))
        .await?;

    log::info!("Server shut down");
    Ok(())
}

async fn auth_middleware(expected_key: String, req: Request, next: Next) -> Response {
    let api_key = req
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok());

    match api_key {
        Some(key) if key == expected_key => next.run(req).await,
        _ => Response::builder()
            .status(401)
            .body(Body::from("Unauthorized"))
            .unwrap(),
    }
}

async fn shutdown_signal(ct: CancellationToken) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => log::info!("Received Ctrl+C"),
        _ = terminate => log::info!("Received SIGTERM"),
    }

    ct.cancel();
}
