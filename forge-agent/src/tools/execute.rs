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

    // Allow per-call timeout override (clamped to MAX).
    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .min(MAX_TIMEOUT_SECS);

    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(command)
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
                truncate_cmd(command, 200),
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
