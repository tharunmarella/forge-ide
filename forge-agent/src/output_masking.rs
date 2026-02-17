//! Tool output masking, ported from gemini-cli's `toolOutputMaskingService.ts`.
//!
//! Provides smarter truncation than a simple character cap:
//! - Small outputs (< 4000 chars): returned as-is
//! - Medium outputs (4000-12000): head + tail preview
//! - Large outputs (> 12000): head + tail + save to temp file
//!
//! Shell output gets special treatment to always preserve exit codes and errors.

use std::path::PathBuf;

/// Thresholds
const SMALL_THRESHOLD: usize = 4_000;
const MEDIUM_THRESHOLD: usize = 12_000;
const HEAD_CHARS_MEDIUM: usize = 2_000;
const TAIL_CHARS_MEDIUM: usize = 1_000;
const HEAD_CHARS_LARGE: usize = 1_500;
const TAIL_CHARS_LARGE: usize = 500;

/// Directory for saved full outputs
const OUTPUT_SAVE_DIR: &str = "forge-ide/tool-outputs";

/// The kind of tool output, which affects masking behavior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputKind {
    /// Regular file content, search results, etc.
    Default,
    /// Shell command output -- always preserve exit code and error info.
    Shell,
    /// Display tools (show_code, show_diagram) -- never truncate, always preserve full content.
    Display,
}

/// Result of masking a tool output.
#[derive(Debug, Clone)]
pub struct MaskedOutput {
    /// The (possibly truncated) output to send to the LLM.
    pub text: String,
    /// If the output was saved to a file, the path is here.
    pub saved_path: Option<PathBuf>,
    /// Whether the output was truncated.
    pub was_truncated: bool,
}

/// Mask (truncate) a tool output intelligently.
///
/// Unlike the old `cap_output()`, this:
/// 1. Preserves both head AND tail of the output
/// 2. Shows total line count and char count
/// 3. Saves full output to disk for large results
/// 4. Special-cases shell output to preserve exit codes/errors
pub fn mask_output(output: &str, kind: OutputKind) -> MaskedOutput {
    let total_chars = output.len();
    let total_lines = output.lines().count();

    // Display output: never truncate (show_code, show_diagram need full content)
    if kind == OutputKind::Display {
        return MaskedOutput {
            text: output.to_string(),
            saved_path: None,
            was_truncated: false,
        };
    }

    // Small output: return as-is
    if total_chars <= SMALL_THRESHOLD {
        return MaskedOutput {
            text: output.to_string(),
            saved_path: None,
            was_truncated: false,
        };
    }

    // For shell output, extract and preserve structured info
    let (shell_prefix, body) = if kind == OutputKind::Shell {
        extract_shell_metadata(output)
    } else {
        (String::new(), output.to_string())
    };

    let body_chars = body.len();

    if total_chars <= MEDIUM_THRESHOLD {
        // Medium: head + tail preview
        let head_end = HEAD_CHARS_MEDIUM.min(body_chars);
        let tail_start = body_chars.saturating_sub(TAIL_CHARS_MEDIUM);

        let head = &body[..head_end];
        let tail = if tail_start > head_end {
            &body[tail_start..]
        } else {
            ""
        };

        let omitted = if tail_start > head_end {
            tail_start - head_end
        } else {
            0
        };

        let text = if !tail.is_empty() {
            format!(
                "{}{}\n\n[... {} chars omitted ({} total lines, {} total chars) ...]\n\n{}\n\n[Output truncated. Use read_file with line ranges for specific sections.]",
                shell_prefix, head, omitted, total_lines, total_chars, tail
            )
        } else {
            format!(
                "{}{}\n\n[Output truncated at {} chars ({} total lines, {} total chars). Use read_file with line ranges for specific sections.]",
                shell_prefix, head, total_chars, total_lines, total_chars
            )
        };

        MaskedOutput {
            text,
            saved_path: None,
            was_truncated: true,
        }
    } else {
        // Large: head + tail + save to file
        let head_end = HEAD_CHARS_LARGE.min(body_chars);
        let tail_start = body_chars.saturating_sub(TAIL_CHARS_LARGE);

        let head = &body[..head_end];
        let tail = if tail_start > head_end {
            &body[tail_start..]
        } else {
            ""
        };

        let omitted = if tail_start > head_end {
            tail_start - head_end
        } else {
            0
        };

        // Save full output to disk
        let saved_path = save_full_output(output);
        let save_notice = match &saved_path {
            Some(p) => format!("\nFull output saved to: {}", p.display()),
            None => String::new(),
        };

        let text = if !tail.is_empty() {
            format!(
                "{}{}\n\n[... {} chars omitted ({} total lines, {} total chars) ...]\n\n{}\n\n[Output truncated.{}]",
                shell_prefix, head, omitted, total_lines, total_chars, tail, save_notice
            )
        } else {
            format!(
                "{}{}\n\n[Output truncated at {} chars ({} total lines, {} total chars).{}]",
                shell_prefix, head, total_chars, total_lines, total_chars, save_notice
            )
        };

        MaskedOutput {
            text,
            saved_path,
            was_truncated: true,
        }
    }
}

