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
pub async fn git(args: &Value, workdir: &std::path::Path) -> ToolResult {
    let operation = args.get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    
    if operation.is_empty() {
        return ToolResult::err(
            "Missing 'operation' parameter. Valid operations: status, stage, unstage, commit, push, pull, branch, log, diff"
        );
    }
    
    // Build the git command based on operation
    let git_cmd = match operation {
        "status" => "git status".to_string(),
        "stage" => {
            let paths = args.get("paths")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            
            if paths.is_empty() {
                return ToolResult::err("'stage' operation requires 'paths' parameter");
            }
            format!("git add {}", paths)
        }
        "unstage" => {
            let paths = args.get("paths")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            
            if paths.is_empty() {
                return ToolResult::err("'unstage' operation requires 'paths' parameter");
            }
            format!("git reset HEAD {}", paths)
        }
        "commit" => {
            let message = args.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            
            if message.is_empty() {
                return ToolResult::err("'commit' operation requires 'message' parameter");
            }
            format!("git commit -m '{}'", message.replace("'", "'\\''"))
        }
        "push" => "git push".to_string(),
        "pull" => "git pull".to_string(),
        "branch" => {
            let action = args.get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("list");
            
            match action {
                "list" => "git branch -a".to_string(),
                "create" => {
                    let name = args.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if name.is_empty() {
                        return ToolResult::err("'branch create' requires 'name' parameter");
                    }
                    format!("git branch {}", name)
                }
                "switch" => {
                    let name = args.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if name.is_empty() {
                        return ToolResult::err("'branch switch' requires 'name' parameter");
                    }
                    format!("git checkout {}", name)
                }
                _ => return ToolResult::err(&format!("Invalid branch action '{}'. Valid: list, create, switch", action)),
            }
        }
        "log" => {
            let limit = args.get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10);
            format!("git log -n {} --oneline", limit)
        }
        "diff" => {
            let path = args.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let staged = args.get("staged")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            
            if staged {
                format!("git diff --staged {}", path)
            } else {
                format!("git diff {}", path)
            }
        }
        _ => return ToolResult::err(&format!("Unknown operation '{}'. Valid: status, stage, unstage, commit, push, pull, branch, log, diff", operation)),
    };
    
    // Execute the git command
    use tokio::process::Command;
    
    let output = match Command::new("sh")
        .arg("-c")
        .arg(&git_cmd)
        .current_dir(workdir)
        .output()
        .await
    {
        Ok(output) => output,
        Err(e) => return ToolResult::err(&format!("Failed to execute git command: {}", e)),
    };
    
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        
        let result = if !stdout.is_empty() {
            stdout.to_string()
        } else if !stderr.is_empty() {
            stderr.to_string()
        } else {
            format!("✓ {} completed successfully", operation)
        };
        
        ToolResult::ok(result)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        ToolResult::err(&format!("Git {} failed: {}", operation, stderr))
    }
}
