//! AI Diff Preview — inline diff display with per-hunk accept/reject.
//!
//! When the agent proposes file edits, instead of writing directly to disk,
//! the proxy sends `CoreNotification::AgentDiffPreview` with old/new content
//! and computed hunks. This module stores and manages those pending diffs.

use std::{collections::HashMap, path::PathBuf, rc::Rc};

use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use lapce_rpc::core::AiDiffHunk;

// ── Hunk status ─────────────────────────────────────────────────

/// Status of an individual hunk within a pending diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkStatus {
    /// Not yet decided by the user.
    Pending,
    /// User accepted this hunk.
    Accepted,
    /// User rejected this hunk.
    Rejected,
}

// ── A single pending diff ───────────────────────────────────────

/// A file-level diff proposed by the AI agent.
#[derive(Debug, Clone)]
pub struct PendingDiff {
    /// Unique id for this diff (matches `CoreNotification::AgentDiffPreview::diff_id`).
    pub diff_id: String,
    /// The tool call that produced this edit.
    pub tool_call_id: String,
    /// Relative workspace path.
    pub file_path: String,
    /// Original content of the file (empty for new files).
    pub old_content: String,
    /// Proposed new content.
    pub new_content: String,
    /// Parsed hunks with their accept/reject status.
    pub hunks: Vec<(AiDiffHunk, HunkStatus)>,
}

impl PendingDiff {
    pub fn new(
        diff_id: String,
        tool_call_id: String,
        file_path: String,
        old_content: String,
        new_content: String,
        hunks: Vec<AiDiffHunk>,
    ) -> Self {
        let hunks = hunks
            .into_iter()
            .map(|h| (h, HunkStatus::Pending))
            .collect();
        Self {
            diff_id,
            tool_call_id,
            file_path,
            old_content,
            new_content,
            hunks,
        }
    }

    /// Returns true if all hunks have been resolved (accepted or rejected).
    pub fn is_fully_resolved(&self) -> bool {
        self.hunks
            .iter()
            .all(|(_, s)| *s != HunkStatus::Pending)
    }

    /// Accept a single hunk by index.
    pub fn accept_hunk(&mut self, index: usize) {
        if let Some((_, status)) = self.hunks.get_mut(index) {
            *status = HunkStatus::Accepted;
        }
    }

    /// Reject a single hunk by index.
    pub fn reject_hunk(&mut self, index: usize) {
        if let Some((_, status)) = self.hunks.get_mut(index) {
            *status = HunkStatus::Rejected;
        }
    }

    /// Accept all pending hunks.
    pub fn accept_all(&mut self) {
        for (_, status) in &mut self.hunks {
            if *status == HunkStatus::Pending {
                *status = HunkStatus::Accepted;
            }
        }
    }

    /// Reject all pending hunks.
    pub fn reject_all(&mut self) {
        for (_, status) in &mut self.hunks {
            if *status == HunkStatus::Pending {
                *status = HunkStatus::Rejected;
            }
        }
    }

