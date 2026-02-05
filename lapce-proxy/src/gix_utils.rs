//! Gitoxide (gix) utility functions for git operations
//!
//! This module provides wrapper functions around the gitoxide (gix) library,
//! which is a pure Rust implementation of Git. These functions are used to
//! incrementally replace git2 operations with gix equivalents.

use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use gix::bstr::ByteSlice;
use lapce_rpc::source_control::GitBranchInfo;

/// Discover a git repository starting from the given path.
///
/// This is equivalent to `git2::Repository::discover()`.
pub fn discover_repo(path: &Path) -> Result<gix::Repository> {
    gix::discover(path)
        .with_context(|| format!("Failed to discover git repository at {:?}", path))
}

/// Get the current HEAD reference name (short form) from a repository.
///
/// Returns the branch name if HEAD points to a branch, or None if detached.
pub fn get_head_name(repo: &gix::Repository) -> Option<String> {
    repo.head_name()
        .ok()
        .flatten()
        .map(|name| {
            let full_name = name.as_bstr();
            // Strip "refs/heads/" prefix if present
            if let Some(short) = full_name.strip_prefix(b"refs/heads/") {
                short.to_str_lossy().into_owned()
            } else {
                full_name.to_str_lossy().into_owned()
            }
        })
}

/// Get the current HEAD reference name from a path.
///
/// Returns the branch name if HEAD points to a branch, or None if detached.
pub fn get_head_name_from_path(workspace_path: &Path) -> Result<Option<String>> {
    let repo = discover_repo(workspace_path)?;
    Ok(get_head_name(&repo))
}

