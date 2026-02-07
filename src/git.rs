use std::path::PathBuf;

use git2::{
    BranchType, Cred, FetchOptions, IndexAddOption, MergeAnalysis, PushOptions,
    RemoteCallbacks, Repository, Signature,
};

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

fn fetch_options(token: &str) -> FetchOptions<'_> {
    let mut callbacks = RemoteCallbacks::new();
    let token = token.to_string();
    callbacks.credentials(move |_url, _username, _allowed| {
        Cred::userpass_plaintext("x-access-token", &token)
    });
    let mut opts = FetchOptions::new();
    opts.remote_callbacks(callbacks);
    opts
}

fn push_options(token: &str) -> PushOptions<'_> {
    let mut callbacks = RemoteCallbacks::new();
    let token = token.to_string();
    callbacks.credentials(move |_url, _username, _allowed| {
        Cred::userpass_plaintext("x-access-token", &token)
    });
    let mut opts = PushOptions::new();
    opts.remote_callbacks(callbacks);
    opts
}

/// Clone the repo if it doesn't exist locally, otherwise fetch from origin.
pub async fn clone_or_fetch(config: GitConfig) -> Result<(), GitError> {
    tokio::task::spawn_blocking(move || clone_or_fetch_sync(&config))
        .await
        .map_err(GitError::Join)?
}

fn clone_or_fetch_sync(config: &GitConfig) -> Result<(), GitError> {
    if config.repo_path.join(".git").exists() {
        log::info!("Repository exists, fetching...");
        let repo = Repository::open(&config.repo_path)?;
        let mut remote = repo.find_remote("origin")?;
        let mut opts = fetch_options(&config.token);
        remote.fetch(
            &["refs/heads/*:refs/remotes/origin/*"],
            Some(&mut opts),
            None,
        )?;
    } else {
        log::info!("Cloning repository to {:?}...", config.repo_path);
        let opts = fetch_options(&config.token);
        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(opts);
        builder.clone(&config.remote_url, &config.repo_path)?;
    }
    Ok(())
}

/// Checkout the mcp branch, creating it from origin/main if it doesn't exist.
pub async fn checkout_mcp_branch(config: GitConfig) -> Result<(), GitError> {
    tokio::task::spawn_blocking(move || checkout_mcp_branch_sync(&config))
        .await
        .map_err(GitError::Join)?
}

fn checkout_mcp_branch_sync(config: &GitConfig) -> Result<(), GitError> {
    let repo = Repository::open(&config.repo_path)?;

    // Check if local branch exists
    let branch_exists = repo
        .find_branch(&config.branch, BranchType::Local)
        .is_ok();

    if !branch_exists {
        log::info!(
            "Creating branch '{}' from origin/main...",
            config.branch
        );
        // Find origin/main commit
        let origin_main = repo.find_reference("refs/remotes/origin/main")?;
        let commit = origin_main.peel_to_commit()?;
        repo.branch(&config.branch, &commit, false)?;
    }

    // Checkout the branch
    let refname = format!("refs/heads/{}", config.branch);
    repo.set_head(&refname)?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
    log::info!("Checked out branch '{}'", config.branch);
    Ok(())
}

/// Merge origin/main into the current (mcp) branch. Errors on conflicts.
pub async fn merge_main(config: GitConfig) -> Result<(), GitError> {
    tokio::task::spawn_blocking(move || merge_main_sync(&config))
        .await
        .map_err(GitError::Join)?
}

fn merge_main_sync(config: &GitConfig) -> Result<(), GitError> {
    let repo = Repository::open(&config.repo_path)?;

    // Find origin/main
    let origin_ref = match repo.find_reference("refs/remotes/origin/main") {
        Ok(r) => r,
        Err(_) => {
            log::info!("No origin/main found, skipping merge");
            return Ok(());
        }
    };
    let annotated = repo.reference_to_annotated_commit(&origin_ref)?;

    let (analysis, _) = repo.merge_analysis(&[&annotated])?;

    if analysis.contains(MergeAnalysis::ANALYSIS_UP_TO_DATE) {
        log::info!("Already up-to-date with origin/main");
        return Ok(());
    }

    if analysis.contains(MergeAnalysis::ANALYSIS_FASTFORWARD) {
        log::info!("Fast-forwarding to origin/main...");
        let refname = format!("refs/heads/{}", config.branch);
        let mut reference = repo.find_reference(&refname)?;
        reference.set_target(annotated.id(), "Fast-forward merge of origin/main")?;
        repo.set_head(&refname)?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
        return Ok(());
    }

    if analysis.contains(MergeAnalysis::ANALYSIS_NORMAL) {
        log::info!("Performing merge of origin/main...");
        repo.merge(&[&annotated], None, None)?;

        let mut index = repo.index()?;
        if index.has_conflicts() {
            repo.cleanup_state()?;
            return Err(GitError::MergeConflicts);
        }

        // Create merge commit
        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        let sig = Signature::now("Firm MCP", "mcp@firm.bot")?;
        let head_commit = repo.head()?.peel_to_commit()?;
        let merge_commit = repo.find_commit(annotated.id())?;
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Merge origin/main into mcp",
            &tree,
            &[&head_commit, &merge_commit],
        )?;
        repo.cleanup_state()?;
        log::info!("Merge commit created");
    }

    Ok(())
}

/// Stage all changes, commit, and push to origin.
pub async fn commit_and_push(config: GitConfig, message: &str) -> Result<(), GitError> {
    let message = message.to_string();
    tokio::task::spawn_blocking(move || commit_and_push_sync(&config, &message))
        .await
        .map_err(GitError::Join)?
}

fn commit_and_push_sync(config: &GitConfig, message: &str) -> Result<(), GitError> {
    let repo = Repository::open(&config.repo_path)?;

    // Stage all changes
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
    index.write()?;

    // Check if there are changes
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;

    let head = repo.head()?;
    let parent = head.peel_to_commit()?;

    // Compare with parent tree to detect no-op
    if parent.tree()?.id() == tree.id() {
        return Err(GitError::NothingToCommit);
    }

    // Create commit
    let sig = Signature::now("Firm MCP", "mcp@firm.bot")?;
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?;
    log::info!("Committed: {}", message);

    // Push
    let mut remote = repo.find_remote("origin")?;
    let refspec = format!(
        "refs/heads/{}:refs/heads/{}",
        config.branch, config.branch
    );
    let mut opts = push_options(&config.token);
    remote.push(&[&refspec], Some(&mut opts))?;
    log::info!("Pushed to origin/{}", config.branch);

    Ok(())
}