    /// Build the final file content by applying only accepted hunks.
    /// Rejected and pending hunks keep the original content.
    pub fn build_resolved_content(&self) -> String {
        let old_lines: Vec<&str> = if self.old_content.is_empty() {
            Vec::new()
        } else {
            self.old_content.lines().collect()
        };
        let new_lines: Vec<&str> = self.new_content.lines().collect();

        // If no hunks, just check if file is new
        if self.hunks.is_empty() {
            // No hunks but there's new content — treat as full-file accept/reject
            return self.old_content.clone();
        }

        let mut result = Vec::new();
        let mut old_cursor: usize = 0; // current line in old content

        for (hunk, status) in &self.hunks {
            // Copy unchanged lines from old content up to this hunk
            while old_cursor < hunk.old_start && old_cursor < old_lines.len() {
                result.push(old_lines[old_cursor]);
                old_cursor += 1;
            }

            match status {
                HunkStatus::Accepted => {
                    // Use new content for this hunk
                    let new_end =
                        (hunk.new_start + hunk.new_lines).min(new_lines.len());
                    for i in hunk.new_start..new_end {
                        result.push(new_lines[i]);
                    }
                    // Skip old lines
                    old_cursor = (hunk.old_start + hunk.old_lines).min(old_lines.len());
                }
                HunkStatus::Rejected | HunkStatus::Pending => {
                    // Keep old content for this hunk
                    let old_end =
                        (hunk.old_start + hunk.old_lines).min(old_lines.len());
                    for i in hunk.old_start..old_end {
                        result.push(old_lines[i]);
                    }
                    old_cursor = old_end;
                }
            }
        }

        // Copy remaining old lines after the last hunk
        while old_cursor < old_lines.len() {
            result.push(old_lines[old_cursor]);
            old_cursor += 1;
        }

        let mut out = result.join("\n");
        // Preserve trailing newline if the new content had one
        if self.new_content.ends_with('\n') || self.old_content.ends_with('\n') {
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }

    /// Returns lines that are additions (green) for rendering.
    /// Each entry is (line_number_in_new_content, line_text).
    pub fn added_lines(&self) -> Vec<(usize, String)> {
        let new_lines: Vec<&str> = self.new_content.lines().collect();
        let mut result = Vec::new();
        for (hunk, status) in &self.hunks {
            if *status == HunkStatus::Rejected {
                continue;
            }
            let new_end = (hunk.new_start + hunk.new_lines).min(new_lines.len());
            for i in hunk.new_start..new_end {
                result.push((i, new_lines[i].to_string()));
            }
        }
        result
    }

    /// Returns lines that are deletions (red) for rendering.
    /// Each entry is (line_number_in_old_content, line_text).
    pub fn deleted_lines(&self) -> Vec<(usize, String)> {
        let old_lines: Vec<&str> = self.old_content.lines().collect();
        let mut result = Vec::new();
        for (hunk, status) in &self.hunks {
            if *status == HunkStatus::Rejected {
                continue;
            }
            let old_end = (hunk.old_start + hunk.old_lines).min(old_lines.len());
            for i in hunk.old_start..old_end {
                result.push((i, old_lines[i].to_string()));
            }
        }
        result
    }
}

// ── Store for all pending diffs ─────────────────────────────────

/// Manages all pending AI diffs across the workspace.
#[derive(Clone, Debug)]
pub struct AiDiffStore {
    /// All pending diffs, keyed by diff_id.
    pub diffs: RwSignal<HashMap<String, PendingDiff>>,
    /// Whether there are any pending diffs (for UI indicators).
    pub has_pending: RwSignal<bool>,
    /// Counter for UI reactivity — incremented on any change.
    pub version: RwSignal<u64>,
}

impl AiDiffStore {
    pub fn new(cx: Scope) -> Self {
        Self {
            diffs: cx.create_rw_signal(HashMap::new()),
            has_pending: cx.create_rw_signal(false),
            version: cx.create_rw_signal(0),
        }
    }

    /// Add a new pending diff from an `AgentDiffPreview` notification.
    pub fn add_diff(&self, diff: PendingDiff) {
        self.diffs.update(|d| {
            d.insert(diff.diff_id.clone(), diff);
        });
        self.has_pending.set(true);
        self.version.update(|v| *v += 1);
    }

    /// Get pending diffs for a specific file path.
    pub fn diffs_for_file(&self, file_path: &str) -> Vec<PendingDiff> {
        self.diffs.with_untracked(|d| {
            d.values()
                .filter(|diff| diff.file_path == file_path)
                .cloned()
                .collect()
        })
    }

    /// Accept all hunks in a specific diff.
    pub fn accept_diff(&self, diff_id: &str) -> Option<PendingDiff> {
        let mut resolved = None;
        self.diffs.update(|d| {
            if let Some(diff) = d.get_mut(diff_id) {
                diff.accept_all();
                resolved = Some(diff.clone());
            }
        });
        self.update_has_pending();
        self.version.update(|v| *v += 1);
        resolved
    }

    /// Reject all hunks in a specific diff.
    pub fn reject_diff(&self, diff_id: &str) {
        self.diffs.update(|d| {
            d.remove(diff_id);
        });
        self.update_has_pending();
        self.version.update(|v| *v += 1);
    }

    /// Accept a single hunk within a diff.
    pub fn accept_hunk(&self, diff_id: &str, hunk_index: usize) {
        self.diffs.update(|d| {
            if let Some(diff) = d.get_mut(diff_id) {
                diff.accept_hunk(hunk_index);
            }
        });
        self.version.update(|v| *v += 1);
    }

    /// Reject a single hunk within a diff.
    pub fn reject_hunk(&self, diff_id: &str, hunk_index: usize) {
        self.diffs.update(|d| {
            if let Some(diff) = d.get_mut(diff_id) {
                diff.reject_hunk(hunk_index);
            }
        });
        self.version.update(|v| *v += 1);
    }

    /// Accept all pending diffs.
    pub fn accept_all(&self) -> Vec<PendingDiff> {
        let mut resolved = Vec::new();
        self.diffs.update(|d| {
            for diff in d.values_mut() {
                diff.accept_all();
                resolved.push(diff.clone());
            }
        });
        self.update_has_pending();
        self.version.update(|v| *v += 1);
        resolved
    }