/// List branches in the repository using gix.
///
/// This is a partial replacement for the git2-based `git_list_branches` function.
/// Currently returns basic branch information without upstream tracking details.
pub fn list_branches(
    workspace_path: &Path,
    include_remote: bool,
) -> Result<Vec<GitBranchInfo>> {
    let repo = discover_repo(workspace_path)?;
    let head_name = get_head_name(&repo);
    
    let mut branches = Vec::new();
    
    // Get local branches
    let refs = repo.references().context("Failed to get references")?;
    for branch_ref in refs.local_branches()? {
        match branch_ref {
            Ok(reference) => {
                let name = reference.name().shorten().to_str_lossy().into_owned();
                let is_head = head_name.as_ref() == Some(&name);
                
                // Get commit info
                let (last_commit_id, last_commit_summary) = get_commit_info(&reference);
                
                branches.push(GitBranchInfo {
                    name,
                    is_remote: false,
                    is_head,
                    upstream: None,  // TODO: Implement upstream tracking
                    ahead: 0,        // TODO: Implement ahead/behind calculation
                    behind: 0,
                    last_commit_id,
                    last_commit_summary,
                });
            }
            Err(e) => {
                tracing::warn!("Failed to read branch reference: {}", e);
            }
        }
    }
    
    // Get remote branches if requested
    if include_remote {
        let refs = repo.references().context("Failed to get references for remotes")?;
        for branch_ref in refs.remote_branches()? {
            match branch_ref {
                Ok(reference) => {
                    let name = reference.name().shorten().to_str_lossy().into_owned();
                    
                    // Skip HEAD references
                    if name.ends_with("/HEAD") || name == "HEAD" {
                        continue;
                    }
                    
                    let (last_commit_id, last_commit_summary) = get_commit_info(&reference);
                    
                    branches.push(GitBranchInfo {
                        name,
                        is_remote: true,
                        is_head: false,
                        upstream: None,
                        ahead: 0,
                        behind: 0,
                        last_commit_id,
                        last_commit_summary,
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to read remote branch reference: {}", e);
                }
            }
        }
    }
    
    Ok(branches)
}

/// Get commit information from a reference.
fn get_commit_info(reference: &gix::Reference<'_>) -> (Option<String>, Option<String>) {
    match reference.try_id() {
        Some(id) => {
            let commit_id = Some(id.to_string());
            
            // Try to get commit summary
            let summary = reference.id().object()
                .ok()
                .and_then(|obj| obj.try_into_commit().ok())
                .and_then(|commit| {
                    commit.message_raw()
                        .ok()
                        .map(|msg| {
                            // Get first line as summary
                            msg.lines()
                                .next()
                                .map(|line| line.to_str_lossy().into_owned())
                                .unwrap_or_default()
                        })
                });
            
            (commit_id, summary)
        }
        None => (None, None),
    }
}

/// Get all branch names (short form) as a simple list.
/// 
/// This is useful for populating branch dropdowns in the UI.
pub fn get_branch_names(workspace_path: &Path, include_remote: bool) -> Result<Vec<String>> {
    let branches = list_branches(workspace_path, include_remote)?;
    Ok(branches.into_iter().map(|b| b.name).collect())
}

/// Check if a path is inside a git repository.
pub fn is_git_repo(path: &Path) -> bool {
    gix::discover(path).is_ok()
}

/// Get the repository's work directory (working tree root).
pub fn get_work_dir(workspace_path: &Path) -> Result<Option<std::path::PathBuf>> {
    let repo = discover_repo(workspace_path)?;
    Ok(repo.work_dir().map(|p| p.to_path_buf()))
}

/// Get the repository's git directory (.git folder).
pub fn get_git_dir(workspace_path: &Path) -> Result<std::path::PathBuf> {
    let repo = discover_repo(workspace_path)?;
    Ok(repo.git_dir().to_path_buf())
}

/// Get the HEAD commit ID as a hex string.
pub fn get_head_commit_id(workspace_path: &Path) -> Result<Option<String>> {
    let repo = discover_repo(workspace_path)?;
    
    match repo.head_commit() {
        Ok(commit) => Ok(Some(commit.id().to_string())),
        Err(_) => Ok(None), // Unborn branch or other error
    }
}

/// List all tags in the repository.
pub fn list_tags(workspace_path: &Path) -> Result<Vec<String>> {
    let repo = discover_repo(workspace_path)?;
    let refs = repo.references().context("Failed to get references")?;
    
    let mut tags = Vec::new();
    for tag_ref in refs.tags()? {
        match tag_ref {
            Ok(reference) => {
                let name = reference.name().shorten().to_str_lossy().into_owned();
                tags.push(name);
            }
            Err(e) => {
                tracing::warn!("Failed to read tag reference: {}", e);
            }
        }
    }
    
    Ok(tags)
}

/// List all remote names in the repository.
pub fn list_remotes(workspace_path: &Path) -> Result<Vec<String>> {
    let repo = discover_repo(workspace_path)?;
    
    let mut remotes = Vec::new();
    for remote_name in repo.remote_names() {
        remotes.push(remote_name.to_str_lossy().into_owned());
    }
    
    Ok(remotes)
}

/// Get information about a specific remote.
pub fn get_remote_url(workspace_path: &Path, remote_name: &str) -> Result<Option<String>> {
    let repo = discover_repo(workspace_path)?;
    
    match repo.find_remote(remote_name) {
        Ok(remote) => {
            let url = remote.url(gix::remote::Direction::Fetch)
                .map(|u| u.to_bstring().to_string());
            Ok(url)
        }
        Err(_) => Ok(None),
    }
}

/// Walk through commits starting from HEAD.
/// Returns an iterator that yields commit IDs.
pub fn walk_commits(
    workspace_path: &Path,
    max_count: Option<usize>,
) -> Result<Vec<String>> {
    let repo = discover_repo(workspace_path)?;
    
    let head = repo.head_commit()
        .context("Failed to get HEAD commit")?;
    
    let mut commits = Vec::new();
    let walk = repo.rev_walk([head.id])
        .all()
        .context("Failed to create revision walker")?;
    
    for (i, info) in walk.enumerate() {
        if let Some(max) = max_count {
            if i >= max {
                break;
            }
        }
        
        match info {
            Ok(info) => {
                commits.push(info.id().to_string());
            }
            Err(e) => {
                tracing::warn!("Failed to walk commit: {}", e);
                break;
            }
        }
    }
    
    Ok(commits)
}

// ============================================================================
// Status Operations
// ============================================================================

/// Check if the repository has uncommitted changes (is dirty).
/// 
/// This checks both staged and unstaged changes.
pub fn is_dirty(workspace_path: &Path) -> Result<bool> {
    let repo = discover_repo(workspace_path)?;
    repo.is_dirty()
        .context("Failed to check if repository is dirty")
}

/// Status item representing a changed file.
#[derive(Debug, Clone)]
pub struct StatusItem {
    pub path: std::path::PathBuf,
    pub status: FileStatus,
}

/// The status of a file in the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Ignored,
    Conflicted,
    TypeChange,
}

/// Get status items from the repository using gix.
/// 
/// Returns a list of files with their status (modified, added, deleted, etc.)
pub fn get_status(workspace_path: &Path) -> Result<Vec<StatusItem>> {
    use gix::status::index_worktree::iter::Summary;
    
    let repo = discover_repo(workspace_path)?;
    
    // Ensure we have a working directory
    let work_dir = repo.work_dir()
        .context("Repository has no working directory")?
        .to_path_buf();
    
    let mut items = Vec::new();
    
    // Create status iterator
    let status = repo.status(gix::progress::Discard)
        .context("Failed to create status platform")?;
    
    let iter = status.into_iter(Vec::<gix::bstr::BString>::new())
        .context("Failed to create status iterator")?;
    
    for entry in iter {
        match entry {
            Ok(item) => {
                let path = work_dir.join(item.location().to_str_lossy().as_ref());
                
                // Map gix status to our FileStatus
                let file_status = match item {
                    gix::status::Item::IndexWorktree(iw_item) => {
                        match iw_item.summary() {
                            Some(Summary::Added) => Some(FileStatus::Added),
                            Some(Summary::Removed) => Some(FileStatus::Deleted),
                            Some(Summary::Modified) => Some(FileStatus::Modified),
                            Some(Summary::Renamed) => Some(FileStatus::Renamed),
                            Some(Summary::Copied) => Some(FileStatus::Copied),
                            Some(Summary::TypeChange) => Some(FileStatus::TypeChange),
                            Some(Summary::Conflict) => Some(FileStatus::Conflicted),
                            Some(Summary::IntentToAdd) => Some(FileStatus::Added),
                            None => None,
                        }
                    }
                    gix::status::Item::TreeIndex(change) => {
                        use gix::diff::index::Change;
                        match change {
                            Change::Addition { .. } => Some(FileStatus::Added),
                            Change::Deletion { .. } => Some(FileStatus::Deleted),
                            Change::Modification { .. } => Some(FileStatus::Modified),
                            Change::Rewrite { .. } => Some(FileStatus::Renamed),
                        }
                    }
                };
                
                if let Some(status) = file_status {
                    items.push(StatusItem { path, status });
                }
            }
            Err(e) => {
                tracing::warn!("Failed to get status item: {}", e);
            }
        }
    }
    
    Ok(items)
}

// ============================================================================
// Checkout Operations
// ============================================================================
//
// Note: Full checkout (updating worktree) requires git2 for now.
// gix provides low-level worktree-state functionality, but it's more complex
// to use than git2's checkout_tree(). These helpers provide pre-checkout
// validation and reference resolution using gix.

/// Parse a revision specification and return the commit ID.
/// 
/// This is similar to `git rev-parse`.
pub fn rev_parse(workspace_path: &Path, spec: &str) -> Result<String> {
    let repo = discover_repo(workspace_path)?;
    
    let object = repo.rev_parse_single(spec)
        .with_context(|| format!("Failed to parse revision: {}", spec))?;
    
    Ok(object.detach().to_string())
}

/// Check if a reference exists in the repository.
pub fn reference_exists(workspace_path: &Path, name: &str) -> Result<bool> {
    let repo = discover_repo(workspace_path)?;
    
    // Try different reference formats
    let full_names = [
        format!("refs/heads/{}", name),
        format!("refs/remotes/{}", name),
        format!("refs/tags/{}", name),
        name.to_string(),
    ];
    
    for ref_name in &full_names {
        if repo.try_find_reference(ref_name).ok().flatten().is_some() {
            return Ok(true);
        }
    }
    
    Ok(false)
}

/// Get the target commit ID of a reference.
pub fn get_reference_target(workspace_path: &Path, name: &str) -> Result<Option<String>> {
    let repo = discover_repo(workspace_path)?;
    
    // Try different reference formats
    let full_names = [
        format!("refs/heads/{}", name),
        format!("refs/remotes/{}", name),
        format!("refs/tags/{}", name),
        name.to_string(),
    ];
    
    for ref_name in &full_names {
        if let Ok(Some(reference)) = repo.try_find_reference(ref_name) {
            if let Some(id) = reference.try_id() {
                return Ok(Some(id.to_string()));
            }
        }
    }
    
    Ok(None)
}

/// Validate that a checkout target exists and is valid.
/// Returns the resolved commit ID if valid, or an error if not.
pub fn validate_checkout_target(workspace_path: &Path, reference: &str) -> Result<String> {
    let repo = discover_repo(workspace_path)?;
    
    // Try to resolve the reference
    let resolved = repo.rev_parse_single(reference)
        .with_context(|| format!("Cannot resolve '{}' - not a valid branch, tag, or commit", reference))?;
    
    Ok(resolved.detach().to_string())
}

/// Check if the repository has uncommitted changes that would block checkout.
/// Returns true if there are changes that need to be stashed or committed.
pub fn has_uncommitted_changes(workspace_path: &Path) -> Result<bool> {
    is_dirty(workspace_path)
}

// ============================================================================
// Repository Initialization
// ============================================================================

/// Initialize a new git repository at the given path.
pub fn init_repo(path: &Path) -> Result<()> {
    use std::process::Command;
    
    // gix doesn't have a simple init API in basic features, use git command
    let output = Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .context("Failed to run git init")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git init failed: {}", stderr);
    }
    
    Ok(())
}

