use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DiffInfo {
    pub head: String,
    pub branches: Vec<String>,
    pub tags: Vec<String>,
    pub diffs: Vec<FileDiff>,
}

// ============================================================================
// Git Branch Operations
// ============================================================================

/// Branch information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitBranchInfo {
    pub name: String,
    pub is_remote: bool,
    pub is_head: bool,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub last_commit_id: Option<String>,
    pub last_commit_summary: Option<String>,
}

/// Result of branch operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitBranchResult {
    pub success: bool,
    pub message: String,
    pub branch_name: Option<String>,
}

// ============================================================================
// Git Push/Pull/Fetch Operations
// ============================================================================

/// Push options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitPushOptions {
    pub remote: String,
    pub branch: Option<String>,
    pub force: bool,
    pub force_with_lease: bool,
    pub set_upstream: bool,
    pub push_tags: bool,
}

/// Pull options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitPullOptions {
    pub remote: String,
    pub branch: Option<String>,
    pub rebase: bool,
    pub autostash: bool,
}

/// Fetch options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitFetchOptions {
    pub remote: Option<String>,
    pub prune: bool,
    pub all: bool,
    pub tags: bool,
}

/// Result of push/pull/fetch operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitRemoteResult {
    pub success: bool,
    pub message: String,
    pub updated_refs: Vec<String>,
}

// ============================================================================
// Git Stash Operations
// ============================================================================

/// Stash entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitStashEntry {
    pub index: usize,
    pub message: String,
    pub branch: String,
    pub commit_id: String,
    pub timestamp: i64,
}

/// Stash list result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitStashList {
    pub entries: Vec<GitStashEntry>,
}

/// Stash operation result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitStashResult {
    pub success: bool,
    pub message: String,
}

// ============================================================================
// Git Merge Operations
// ============================================================================

/// Merge options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitMergeOptions {
    pub branch: String,
    pub no_ff: bool,
    pub squash: bool,
    pub message: Option<String>,
}

/// Merge result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitMergeResult {
    pub success: bool,
    pub message: String,
    pub conflicts: Vec<PathBuf>,
    pub merged_commit: Option<String>,
}

// ============================================================================
// Git Rebase Operations
// ============================================================================

/// Rebase options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitRebaseOptions {
    pub onto: String,
    pub interactive: bool,
    pub autostash: bool,
    pub preserve_merges: bool,
}

/// Rebase result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitRebaseResult {
    pub success: bool,
    pub message: String,
    pub conflicts: Vec<PathBuf>,
    pub current_step: usize,
    pub total_steps: usize,
}

/// Rebase action for interactive rebase
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GitRebaseAction {
    Continue,
    Abort,
    Skip,
}

// ============================================================================
// Git Cherry-pick Operations
// ============================================================================

/// Cherry-pick options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitCherryPickOptions {
    pub commits: Vec<String>,
    pub no_commit: bool,
    pub mainline: Option<usize>,
}

/// Cherry-pick result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitCherryPickResult {
    pub success: bool,
    pub message: String,
    pub conflicts: Vec<PathBuf>,
    pub cherry_picked_commits: Vec<String>,
}

// ============================================================================
// Git Reset Operations
// ============================================================================

/// Reset mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GitResetMode {
    Soft,
    Mixed,
    Hard,
    Keep,
}

/// Reset options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitResetOptions {
    pub target: String,
    pub mode: GitResetMode,
}

/// Reset result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitResetResult {
    pub success: bool,
    pub message: String,
    pub new_head: Option<String>,
}

// ============================================================================
// Git Revert Operations
// ============================================================================

/// Revert options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitRevertOptions {
    pub commits: Vec<String>,
    pub no_commit: bool,
    pub mainline: Option<usize>,
}

/// Revert result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitRevertResult {
    pub success: bool,
    pub message: String,
    pub conflicts: Vec<PathBuf>,
    pub reverted_commits: Vec<String>,
}

// ============================================================================
// Git Blame Operations
// ============================================================================

