use super::ToolResult;
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Max output size per command (chars). Prevents a single tool result from
/// blowing up the LLM context window (e.g., `cat` on a 2MB bundle).
const MAX_OUTPUT_CHARS: usize = 30_000;

/// Default command timeout in seconds. Prevents hung builds or `sleep`
/// from blocking the agent forever. Override per-call via `timeout_secs` param.
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Maximum allowed timeout (10 minutes).
const MAX_TIMEOUT_SECS: u64 = 600;

/// Execute a shell command with timeout protection.
pub async fn run(args: &Value, workdir: &Path) -> ToolResult {
    let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'command' parameter");
    };

    // ── Command sanitization ────────────────────────────────────
    // Intercept grep/find commands that should use the dedicated tools.
    let command = sanitize_command(command, workdir);

    // Allow per-call timeout override (clamped to MAX).
    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .min(MAX_TIMEOUT_SECS);

    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(&command)
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return ToolResult::err(format!("Failed to spawn: {e}")),
    };

    // Run the output collection with a timeout.
    let timeout_duration = std::time::Duration::from_secs(timeout_secs);

    match tokio::time::timeout(timeout_duration, collect_output(&mut child)).await {
        Ok((output, truncated, exit_code)) => {
            let mut result = output;
            if truncated {
                result.push_str(&format!(
                    "\n... (output truncated at {} chars. Use head/tail/grep to get specific parts)",
                    MAX_OUTPUT_CHARS
                ));
            }

            if exit_code == 0 {
                ToolResult::ok(format!("Exit code: 0\n{result}"))
            } else {
                ToolResult::err(format!("Exit code: {exit_code}\n{result}"))
            }
        }
        Err(_) => {
            // Timeout expired — kill the child process.
            let _ = child.kill().await;
            ToolResult::err(format!(
                "Command timed out after {}s. The process was killed.\n\
                 If this command needs more time, add \"timeout_secs\": {} (max {}) to the arguments.\n\
                 Command: {}",
                timeout_secs,
                timeout_secs * 2,
                MAX_TIMEOUT_SECS,
                truncate_cmd(&command, 200),
            ))
        }
    }
}

/// Collect stdout + stderr from a child process.
/// Returns (output_string, was_truncated, exit_code).
async fn collect_output(
    child: &mut tokio::process::Child,
) -> (String, bool, i32) {
    let mut output = String::new();
    let mut truncated = false;

    // Stream stdout
    if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if output.len() + line.len() > MAX_OUTPUT_CHARS {
                truncated = true;
                break;
            }
            output.push_str(&line);
            output.push('\n');
        }
    }

    // Stream stderr (only if we haven't already truncated)
    if !truncated {
        if let Some(stderr) = child.stderr.take() {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if output.len() + line.len() > MAX_OUTPUT_CHARS {
                    truncated = true;
                    break;
                }
                output.push_str(&line);
                output.push('\n');
            }
        }
    }

    let status = child.wait().await;
    let exit_code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);

    (output, truncated, exit_code)
}

fn truncate_cmd(cmd: &str, max: usize) -> &str {
    if cmd.len() <= max { cmd } else { &cmd[..max] }
}