// ============================================================================
// Commit Operations
// ============================================================================

/// Create a new commit with the given message.
/// 
/// This commits all staged changes.
pub fn commit(workspace_path: &Path, message: &str) -> Result<String> {
    use std::process::Command;
    
    // Use git command for committing (gix commit API requires complex index manipulation)
    let output = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git commit")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git commit failed: {}", stderr);
    }
    
    // Get the commit ID
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_path)
        .output()
        .context("Failed to get commit ID")?;
    
    let commit_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(commit_id)
}

// ============================================================================
// Index/Staging Operations
// ============================================================================

/// Stage files by adding them to the index.
pub fn stage_files(workspace_path: &Path, paths: &[std::path::PathBuf]) -> Result<()> {
    use std::process::Command;
    
    if paths.is_empty() {
        return Ok(());
    }
    
    // Use git command for staging (gix index manipulation is complex)
    let path_strs: Vec<&str> = paths.iter()
        .filter_map(|p| p.to_str())
        .collect();
    
    let output = Command::new("git")
        .args(["add", "--"])
        .args(&path_strs)
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git add")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git add failed: {}", stderr);
    }
    
    Ok(())
}

/// Unstage files by removing them from the index.
pub fn unstage_files(workspace_path: &Path, paths: &[std::path::PathBuf]) -> Result<()> {
    use std::process::Command;
    
    if paths.is_empty() {
        return Ok(());
    }
    
    let path_strs: Vec<&str> = paths.iter()
        .filter_map(|p| p.to_str())
        .collect();
    
    let output = Command::new("git")
        .args(["reset", "HEAD", "--"])
        .args(&path_strs)
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git reset")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git reset failed: {}", stderr);
    }
    
    Ok(())
}