    /// Reject all pending diffs.
    pub fn reject_all(&self) {
        self.diffs.update(|d| d.clear());
        self.has_pending.set(false);
        self.version.update(|v| *v += 1);
    }

    /// Remove a fully resolved diff from the store.
    pub fn remove_diff(&self, diff_id: &str) {
        self.diffs.update(|d| {
            d.remove(diff_id);
        });
        self.update_has_pending();
        self.version.update(|v| *v += 1);
    }

    fn update_has_pending(&self) {
        let has = self.diffs.with_untracked(|d| !d.is_empty());
        self.has_pending.set(has);
    }
}

// ── Diff computation helpers ────────────────────────────────────

/// Compute unified-diff-style hunks between old and new content.
/// Returns a list of `AiDiffHunk` suitable for the RPC protocol.
pub fn compute_hunks(old_content: &str, new_content: &str) -> Vec<AiDiffHunk> {
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    // Simple LCS-based diff to find changed regions
    let lcs = lcs_diff(&old_lines, &new_lines);
    collapse_into_hunks(&lcs, old_lines.len(), new_lines.len())
}

/// Edit operation from LCS diff.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DiffOp {
    Keep,
    Insert,
    Delete,
}

/// Compute diff operations using a simple LCS approach.
fn lcs_diff(old: &[&str], new: &[&str]) -> Vec<DiffOp> {
    let m = old.len();
    let n = new.len();

    // Build LCS table
    let mut table = vec![vec![0u32; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if old[i - 1] == new[j - 1] {
                table[i][j] = table[i - 1][j - 1] + 1;
            } else {
                table[i][j] = table[i - 1][j].max(table[i][j - 1]);
            }
        }
    }

    // Backtrack to produce edit script
    let mut ops = Vec::new();
    let mut i = m;
    let mut j = n;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old[i - 1] == new[j - 1] {
            ops.push(DiffOp::Keep);
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || table[i][j - 1] >= table[i - 1][j]) {
            ops.push(DiffOp::Insert);
            j -= 1;
        } else {
            ops.push(DiffOp::Delete);
            i -= 1;
        }
    }
    ops.reverse();
    ops
}

/// Collapse a sequence of DiffOps into hunks with 3 lines of context.
fn collapse_into_hunks(ops: &[DiffOp], old_len: usize, new_len: usize) -> Vec<AiDiffHunk> {
    const CONTEXT: usize = 3;

    // Find changed regions
    let mut changes: Vec<(usize, usize, usize, usize)> = Vec::new(); // (old_start, old_count, new_start, new_count)
    let mut old_pos = 0usize;
    let mut new_pos = 0usize;
    let mut i = 0;

    while i < ops.len() {
        // Skip keeps
        if ops[i] == DiffOp::Keep {
            old_pos += 1;
            new_pos += 1;
            i += 1;
            continue;
        }

        // Found a change — collect contiguous changes
        let change_old_start = old_pos;
        let change_new_start = new_pos;
        while i < ops.len() && ops[i] != DiffOp::Keep {
            match ops[i] {
                DiffOp::Delete => old_pos += 1,
                DiffOp::Insert => new_pos += 1,
                _ => {}
            }
            i += 1;
        }
        changes.push((
            change_old_start,
            old_pos - change_old_start,
            change_new_start,
            new_pos - change_new_start,
        ));
    }

    // Merge nearby changes into hunks with context
    let mut hunks = Vec::new();
    let mut idx = 0;
    while idx < changes.len() {
        let (mut old_start, mut old_count, mut new_start, mut new_count) = changes[idx];

        // Add leading context
        let ctx_before = old_start.min(CONTEXT);
        old_start -= ctx_before;
        new_start -= ctx_before;
        old_count += ctx_before;
        new_count += ctx_before;

        // Try to merge with subsequent changes if they're within 2*CONTEXT lines
        while idx + 1 < changes.len() {
            let (next_os, next_oc, next_ns, next_nc) = changes[idx + 1];
            let gap = next_os.saturating_sub(old_start + old_count);
            if gap <= 2 * CONTEXT {
                // Merge
                let new_old_end = next_os + next_oc;
                let new_new_end = next_ns + next_nc;
                old_count = new_old_end - old_start;
                new_count = new_new_end - new_start;
                idx += 1;
            } else {
                break;
            }
        }

        // Add trailing context
        let ctx_after = (old_len - (old_start + old_count)).min(CONTEXT);
        old_count += ctx_after;
        new_count += ctx_after;

        hunks.push(AiDiffHunk {
            old_start,
            old_lines: old_count,
            new_start,
            new_lines: new_count,
        });

        idx += 1;
    }

    hunks
}
