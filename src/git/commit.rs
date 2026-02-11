use git2::{IndexAddOption, Repository, Signature};

use super::{GitConfig, GitError, push_options};

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

    // Push (force-push: single-user branch, safe after squash-merge cycles)
    let mut remote = repo.find_remote("origin")?;
    let refspec = format!(
        "+refs/heads/{}:refs/heads/{}",
        config.branch, config.branch
    );
    let mut opts = push_options(&config.token);
    remote.push(&[&refspec], Some(&mut opts))?;
    log::info!("Pushed to origin/{}", config.branch);

    Ok(())
}