/// Blame line info
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitBlameLine {
    pub line_number: usize,
    pub commit_id: String,
    pub short_commit_id: String,
    pub author_name: String,
    pub author_email: String,
    pub timestamp: i64,
    pub summary: String,
    pub original_line_number: usize,
    pub original_path: Option<PathBuf>,
}

/// Blame result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitBlameResult {
    pub lines: Vec<GitBlameLine>,
    pub path: PathBuf,
}

// ============================================================================
// Git Tag Operations
// ============================================================================

/// Tag info
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitTagInfo {
    pub name: String,
    pub commit_id: String,
    pub message: Option<String>,
    pub tagger_name: Option<String>,
    pub tagger_email: Option<String>,
    pub timestamp: Option<i64>,
    pub is_annotated: bool,
}

/// Tag operation result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitTagResult {
    pub success: bool,
    pub message: String,
    pub tag_name: Option<String>,
}

// ============================================================================
// Git Remote Operations
// ============================================================================

/// Remote info
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitRemoteInfo {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

/// Remote list result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitRemoteList {
    pub remotes: Vec<GitRemoteInfo>,
}

// ============================================================================
// Git Status
// ============================================================================

/// Repository status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitStatus {
    pub is_rebasing: bool,
    pub is_merging: bool,
    pub is_cherry_picking: bool,
    pub is_reverting: bool,
    pub is_bisecting: bool,
    pub rebase_head_name: Option<String>,
    pub merge_head: Option<String>,
    pub conflicts: Vec<PathBuf>,
}

// ============================================================================
// Git Diff (detailed)
// ============================================================================

/// Diff hunk
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitDiffHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub header: String,
    pub lines: Vec<GitDiffLine>,
}

/// Diff line
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitDiffLine {
    pub origin: char, // '+', '-', ' ', etc.
    pub old_line_no: Option<usize>,
    pub new_line_no: Option<usize>,
    pub content: String,
}

/// File diff with hunks
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitFileDiff {
    pub old_path: Option<PathBuf>,
    pub new_path: Option<PathBuf>,
    pub status: FileDiffKind,
    pub hunks: Vec<GitDiffHunk>,
    pub is_binary: bool,
}

/// Commit diff result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitCommitDiff {
    pub commit_id: String,
    pub files: Vec<GitFileDiff>,
}

/// Represents a single git commit in the log
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitCommitInfo {
    /// Commit hash (full SHA)
    pub id: String,
    /// Short commit hash (first 7 chars)
    pub short_id: String,
    /// Commit message (first line / summary)
    pub summary: String,
    /// Full commit message
    pub message: String,
    /// Author name
    pub author_name: String,
    /// Author email
    pub author_email: String,
    /// Commit timestamp (Unix epoch seconds)
    pub timestamp: i64,
    /// Parent commit IDs
    pub parents: Vec<String>,
    /// Branches that point to this commit
    pub branches: Vec<String>,
    /// Tags that point to this commit
    pub tags: Vec<String>,
    /// Whether this is the HEAD commit
    pub is_head: bool,
}

/// Git log result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitLogResult {
    pub commits: Vec<GitCommitInfo>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FileDiff {
    Modified(PathBuf),
    Added(PathBuf),
    Deleted(PathBuf),
    Renamed(PathBuf, PathBuf),
}

impl FileDiff {
    pub fn path(&self) -> &PathBuf {
        match &self {
            FileDiff::Modified(p)
            | FileDiff::Added(p)
            | FileDiff::Deleted(p)
            | FileDiff::Renamed(_, p) => p,
        }
    }

    pub fn kind(&self) -> FileDiffKind {
        match self {
            FileDiff::Modified(_) => FileDiffKind::Modified,
            FileDiff::Added(_) => FileDiffKind::Added,
            FileDiff::Deleted(_) => FileDiffKind::Deleted,
            FileDiff::Renamed(_, _) => FileDiffKind::Renamed,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileDiffKind {
    Modified,
    Added,
    Deleted,
    Renamed,
}
