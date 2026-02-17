//! Git source control tools - essential operations only.
//!
//! This provides a single unified git tool with the most commonly needed operations:
//! - status: Check repo status (staged, unstaged, untracked files)
//! - stage: Stage files for commit
//! - commit: Commit staged changes
//! - push: Push commits to remote
//! - pull: Pull changes from remote
//! - branch: List, create, or switch branches
//! - log: View commit history
//! - diff: View changes in files

use serde_json::Value;
use crate::tools::ToolResult;

/// Unified git tool for essential source control operations.
///
/// Supports multiple git operations through an "operation" parameter:
/// - status: Get current repo status
/// - stage: Stage files (paths parameter)
/// - unstage: Unstage files (paths parameter)
/// - commit: Commit with message (message parameter)
/// - push: Push to remote
/// - pull: Pull from remote
/// - branch: Branch operations (action: list/create/switch, name parameter)
/// - log: View commit history (limit parameter)
/// - diff: View file changes (path parameter, staged: bool)
///
/// This integrates with the IDE's native git system for proper UI updates.
pub async fn git(args: &Value, _workdir: &std::path::Path) -> ToolResult {
    let operation = args.get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    
    if operation.is_empty() {
        return ToolResult::err(
            "Missing 'operation' parameter. Valid operations: status, stage, unstage, commit, push, pull, branch, log, diff"
        );
    }
    
    // Mark as pending IDE execution - the IDE will handle these through RPC
    ToolResult::ok("PENDING_IDE_EXECUTION")
}
