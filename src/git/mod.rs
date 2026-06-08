mod branch;
mod clone;
mod commit;

use std::path::PathBuf;

use git2::{Cred, FetchOptions, PushOptions, RemoteCallbacks};

pub use branch::{checkout_mcp_branch, merge_main, sync_branch};
pub use clone::clone_or_fetch;
pub use commit::commit_and_push;

#[derive(Debug)]
pub enum GitError {
    Git2(git2::Error),
    MergeConflicts,
    NothingToCommit,
    Join(tokio::task::JoinError),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Git2(e) => write!(f, "git error: {}", e),
            GitError::MergeConflicts => write!(
                f,
                "merge conflicts between origin/main and mcp branch (resolve manually)"
            ),
            GitError::NothingToCommit => write!(f, "nothing to commit"),
            GitError::Join(e) => write!(f, "task join error: {}", e),
        }
    }
}

impl std::error::Error for GitError {}

impl From<git2::Error> for GitError {
    fn from(e: git2::Error) -> Self {
        GitError::Git2(e)
    }
}

/// Configuration for git operations. Clone-friendly for use in spawn_blocking.
#[derive(Clone)]
pub struct GitConfig {
    pub repo_path: PathBuf,
    pub remote_url: String,
    pub branch: String,
    pub token: String,
}

pub(crate) fn fetch_options(token: &str) -> FetchOptions<'_> {
    let mut callbacks = RemoteCallbacks::new();
    let token = token.to_string();
    callbacks.credentials(move |_url, _username, _allowed| {
        Cred::userpass_plaintext("oauth2", &token)
    });
    let mut opts = FetchOptions::new();
    opts.remote_callbacks(callbacks);
    opts
}

pub(crate) fn push_options(token: &str) -> PushOptions<'_> {
    let mut callbacks = RemoteCallbacks::new();
    let token = token.to_string();
    callbacks.credentials(move |_url, _username, _allowed| {
        Cred::userpass_plaintext("oauth2", &token)
    });
    let mut opts = PushOptions::new();
    opts.remote_callbacks(callbacks);
    opts
}