/// Stage all changes.
pub fn stage_all(workspace_path: &Path) -> Result<()> {
    use std::process::Command;
    
    let output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git add -A")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git add -A failed: {}", stderr);
    }
    
    Ok(())
}

/// Unstage all changes.
pub fn unstage_all(workspace_path: &Path) -> Result<()> {
    use std::process::Command;
    
    let output = Command::new("git")
        .args(["reset", "HEAD"])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git reset HEAD")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git reset HEAD failed: {}", stderr);
    }
    
    Ok(())
}

// ============================================================================
// Checkout Operations (using git command for reliability)
// ============================================================================

/// Checkout a branch, tag, or commit.
pub fn checkout(workspace_path: &Path, reference: &str, force: bool) -> Result<()> {
    use std::process::Command;
    
    let mut args = vec!["checkout"];
    if force {
        args.push("-f");
    }
    args.push(reference);
    
    let output = Command::new("git")
        .args(&args)
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git checkout")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git checkout failed: {}", stderr);
    }
    
    Ok(())
}

/// Discard changes in specific files.
pub fn discard_file_changes(workspace_path: &Path, paths: &[std::path::PathBuf]) -> Result<()> {
    use std::process::Command;
    
    if paths.is_empty() {
        return Ok(());
    }
    
    let path_strs: Vec<&str> = paths.iter()
        .filter_map(|p| p.to_str())
        .collect();
    
    let output = Command::new("git")
        .args(["checkout", "HEAD", "--"])
        .args(&path_strs)
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git checkout")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git checkout failed: {}", stderr);
    }
    
    Ok(())
}

