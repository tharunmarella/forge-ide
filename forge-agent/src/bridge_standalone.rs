//! Standalone implementation of `ProxyBridge`.
//!
//! This implementation uses direct filesystem, process, and git CLI
//! operations.  It works immediately without needing the full proxy
//! RPC infrastructure, making it suitable for:
//!  - Initial integration testing
//!  - Headless / CI usage
//!  - Fallback when proxy is not available
//!
//! The IDE will eventually replace this with a `ProxyBridgeImpl` that
//! routes through `lapce-proxy` RPC for tighter integration with the
//! running editor (open buffers, active LSP, etc.).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;

use crate::bridge::*;

/// A bridge implementation that directly talks to the OS.
pub struct StandaloneBridge {
    workspace_root: PathBuf,
}

impl StandaloneBridge {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }
}

#[async_trait]
impl ProxyBridge for StandaloneBridge {
    // ── File operations ───────────────────────────────────────────

    async fn read_file(&self, path: &Path) -> Result<String> {
        tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read {}", path.display()))
    }

    async fn write_file(&self, path: &Path, contents: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, contents)
            .await
            .with_context(|| format!("Failed to write {}", path.display()))
    }

    async fn create_dir(&self, path: &Path) -> Result<()> {
        tokio::fs::create_dir_all(path)
            .await
            .with_context(|| format!("Failed to create dir {}", path.display()))
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(path).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            entries.push(DirEntry {
                path: entry.path(),
                is_dir: metadata.is_dir(),
            });
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    async fn delete_path(&self, path: &Path) -> Result<()> {
        if path.is_dir() {
            tokio::fs::remove_dir_all(path).await?;
        } else {
            tokio::fs::remove_file(path).await?;
        }
        Ok(())
    }

    async fn rename_path(&self, from: &Path, to: &Path) -> Result<()> {
        tokio::fs::rename(from, to).await?;
        Ok(())
    }

    // ── Search ────────────────────────────────────────────────────

    async fn global_search(
        &self,
        pattern: &str,
        path: &Path,
        case_sensitive: bool,
        _whole_word: bool,
        max_results: usize,
    ) -> Result<Vec<SearchMatch>> {
        // Use `rg` (ripgrep) for search -- it respects .gitignore
        let mut cmd = Command::new("rg");
        cmd.arg("--line-number")
            .arg("--no-heading")
            .arg("--color=never")
            .arg("--max-count")
            .arg(max_results.to_string());

        if !case_sensitive {
            cmd.arg("-i");
        }

        cmd.arg(pattern).arg(path);

        let output = cmd.output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut matches = Vec::new();
        for line in stdout.lines().take(max_results) {
            // rg output: file:line:content
            let mut parts = line.splitn(3, ':');
            if let (Some(file), Some(line_num), Some(content)) =
                (parts.next(), parts.next(), parts.next())
            {
                if let Ok(ln) = line_num.parse::<usize>() {
                    matches.push(SearchMatch {
                        path: PathBuf::from(file),
                        line_number: ln,
                        line_content: content.to_string(),
                    });
                }
            }
        }

        Ok(matches)
    }

    // ── LSP / Code intelligence ──────────────────────────────────
    // These return empty for now -- will be wired to proxy LSP later.

    async fn get_definition(
        &self,
        _path: &Path,
        _line: u32,
        _column: u32,
    ) -> Result<Vec<CodeLocation>> {
        // TODO: Wire to proxy LSP
        Ok(Vec::new())
    }

    async fn get_references(
        &self,
        _path: &Path,
        _line: u32,
        _column: u32,
    ) -> Result<Vec<CodeLocation>> {
        // TODO: Wire to proxy LSP
        Ok(Vec::new())
    }

    async fn get_document_symbols(&self, _path: &Path) -> Result<Vec<DocSymbol>> {
        // TODO: Wire to proxy LSP
        Ok(Vec::new())
    }

    async fn get_hover(
        &self,
        _path: &Path,
        _line: u32,
        _column: u32,
    ) -> Result<Option<HoverInfo>> {
        // TODO: Wire to proxy LSP
        Ok(None)
    }

    async fn get_diagnostics(&self, _path: &Path) -> Result<Vec<LspDiagnostic>> {
        // TODO: Wire to proxy LSP
        Ok(Vec::new())
    }

    async fn rename_symbol(
        &self,
        _path: &Path,
        _line: u32,
        _column: u32,
        _new_name: &str,
    ) -> Result<()> {
        // TODO: Wire to proxy LSP
        Ok(())
    }

    // ── Terminal / Command execution ─────────────────────────────

    async fn execute_command(
        &self,
        command: &str,
        working_dir: &Path,
    ) -> Result<CommandOutput> {
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(working_dir)
            .output()
            .await
            .with_context(|| format!("Failed to execute: {command}"))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }

    // ── Git operations ───────────────────────────────────────────

    async fn git_status(&self) -> Result<Vec<GitFileStatus>> {
        let output = Command::new("git")
            .args(["status", "--porcelain=v1"])
            .current_dir(&self.workspace_root)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let statuses = stdout
            .lines()
            .filter_map(|line| {
                if line.len() < 4 {
                    return None;
                }
                let status_code = &line[..2];
                let path = line[3..].trim().to_string();
                let status = match status_code.trim() {
                    "M" => "modified",
                    "A" => "added",
                    "D" => "deleted",
                    "R" => "renamed",
                    "??" => "untracked",
                    _ => "unknown",
                };
                Some(GitFileStatus {
                    path: PathBuf::from(path),
                    status: status.to_string(),
                })
            })
            .collect();
        Ok(statuses)
    }

    async fn git_log(&self, max_count: usize) -> Result<Vec<GitCommit>> {
        let output = Command::new("git")
            .args([
                "log",
                &format!("--max-count={max_count}"),
                "--format=%H%n%s%n%an%n%at",
            ])
            .current_dir(&self.workspace_root)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        let mut commits = Vec::new();

        for chunk in lines.chunks(4) {
            if chunk.len() == 4 {
                commits.push(GitCommit {
                    hash: chunk[0].to_string(),
                    message: chunk[1].to_string(),
                    author: chunk[2].to_string(),
                    timestamp: chunk[3].parse().unwrap_or(0),
                });
            }
        }

        Ok(commits)
    }

    async fn git_stage_files(&self, paths: &[PathBuf]) -> Result<()> {
        let path_strs: Vec<&str> = paths.iter().filter_map(|p| p.to_str()).collect();
        if path_strs.is_empty() {
            return Ok(());
        }
        let mut cmd = Command::new("git");
        cmd.arg("add").args(&path_strs).current_dir(&self.workspace_root);
        cmd.output().await?;
        Ok(())
    }

    async fn git_commit(&self, message: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&self.workspace_root)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.to_string())
    }

    async fn git_create_tag(&self, name: &str) -> Result<()> {
        Command::new("git")
            .args(["tag", name])
            .current_dir(&self.workspace_root)
            .output()
            .await?;
        Ok(())
    }

    async fn git_delete_tag(&self, name: &str) -> Result<()> {
        Command::new("git")
            .args(["tag", "-d", name])
            .current_dir(&self.workspace_root)
            .output()
            .await?;
        Ok(())
    }

    async fn git_list_tags(&self, pattern: &str) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["tag", "-l", pattern])
            .current_dir(&self.workspace_root)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().map(|s| s.to_string()).collect())
    }

    async fn git_reset_hard(&self, target: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["reset", "--hard", target])
            .current_dir(&self.workspace_root)
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "git reset --hard failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    async fn git_diff(&self, from: &str, to: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", from, to])
            .current_dir(&self.workspace_root)
            .output()
            .await?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    // ── Workspace info ───────────────────────────────────────────

    fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
}
