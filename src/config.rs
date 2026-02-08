use std::env;

/// Server configuration parsed from environment variables.
#[derive(Clone)]
pub struct Config {
    /// Git repo URL (HTTPS).
    pub repo_url: String,
    /// MCP working branch name.
    pub branch: String,
    /// GitHub PAT for git push authentication.
    pub github_token: String,
    /// OAuth client ID (entered in Claude connector UI).
    pub oauth_client_id: String,
    /// OAuth client secret (entered in Claude connector UI).
    pub oauth_client_secret: String,
    /// Secret key for signing JWT access/refresh tokens.
    pub jwt_signing_key: String,
    /// Canonical externally-reachable server URL (e.g. "https://firm-mcp-xyz.a.run.app").
    /// Used in OAuth metadata and resource indicator validation.
    pub server_url: String,
    /// HTTP listen port.
    pub port: u16,
    /// Optional subdirectory within the repo containing the Firm workspace.
    pub workspace_subdir: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let repo_url =
            env::var("REPO_URL").map_err(|_| "REPO_URL environment variable is required")?;
        let github_token = env::var("GITHUB_TOKEN")
            .map_err(|_| "GITHUB_TOKEN environment variable is required")?;
        let oauth_client_id = env::var("OAUTH_CLIENT_ID")
            .map_err(|_| "OAUTH_CLIENT_ID environment variable is required")?;
        let oauth_client_secret = env::var("OAUTH_CLIENT_SECRET")
            .map_err(|_| "OAUTH_CLIENT_SECRET environment variable is required")?;
        let jwt_signing_key = env::var("JWT_SIGNING_KEY")
            .map_err(|_| "JWT_SIGNING_KEY environment variable is required")?;
        let server_url =
            env::var("SERVER_URL").map_err(|_| "SERVER_URL environment variable is required")?;

        let branch = env::var("BRANCH").unwrap_or_else(|_| "mcp".to_string());

        let port = env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()
            .map_err(|e| format!("Invalid PORT value: {}", e))?;

        let workspace_subdir = env::var("WORKSPACE_SUBDIR").ok();

        // Strip trailing slash from server_url for consistency
        let server_url = server_url.trim_end_matches('/').to_string();

        Ok(Config {
            repo_url,
            branch,
            github_token,
            oauth_client_id,
            oauth_client_secret,
            jwt_signing_key,
            server_url,
            port,
            workspace_subdir,
        })
    }
}