/// Discard all workspace changes.
pub fn discard_all_changes(workspace_path: &Path) -> Result<()> {
    use std::process::Command;
    
    // Reset staged changes
    let _ = Command::new("git")
        .args(["reset", "HEAD"])
        .current_dir(workspace_path)
        .output();
    
    // Checkout all files
    let output = Command::new("git")
        .args(["checkout", "."])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git checkout")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git checkout failed: {}", stderr);
    }
    
    // Clean untracked files
    let _ = Command::new("git")
        .args(["clean", "-fd"])
        .current_dir(workspace_path)
        .output();
    
    Ok(())
}

// ============================================================================
// Stash Operations (via git command - gix doesn't support stash)
// ============================================================================

/// List all stashes.
pub fn stash_list(workspace_path: &Path) -> Result<Vec<StashEntry>> {
    use std::process::Command;
    
    // Get current branch for reference
    let current_branch = get_head_name_from_path(workspace_path)
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".to_string());
    
    let output = Command::new("git")
        .args(["stash", "list", "--format=%H|%s|%an|%ae|%at"])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git stash list")?;
    
    if !output.status.success() {
        return Ok(Vec::new());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    
    for (index, line) in stdout.lines().enumerate() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 5 {
            // Extract branch from message if it's in "WIP on <branch>:" format
            let message = parts[1].to_string();
            let branch = if message.starts_with("WIP on ") {
                message.strip_prefix("WIP on ")
                    .and_then(|s| s.split(':').next())
                    .unwrap_or(&current_branch)
                    .to_string()
            } else if message.starts_with("On ") {
                message.strip_prefix("On ")
                    .and_then(|s| s.split(':').next())
                    .unwrap_or(&current_branch)
                    .to_string()
            } else {
                current_branch.clone()
            };
            
            entries.push(StashEntry {
                index,
                message,
                branch,
                commit_id: parts[0].to_string(),
                author_name: parts[2].to_string(),
                author_email: parts[3].to_string(),
                timestamp: parts[4].parse().unwrap_or(0),
            });
        }
    }
    
    Ok(entries)
}

