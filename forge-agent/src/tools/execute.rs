use super::ToolResult;
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Max output size per command (chars). Prevents a single tool result from
/// blowing up the LLM context window (e.g., `cat` on a 2MB bundle).
const MAX_OUTPUT_CHARS: usize = 30_000;

/// Execute a shell command
pub async fn run(args: &Value, workdir: &Path) -> ToolResult {
    let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'command' parameter");
    };

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

    if truncated {
        output.push_str(&format!(
            "\n... (output truncated at {} chars. Use head/tail/grep to get specific parts)",
            MAX_OUTPUT_CHARS
        ));
    }

    if exit_code == 0 {
        ToolResult::ok(format!("Exit code: 0\n{output}"))
    } else {
        ToolResult::err(format!("Exit code: {exit_code}\n{output}"))
    }
}
