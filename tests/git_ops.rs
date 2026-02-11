#[allow(dead_code)]
mod common;

use firm_remote_mcp::git;
use common::{add_commit_to_bare, create_bare_origin, make_git_config};

// ── clone_or_fetch ──

#[tokio::test]
async fn clone_or_fetch_clones_repo() {
    let (origin_dir, _bare) = create_bare_origin();
    let (_work_dir, config) = make_git_config(origin_dir.path(), "mcp");

    git::clone_or_fetch(config.clone()).await.unwrap();

    // Verify the repo was cloned and has a file
    assert!(config.repo_path.join(".git").exists());
    assert!(config.repo_path.join("README.md").exists());
}

#[tokio::test]
async fn clone_or_fetch_fetches_on_second_call() {
    let (origin_dir, _bare) = create_bare_origin();
    let (_work_dir, config) = make_git_config(origin_dir.path(), "mcp");

    git::clone_or_fetch(config.clone()).await.unwrap();
    // Second call should succeed (fetch path)
    git::clone_or_fetch(config.clone()).await.unwrap();
}

// ── checkout_mcp_branch ──

#[tokio::test]
async fn checkout_mcp_branch_creates_from_main() {
    let (origin_dir, _bare) = create_bare_origin();
    let (_work_dir, config) = make_git_config(origin_dir.path(), "mcp");

    git::clone_or_fetch(config.clone()).await.unwrap();
    git::checkout_mcp_branch(config.clone()).await.unwrap();

    // Verify branch exists and is checked out
    let repo = git2::Repository::open(&config.repo_path).unwrap();
    let head = repo.head().unwrap();
    assert!(head.name().unwrap().ends_with("/mcp"));
}

#[tokio::test]
async fn checkout_mcp_branch_uses_remote_mcp_if_exists() {
    let (origin_dir, bare) = create_bare_origin();

    // Create an "mcp" branch on origin with extra content
    add_commit_to_bare(&bare, "main", "extra.txt", "from main");
    // Create mcp branch from main
    let main_commit = bare
        .find_reference("refs/heads/main")
        .unwrap()
        .peel_to_commit()
        .unwrap();
    bare.branch("mcp", &main_commit, false).unwrap();
    add_commit_to_bare(&bare, "mcp", "mcp-only.txt", "mcp content");

    let (_work_dir, config) = make_git_config(origin_dir.path(), "mcp");
    git::clone_or_fetch(config.clone()).await.unwrap();
    git::checkout_mcp_branch(config.clone()).await.unwrap();

    // Should have the mcp-only file (came from origin/mcp, not origin/main)
    assert!(config.repo_path.join("mcp-only.txt").exists());
}

// ── sync_branch ──

#[tokio::test]
async fn sync_branch_resets_to_remote() {
    let (origin_dir, bare) = create_bare_origin();
    let (_work_dir, config) = make_git_config(origin_dir.path(), "mcp");

    git::clone_or_fetch(config.clone()).await.unwrap();
    git::checkout_mcp_branch(config.clone()).await.unwrap();

    // Add a commit to origin/main
    add_commit_to_bare(&bare, "main", "new-file.txt", "new content");

    // Fetch + sync should bring in the new file (sync falls back to origin/main since no origin/mcp)
    git::clone_or_fetch(config.clone()).await.unwrap();
    git::sync_branch(config.clone()).await.unwrap();

    assert!(config.repo_path.join("new-file.txt").exists());
}

// ── commit_and_push ──

#[tokio::test]
async fn commit_and_push_creates_commit_and_pushes() {
    let (origin_dir, bare) = create_bare_origin();
    let (_work_dir, config) = make_git_config(origin_dir.path(), "mcp");

    git::clone_or_fetch(config.clone()).await.unwrap();
    git::checkout_mcp_branch(config.clone()).await.unwrap();

    // Write a new file
    std::fs::write(config.repo_path.join("test.txt"), "hello").unwrap();

    git::commit_and_push(config.clone(), "Add test file").await.unwrap();

    // Verify the commit exists locally
    let repo = git2::Repository::open(&config.repo_path).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap(), "Add test file");

    // Verify it was pushed to origin
    let pushed_ref = bare.find_reference("refs/heads/mcp");
    assert!(pushed_ref.is_ok());
}

#[tokio::test]
async fn commit_and_push_nothing_to_commit() {
    let (origin_dir, _bare) = create_bare_origin();
    let (_work_dir, config) = make_git_config(origin_dir.path(), "mcp");

    git::clone_or_fetch(config.clone()).await.unwrap();
    git::checkout_mcp_branch(config.clone()).await.unwrap();

    // No changes — should error
    let err = git::commit_and_push(config.clone(), "empty")
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("nothing to commit"),
        "Expected NothingToCommit, got: {}",
        err
    );
}

// ── merge_main ──

#[tokio::test]
async fn merge_main_fast_forwards() {
    let (origin_dir, bare) = create_bare_origin();
    let (_work_dir, config) = make_git_config(origin_dir.path(), "mcp");

    git::clone_or_fetch(config.clone()).await.unwrap();
    git::checkout_mcp_branch(config.clone()).await.unwrap();

    // Add a commit to origin/main
    add_commit_to_bare(&bare, "main", "from-main.txt", "main content");

    // Fetch and merge
    git::clone_or_fetch(config.clone()).await.unwrap();
    git::merge_main(config.clone()).await.unwrap();

    assert!(config.repo_path.join("from-main.txt").exists());
}