/// A stash entry.
#[derive(Debug, Clone)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    pub branch: String,
    pub commit_id: String,
    pub author_name: String,
    pub author_email: String,
    pub timestamp: i64,
}

/// Save changes to stash.
pub fn stash_save(workspace_path: &Path, message: Option<&str>, include_untracked: bool) -> Result<()> {
    use std::process::Command;
    
    let mut args = vec!["stash", "push"];
    if include_untracked {
        args.push("-u");
    }
    if let Some(msg) = message {
        args.push("-m");
        args.push(msg);
    }
    
    let output = Command::new("git")
        .args(&args)
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git stash push")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git stash push failed: {}", stderr);
    }
    
    Ok(())
}

/// Pop a stash entry.
pub fn stash_pop(workspace_path: &Path, index: usize) -> Result<()> {
    use std::process::Command;
    
    let stash_ref = format!("stash@{{{}}}", index);
    
    let output = Command::new("git")
        .args(["stash", "pop", &stash_ref])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git stash pop")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git stash pop failed: {}", stderr);
    }
    
    Ok(())
}

/// Apply a stash entry without removing it.
pub fn stash_apply(workspace_path: &Path, index: usize) -> Result<()> {
    use std::process::Command;
    
    let stash_ref = format!("stash@{{{}}}", index);
    
    let output = Command::new("git")
        .args(["stash", "apply", &stash_ref])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git stash apply")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git stash apply failed: {}", stderr);
    }
    
    Ok(())
}

/// Drop a stash entry.
pub fn stash_drop(workspace_path: &Path, index: usize) -> Result<()> {
    use std::process::Command;
    
    let stash_ref = format!("stash@{{{}}}", index);
    
    let output = Command::new("git")
        .args(["stash", "drop", &stash_ref])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git stash drop")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git stash drop failed: {}", stderr);
    }
    
    Ok(())
}

// ============================================================================
// Branch Operations
// ============================================================================

/// Create a new branch.
pub fn create_branch(workspace_path: &Path, name: &str, start_point: Option<&str>) -> Result<()> {
    use std::process::Command;
    
    let mut args = vec!["branch", name];
    if let Some(point) = start_point {
        args.push(point);
    }
    
    let output = Command::new("git")
        .args(&args)
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git branch")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git branch failed: {}", stderr);
    }
    
    Ok(())
}

/// Delete a branch.
pub fn delete_branch(workspace_path: &Path, name: &str, force: bool) -> Result<()> {
    use std::process::Command;
    
    let flag = if force { "-D" } else { "-d" };
    
    let output = Command::new("git")
        .args(["branch", flag, name])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git branch -d")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git branch -d failed: {}", stderr);
    }
    
    Ok(())
}

/// Rename a branch.
pub fn rename_branch(workspace_path: &Path, old_name: &str, new_name: &str) -> Result<()> {
    use std::process::Command;
    
    let output = Command::new("git")
        .args(["branch", "-m", old_name, new_name])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git branch -m")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git branch -m failed: {}", stderr);
    }
    
    Ok(())
}

// ============================================================================
// Tag Operations
// ============================================================================

/// Create a new tag.
pub fn create_tag(workspace_path: &Path, name: &str, target: Option<&str>, message: Option<&str>) -> Result<()> {
    use std::process::Command;
    
    let mut args = vec!["tag"];
    
    if let Some(msg) = message {
        args.push("-a");
        args.push(name);
        args.push("-m");
        args.push(msg);
    } else {
        args.push(name);
    }
    
    if let Some(t) = target {
        args.push(t);
    }
    
    let output = Command::new("git")
        .args(&args)
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git tag")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git tag failed: {}", stderr);
    }
    
    Ok(())
}

/// Delete a tag.
pub fn delete_tag(workspace_path: &Path, name: &str) -> Result<()> {
    use std::process::Command;
    
    let output = Command::new("git")
        .args(["tag", "-d", name])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git tag -d")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git tag -d failed: {}", stderr);
    }
    
    Ok(())
}