/// Extract shell-specific metadata (exit code, errors) that must be preserved.
/// Returns (metadata_prefix, remaining_body).
fn extract_shell_metadata(output: &str) -> (String, String) {
    let mut prefix_lines = Vec::new();
    let mut body_lines = Vec::new();
    let mut in_metadata = true;

    for line in output.lines() {
        if in_metadata {
            let trimmed = line.trim().to_lowercase();
            // Preserve exit code, signal, error lines at the top
            if trimmed.starts_with("exit code:")
                || trimmed.starts_with("exit_code:")
                || trimmed.starts_with("exitcode:")
                || trimmed.starts_with("signal:")
                || trimmed.starts_with("error:")
                || trimmed.starts_with("stderr:")
                || trimmed.starts_with("status:")
            {
                prefix_lines.push(line.to_string());
                continue;
            }
            // After first non-metadata line, everything is body
            in_metadata = false;
        }
        body_lines.push(line.to_string());
    }

    let prefix = if prefix_lines.is_empty() {
        String::new()
    } else {
        format!("{}\n\n", prefix_lines.join("\n"))
    };

    (prefix, body_lines.join("\n"))
}

/// Save full output to a temporary file on disk.
/// Returns the path if successful.
fn save_full_output(output: &str) -> Option<PathBuf> {
    let base_dir = dirs::data_local_dir()?.join(OUTPUT_SAVE_DIR);

    if std::fs::create_dir_all(&base_dir).is_err() {
        return None;
    }

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let filename = format!("output_{}.txt", timestamp);
    let path = base_dir.join(&filename);

    match std::fs::write(&path, output) {
        Ok(_) => {
            // Clean up old files (keep max 20)
            cleanup_old_outputs(&base_dir, 20);
            Some(path)
        }
        Err(e) => {
            tracing::warn!("output_masking: failed to save full output: {}", e);
            None
        }
    }
}

/// Remove oldest output files if there are more than max_files.
fn cleanup_old_outputs(dir: &PathBuf, max_files: usize) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("output_")
        })
        .collect();

    if entries.len() <= max_files {
        return;
    }

    // Sort by modified time (oldest first)
    entries.sort_by(|a, b| {
        let time_a = a.metadata().and_then(|m| m.modified()).ok();
        let time_b = b.metadata().and_then(|m| m.modified()).ok();
        time_a.cmp(&time_b)
    });

    // Remove oldest
    let to_remove = entries.len() - max_files;
    for entry in entries.into_iter().take(to_remove) {
        let _ = std::fs::remove_file(entry.path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_output_unchanged() {
        let output = "Hello, world!";
        let result = mask_output(output, OutputKind::Default);
        assert_eq!(result.text, output);
        assert!(!result.was_truncated);
        assert!(result.saved_path.is_none());
    }

    #[test]
    fn test_medium_output_has_head_and_tail() {
        let output = "x".repeat(6000);
        let result = mask_output(&output, OutputKind::Default);
        assert!(result.was_truncated);
        assert!(result.text.contains("chars omitted"));
        assert!(result.text.len() < output.len());
    }

    #[test]
    fn test_large_output_saved_to_disk() {
        let output = "y".repeat(15000);
        let result = mask_output(&output, OutputKind::Default);
        assert!(result.was_truncated);
        assert!(result.text.contains("truncated"));
        // saved_path may or may not exist depending on disk permissions in CI
    }

    #[test]
    fn test_shell_metadata_preserved() {
        let output = "exit code: 1\nerror: compilation failed\nsome output here\nmore output\n"
            .to_string()
            + &"x".repeat(5000);
        let result = mask_output(&output, OutputKind::Shell);
        assert!(result.text.starts_with("exit code: 1\nerror: compilation failed"));
    }
}
