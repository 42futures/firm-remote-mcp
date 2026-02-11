use git2::Repository;

use super::{GitConfig, GitError, fetch_options};

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