/// Sanitize a shell command to prevent common performance and security pitfalls.
///
/// 1. Rewrites `grep -r/-R` to `rg` (ripgrep) — 10-100x faster, respects .gitignore
/// 2. Blocks `..` path escapes that would search outside the workspace
/// 3. Adds safety flags to grep if rg is unavailable
fn sanitize_command(command: &str, workdir: &Path) -> String {
    let trimmed = command.trim();

    // ── Block parent-directory escapes ────────────────────────
    // Commands like `grep -R foo ..` or `find ../` scan outside the workspace.
    // Replace `..` targets with `.` (current workspace).
    if (trimmed.starts_with("grep") || trimmed.starts_with("find"))
        && (trimmed.contains(" .. ") || trimmed.ends_with(" ..") || trimmed.contains(" ../"))
    {
        tracing::warn!(
            "⚠️  Blocked parent-dir escape in command: {}",
            truncate_cmd(trimmed, 100)
        );
        let fixed = trimmed
            .replace(" .. ", " . ")
            .replace(" ../", " ./");
        let fixed = if fixed.ends_with(" ..") {
            format!("{} .", &fixed[..fixed.len() - 2])
        } else {
            fixed
        };
        return sanitize_command(&fixed, workdir);
    }

    // ── Rewrite `grep -r` / `grep -R` to `rg` ───────────────
    // `grep -R` scans .git/, node_modules/, binaries — extremely slow on large repos.
    // `rg` respects .gitignore and skips binaries by default.
    if trimmed.starts_with("grep ") && 
       (trimmed.contains(" -r") || trimmed.contains(" -R") || 
        trimmed.contains("-rn") || trimmed.contains("-Rn") ||
        trimmed.contains("-rni") || trimmed.contains("-Rni"))
    {
        tracing::info!("🔄 Rewriting grep -R to rg for performance");
        
        // Extract the pattern and path from the grep command.
        // Common forms:
        //   grep -rn "pattern" .
        //   grep -R "pattern" --include="*.py" .
        //   grep -rni "pattern" path/
        // We'll do a best-effort rewrite to rg.
        let mut parts: Vec<&str> = trimmed.split_whitespace().collect();
        
        // Remove "grep" and build rg command
        if parts.is_empty() { return trimmed.to_string(); }
        parts.remove(0); // remove "grep"
        
        let mut rg_args = vec!["rg", "--line-number", "--no-heading", "--color=never", "--max-filesize=100K"];
        let mut pattern: Option<&str> = None;
        let mut search_path: Option<&str> = None;
        let mut i = 0;
        
        while i < parts.len() {
            let part = parts[i];
            if part.starts_with('-') {
                // Translate grep flags to rg flags
                if part.contains('i') && !part.contains("-include") {
                    rg_args.push("-i");
                }
                if part.contains('l') {
                    rg_args.push("-l");
                }
                if part.contains('c') && part.len() <= 4 {
                    rg_args.push("--count");
                }
                // Skip -r/-R/-n flags (rg has them by default)
                // Handle --include="*.py" → -g "*.py"
                if part.starts_with("--include=") {
                    let glob = part.trim_start_matches("--include=").trim_matches('"').trim_matches('\'');
                    rg_args.push("-g");
                    rg_args.push(glob);
                }
            } else if part.starts_with("--include") {
                // --include "*.py" (space-separated)
                if i + 1 < parts.len() {
                    i += 1;
                    let glob = parts[i].trim_matches('"').trim_matches('\'');
                    rg_args.push("-g");
                    rg_args.push(glob);
                }
            } else if pattern.is_none() {
                pattern = Some(part);
            } else {
                search_path = Some(part);
            }
            i += 1;
        }
        
        if let Some(pat) = pattern {
            rg_args.push(pat);
        }
        if let Some(path) = search_path {
            rg_args.push(path);
        }
        
        // Handle piped commands (e.g., `grep -R foo . | head`)
        let pipe_suffix = if let Some(pipe_idx) = trimmed.find(" | ") {
            &trimmed[pipe_idx..]
        } else {
            ""
        };
        
        let rewritten = format!("{}{}", rg_args.join(" "), pipe_suffix);
        tracing::info!("  Original: {}", truncate_cmd(trimmed, 100));
        tracing::info!("  Rewritten: {}", truncate_cmd(&rewritten, 100));
        return rewritten;
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ws() -> PathBuf { PathBuf::from("/tmp/test-workspace") }

    #[test]
    fn rewrites_grep_r_to_rg() {
        let cmd = sanitize_command(r#"grep -rn "Vision" ."#, &ws());
        assert!(cmd.starts_with("rg"), "Expected rg, got: {cmd}");
        assert!(cmd.contains("--line-number"));
        assert!(cmd.contains("\"Vision\""));
    }

    #[test]
    fn rewrites_grep_r_uppercase() {
        let cmd = sanitize_command(r#"grep -R "foo" ."#, &ws());
        assert!(cmd.starts_with("rg"), "Expected rg, got: {cmd}");
    }

    #[test]
    fn rewrites_grep_rni() {
        let cmd = sanitize_command(r#"grep -rni "pattern" src/"#, &ws());
        assert!(cmd.starts_with("rg"), "Expected rg, got: {cmd}");
        assert!(cmd.contains("-i"), "Should have -i flag: {cmd}");
    }

    #[test]
    fn preserves_pipe() {
        let cmd = sanitize_command(r#"grep -rn "foo" . | head -20"#, &ws());
        assert!(cmd.starts_with("rg"), "Expected rg, got: {cmd}");
        assert!(cmd.contains("| head -20"), "Should keep pipe: {cmd}");
    }

    #[test]
    fn translates_include_flag() {
        let cmd = sanitize_command(r#"grep -rn "foo" --include="*.py" ."#, &ws());
        assert!(cmd.contains("-g"), "Should have -g glob: {cmd}");
        assert!(cmd.contains("*.py"), "Should have *.py glob: {cmd}");
    }

    #[test]
    fn blocks_parent_dir_escape() {
        let cmd = sanitize_command(r#"grep -rn "Vision" .."#, &ws());
        // Should NOT contain ".." — should be rewritten to "."
        assert!(!cmd.contains(" .."), "Should block ..: {cmd}");
    }

    #[test]
    fn blocks_parent_dir_slash() {
        let cmd = sanitize_command(r#"grep -R "foo" ../sibling"#, &ws());
        assert!(!cmd.contains("../"), "Should block ../: {cmd}");
    }

    #[test]
    fn leaves_non_grep_commands_alone() {
        let cmd = sanitize_command("cargo check 2>&1 | tail -30", &ws());
        assert_eq!(cmd, "cargo check 2>&1 | tail -30");
    }

    #[test]
    fn leaves_git_grep_alone() {
        // git grep is safe — it only searches tracked files
        let cmd = sanitize_command("git grep -n 'foo'", &ws());
        assert!(cmd.starts_with("git"), "Should not rewrite git grep: {cmd}");
    }

    #[test]
    fn leaves_non_recursive_grep_alone() {
        // Plain `grep` without -r/-R is fine (single file or piped)
        let cmd = sanitize_command("grep 'pattern' file.txt", &ws());
        assert!(cmd.starts_with("grep"), "Should not rewrite non-recursive grep: {cmd}");
    }
}
