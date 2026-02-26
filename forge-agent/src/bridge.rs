//! ProxyBridge trait -- abstraction over IDE capabilities.
//!
//! This trait is the boundary between the AI agent and the IDE backend.
//! The agent calls these methods instead of shelling out to rg, starting
//! its own LSP servers, or using git2 directly.  The IDE implements this
//! trait by routing calls through `lapce-proxy` / `lapce-rpc`.

use anyhow::Result;
use std::path::{Path, PathBuf};

// ── File operations ───────────────────────────────────────────────

/// A directory entry returned by `read_dir`.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub path: PathBuf,
    pub is_dir: bool,
}

// ── Search ────────────────────────────────────────────────────────

/// A single match from a global search (grep).
#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub line_number: usize,
    pub line_content: String,
}

// ── LSP / Code intelligence ──────────────────────────────────────

/// A location in source code (file + line + column).
#[derive(Debug, Clone)]
pub struct CodeLocation {
    pub path: PathBuf,
    pub line: u32,
    pub column: u32,
}

/// A document symbol (function, struct, etc.).
#[derive(Debug, Clone)]
pub struct DocSymbol {
    pub name: String,
    pub kind: String,   // "function", "struct", "class", etc.
    pub start_line: u32,
    pub end_line: u32,
    pub signature: String,
    pub children: Vec<DocSymbol>,
}

/// Hover information for a symbol.
#[derive(Debug, Clone)]
pub struct HoverInfo {
    pub contents: String,
}

/// A diagnostic (error/warning) from the language server.
#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub path: PathBuf,
    pub line: u32,
    pub column: u32,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

// ── Terminal ─────────────────────────────────────────────────────

/// Result of executing a command.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

// ── Git ──────────────────────────────────────────────────────────

/// Simplified git status for a file.
#[derive(Debug, Clone)]
pub struct GitFileStatus {
    pub path: PathBuf,
    pub status: String, // "modified", "added", "deleted", "untracked", etc.
}

/// A git commit entry.
#[derive(Debug, Clone)]
pub struct GitCommit {
    pub hash: String,
    pub message: String,
    pub author: String,
    pub timestamp: i64,
}

// ══════════════════════════════════════════════════════════════════
// The trait
// ══════════════════════════════════════════════════════════════════

/// Abstraction over IDE capabilities that the AI agent can use.
///
/// Instead of the agent directly calling `rg`, starting LSP servers,
/// or using `git2`, it goes through this trait.  The IDE wires it to
/// the proxy backend, giving the agent full access to the IDE's
/// already-running infrastructure.
#[async_trait::async_trait]
pub trait ProxyBridge: Send + Sync {
    // ── File operations ───────────────────────────────────────────

    /// Read file contents.
    async fn read_file(&self, path: &Path) -> Result<String>;

    /// Write file contents (creates if not exists).
    async fn write_file(&self, path: &Path, contents: &str) -> Result<()>;

    /// Create a directory (recursively).
    async fn create_dir(&self, path: &Path) -> Result<()>;

    /// List directory entries.
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;

    /// Delete a file or directory.
    async fn delete_path(&self, path: &Path) -> Result<()>;

    /// Rename / move a file or directory.
    async fn rename_path(&self, from: &Path, to: &Path) -> Result<()>;

    // ── Search ────────────────────────────────────────────────────

    /// Global regex search (like `rg`).
    async fn global_search(
        &self,
        pattern: &str,
        path: &Path,
        case_sensitive: bool,
        whole_word: bool,
        max_results: usize,
    ) -> Result<Vec<SearchMatch>>;

    // ── LSP / Code intelligence ──────────────────────────────────

    /// Go-to-definition.
    async fn get_definition(
        &self,
        path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<CodeLocation>>;

    /// Find all references.
    async fn get_references(
        &self,
        path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<CodeLocation>>;

    /// Get document symbols (outline).
    async fn get_document_symbols(&self, path: &Path) -> Result<Vec<DocSymbol>>;

    /// Get hover info.
    async fn get_hover(
        &self,
        path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<HoverInfo>>;

    /// Get diagnostics for a file.
    async fn get_diagnostics(&self, path: &Path) -> Result<Vec<LspDiagnostic>>;

    /// Rename a symbol.
    async fn rename_symbol(
        &self,
        path: &Path,
        line: u32,
        column: u32,
        new_name: &str,
    ) -> Result<()>;

    // ── Terminal / Command execution ─────────────────────────────

    /// Execute a shell command and return the output.
    async fn execute_command(
        &self,
        command: &str,
        working_dir: &Path,
    ) -> Result<CommandOutput>;

    // ── Git operations ───────────────────────────────────────────

    /// Get the current git status.
    async fn git_status(&self) -> Result<Vec<GitFileStatus>>;

    /// Get git log (recent commits).
    async fn git_log(&self, max_count: usize) -> Result<Vec<GitCommit>>;

    /// Stage files.
    async fn git_stage_files(&self, paths: &[PathBuf]) -> Result<()>;

    /// Create a commit.
    async fn git_commit(&self, message: &str) -> Result<String>;

    /// Create a lightweight tag (used for checkpointing).
    async fn git_create_tag(&self, name: &str) -> Result<()>;

    /// Delete a tag.
    async fn git_delete_tag(&self, name: &str) -> Result<()>;

    /// List tags matching a pattern.
    async fn git_list_tags(&self, pattern: &str) -> Result<Vec<String>>;

    /// Hard reset to a ref (tag or commit).
    async fn git_reset_hard(&self, target: &str) -> Result<()>;

    /// Get diff between two refs.
    async fn git_diff(&self, from: &str, to: &str) -> Result<String>;

    // ── Workspace info ───────────────────────────────────────────

    /// Get the workspace root directory.
    fn workspace_root(&self) -> &Path;
}