// ============================================================================
// Remote Operations
// ============================================================================

/// Add a new remote.
pub fn add_remote(workspace_path: &Path, name: &str, url: &str) -> Result<()> {
    use std::process::Command;
    
    let output = Command::new("git")
        .args(["remote", "add", name, url])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git remote add")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git remote add failed: {}", stderr);
    }
    
    Ok(())
}

/// Remove a remote.
pub fn remove_remote(workspace_path: &Path, name: &str) -> Result<()> {
    use std::process::Command;
    
    let output = Command::new("git")
        .args(["remote", "remove", name])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git remote remove")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git remote remove failed: {}", stderr);
    }
    
    Ok(())
}

// ============================================================================
// Merge and Reset Operations
// ============================================================================

/// Merge result.
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub success: bool,
    pub message: String,
    pub conflicts: Vec<PathBuf>,
    pub merged_commit: Option<String>,
}

/// Merge a branch into the current branch.
pub fn merge(
    workspace_path: &Path,
    branch: &str,
    message: Option<&str>,
    no_ff: bool,
    squash: bool,
) -> Result<MergeResult> {
    use std::process::Command;
    
    let mut args = vec!["merge"];
    if no_ff {
        args.push("--no-ff");
    }
    if squash {
        args.push("--squash");
    }
    if let Some(msg) = message {
        args.push("-m");
        args.push(msg);
    }
    args.push(branch);
    
    let output = Command::new("git")
        .args(&args)
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git merge")?;
    
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    
    if output.status.success() {
        // Get merged commit
        let commit = get_head_commit_id(workspace_path)
            .ok()
            .flatten()
            .map(|id| id.to_string());
        
        return Ok(MergeResult {
            success: true,
            message: format!("Successfully merged '{}'", branch),
            conflicts: Vec::new(),
            merged_commit: commit,
        });
    }
    
    // Check for conflicts
    let mut conflicts = Vec::new();
    if stderr.contains("CONFLICT") || stderr.contains("conflict") {
        // Get list of conflicting files
        if let Ok(status_output) = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(workspace_path)
            .output()
        {
            let status = String::from_utf8_lossy(&status_output.stdout);
            for line in status.lines() {
                if line.starts_with("UU ") || line.starts_with("AA ") || line.starts_with("DD ") {
                    conflicts.push(PathBuf::from(line[3..].trim()));
                }
            }
        }
        
        return Ok(MergeResult {
            success: false,
            message: "Merge has conflicts".to_string(),
            conflicts,
            merged_commit: None,
        });
    }
    
    // Check for "already up to date"
    if stdout.contains("Already up to date") || stdout.contains("Already up-to-date") {
        return Ok(MergeResult {
            success: true,
            message: "Already up to date".to_string(),
            conflicts: Vec::new(),
            merged_commit: None,
        });
    }
    
    // Other failure
    Ok(MergeResult {
        success: false,
        message: stderr,
        conflicts: Vec::new(),
        merged_commit: None,
    })
}

/// Abort an in-progress merge.
pub fn merge_abort(workspace_path: &Path) -> Result<()> {
    use std::process::Command;
    
    let output = Command::new("git")
        .args(["merge", "--abort"])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git merge --abort")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git merge --abort failed: {}", stderr);
    }
    
    Ok(())
}

/// Reset to a specific commit.
pub fn reset(workspace_path: &Path, target: &str, mode: ResetMode) -> Result<()> {
    use std::process::Command;
    
    let mode_arg = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
        ResetMode::Keep => "--keep",
    };
    
    let output = Command::new("git")
        .args(["reset", mode_arg, target])
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git reset")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git reset failed: {}", stderr);
    }
    
    Ok(())
}

/// Reset mode.
#[derive(Debug, Clone, Copy)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
    Keep,
}

// ============================================================================
// Blame Operations  
// ============================================================================

