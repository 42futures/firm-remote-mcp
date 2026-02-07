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
    /// API key for authenticating MCP clients.
    pub api_key: String,
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
        let api_key =
            env::var("API_KEY").map_err(|_| "API_KEY environment variable is required")?;

        let branch = env::var("BRANCH").unwrap_or_else(|_| "mcp".to_string());

        let port = env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()
            .map_err(|e| format!("Invalid PORT value: {}", e))?;

        let workspace_subdir = env::var("WORKSPACE_SUBDIR").ok();

        Ok(Config {
            repo_url,
            branch,
            github_token,
            api_key,
            port,
            workspace_subdir,
        })
    }
}