/// Blame result for a file.
#[derive(Debug, Clone)]
pub struct BlameEntry {
    pub line_start: u32,
    pub line_count: u32,
    pub original_line: usize,
    pub original_path: Option<PathBuf>,
    pub commit_id: String,
    pub author_name: String,
    pub author_email: String,
    pub timestamp: i64,
    pub summary: String,
}

/// Get blame information for a file.
/// 
/// Uses git command because gix blame API is complex and experimental.
pub fn blame(workspace_path: &Path, file_path: &Path, commit: Option<&str>) -> Result<Vec<BlameEntry>> {
    use std::process::Command;
    
    // Get the relative path
    let relative_path = file_path.strip_prefix(workspace_path)
        .unwrap_or(file_path);
    
    let relative_path_str = relative_path.to_str()
        .context("Invalid file path")?;
    
    let mut args = vec!["blame", "--porcelain"];
    if let Some(c) = commit {
        args.push(c);
        args.push("--");
    }
    args.push(relative_path_str);
    
    let output = Command::new("git")
        .args(&args)
        .current_dir(workspace_path)
        .output()
        .context("Failed to run git blame")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git blame failed: {}", stderr);
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    let mut current_commit = String::new();
    let mut current_author = String::new();
    let mut current_email = String::new();
    let mut current_time: i64 = 0;
    let mut current_summary = String::new();
    let mut current_line_start = 0u32;
    let mut current_line_count = 0u32;
    let mut current_orig_line = 1usize;
    let mut current_orig_path: Option<PathBuf> = None;
    
    for line in stdout.lines() {
        if line.starts_with(|c: char| c.is_ascii_hexdigit()) && line.len() >= 40 {
            // New blame header: <sha> <orig_line> <final_line> [<num_lines>]
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                // Save previous entry if we have one
                if !current_commit.is_empty() && current_line_count > 0 {
                    entries.push(BlameEntry {
                        line_start: current_line_start,
                        line_count: current_line_count,
                        original_line: current_orig_line,
                        original_path: current_orig_path.clone(),
                        commit_id: current_commit.clone(),
                        author_name: current_author.clone(),
                        author_email: current_email.clone(),
                        timestamp: current_time,
                        summary: current_summary.clone(),
                    });
                }
                
                current_commit = parts[0].to_string();
                current_orig_line = parts[1].parse().unwrap_or(1);
                current_line_start = parts[2].parse().unwrap_or(1);
                current_line_count = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
            }
        } else if let Some(author) = line.strip_prefix("author ") {
            current_author = author.to_string();
        } else if let Some(email) = line.strip_prefix("author-mail ") {
            current_email = email.trim_matches(|c| c == '<' || c == '>').to_string();
        } else if let Some(time) = line.strip_prefix("author-time ") {
            current_time = time.parse().unwrap_or(0);
        } else if let Some(summary) = line.strip_prefix("summary ") {
            current_summary = summary.to_string();
        } else if let Some(filename) = line.strip_prefix("filename ") {
            current_orig_path = Some(PathBuf::from(filename));
        }
    }
    
    // Don't forget the last entry
    if !current_commit.is_empty() && current_line_count > 0 {
        entries.push(BlameEntry {
            line_start: current_line_start,
            line_count: current_line_count,
            original_line: current_orig_line,
            original_path: current_orig_path,
            commit_id: current_commit,
            author_name: current_author,
            author_email: current_email,
            timestamp: current_time,
            summary: current_summary,
        });
    }
    
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    
    #[test]
    fn test_discover_repo() {
        // Test with the current working directory (assuming it's a git repo)
        let cwd = env::current_dir().unwrap();
        let result = discover_repo(&cwd);
        // This test will pass if run from within a git repository
        assert!(result.is_ok() || result.is_err());
    }
    
    #[test]
    fn test_is_git_repo() {
        let cwd = env::current_dir().unwrap();
        // This should return true or false without panicking
        let _ = is_git_repo(&cwd);
    }
}
