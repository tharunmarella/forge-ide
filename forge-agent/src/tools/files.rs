use super::ToolResult;
use serde_json::Value;
use std::path::Path;
use tokio::fs;
use walkdir::WalkDir;

// ══════════════════════════════════════════════════════════════════
//  MULTI-STRATEGY EDIT MATCHING
//  Ported from gemini-cli: tries Exact -> Flexible -> Regex
// ══════════════════════════════════════════════════════════════════

/// Which replacement strategy succeeded.
#[derive(Debug, Clone, Copy)]
enum MatchStrategy {
    Exact,
    Flexible,
    Regex,
}

impl std::fmt::Display for MatchStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchStrategy::Exact => write!(f, "exact"),
            MatchStrategy::Flexible => write!(f, "flexible"),
            MatchStrategy::Regex => write!(f, "regex"),
        }
    }
}

/// Result of a successful replacement.
struct ReplacementResult {
    new_content: String,
    #[allow(dead_code)]
    occurrences: usize,
    strategy: MatchStrategy,
}

/// Try all 3 strategies in order: exact -> flexible -> regex.
/// Returns None if no strategy matched.
fn try_replace(content: &str, old_str: &str, new_str: &str) -> Option<ReplacementResult> {
    // Strategy 1: Exact match
    if let Some(r) = try_exact_replace(content, old_str, new_str) {
        return Some(r);
    }
    // Strategy 2: Flexible (whitespace-tolerant) match
    if let Some(r) = try_flexible_replace(content, old_str, new_str) {
        return Some(r);
    }
    // Strategy 3: Regex-based flexible match
    if let Some(r) = try_regex_replace(content, old_str, new_str) {
        return Some(r);
    }
    None
}

/// Strategy 1: Exact literal match (current behavior).
fn try_exact_replace(content: &str, old_str: &str, new_str: &str) -> Option<ReplacementResult> {
    let count = content.matches(old_str).count();
    if count == 1 {
        Some(ReplacementResult {
            new_content: content.replacen(old_str, new_str, 1),
            occurrences: 1,
            strategy: MatchStrategy::Exact,
        })
    } else if count > 1 {
        // Multiple matches -- can't use exact, but return None to try other strategies
        // (caller handles the "multiple matches" error separately)
        None
    } else {
        None
    }
}

/// Strategy 2: Flexible whitespace-tolerant match.
/// Strips leading whitespace from each line, matches by trimmed content,
/// then applies replacement preserving original indentation.
fn try_flexible_replace(content: &str, old_str: &str, new_str: &str) -> Option<ReplacementResult> {
    // Split source into lines preserving line endings
    let source_lines: Vec<&str> = content.lines().collect();
    let search_lines: Vec<&str> = old_str.lines().collect();
    let replace_lines: Vec<&str> = new_str.lines().collect();

    if search_lines.is_empty() {
        return None;
    }

    let search_stripped: Vec<&str> = search_lines.iter().map(|l| l.trim()).collect();

    let mut occurrences = 0;
    let mut match_positions: Vec<usize> = Vec::new();

    // Slide a window over source lines
    if source_lines.len() >= search_stripped.len() {
        for i in 0..=(source_lines.len() - search_stripped.len()) {
            let window_stripped: Vec<&str> = source_lines[i..i + search_stripped.len()]
                .iter()
                .map(|l| l.trim())
                .collect();
            if window_stripped == search_stripped {
                occurrences += 1;
                match_positions.push(i);
            }
        }
    }

    if occurrences != 1 {
        return None; // 0 or multiple matches
    }

    let pos = match_positions[0];

    // Detect indentation from the first line of the match
    let first_match_line = source_lines[pos];
    let indentation = &first_match_line[..first_match_line.len() - first_match_line.trim_start().len()];

    // Build replacement with original indentation
    let indented_replacement: Vec<String> = replace_lines
        .iter()
        .enumerate()
        .map(|(j, line)| {
            if j == 0 && line.trim().is_empty() {
                line.to_string()
            } else {
                format!("{}{}", indentation, line.trim_start())
            }
        })
        .collect();

    // Reconstruct content
    let mut new_lines: Vec<String> = Vec::with_capacity(source_lines.len());
    for line in &source_lines[..pos] {
        new_lines.push(line.to_string());
    }
    for line in &indented_replacement {
        new_lines.push(line.clone());
    }
    for line in &source_lines[pos + search_stripped.len()..] {
        new_lines.push(line.to_string());
    }

    // Preserve trailing newline
    let had_trailing_newline = content.ends_with('\n');
    let mut new_content = new_lines.join("\n");
    if had_trailing_newline && !new_content.ends_with('\n') {
        new_content.push('\n');
    }

    Some(ReplacementResult {
        new_content,
        occurrences: 1,
        strategy: MatchStrategy::Flexible,
    })
}

/// Strategy 3: Regex-based flexible match.
/// Tokenizes old_str around delimiters, joins with \s*, matches with regex.
fn try_regex_replace(content: &str, old_str: &str, new_str: &str) -> Option<ReplacementResult> {
    let delimiters = ['(', ')', ':', '[', ']', '{', '}', '>', '<', '='];

    // Tokenize: split around delimiters by inserting spaces around them
    let mut processed = old_str.to_string();
    for delim in &delimiters {
        processed = processed.replace(*delim, &format!(" {} ", delim));
    }

    // Split by whitespace and filter empties
    let tokens: Vec<&str> = processed.split_whitespace().filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return None;
    }

    // Escape each token for regex and join with \s*
    let escaped: Vec<String> = tokens.iter().map(|t| regex::escape(t)).collect();
    let pattern = format!(r"(?m)^(\s*){}", escaped.join(r"\s*"));

    let re = match regex::Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return None,
    };

    let mat = re.find(content)?;

    // Extract indentation from the match
    let matched_text = mat.as_str();
    let indentation = &matched_text[..matched_text.len() - matched_text.trim_start().len()];

    // Build replacement with indentation
    let replace_lines: Vec<&str> = new_str.lines().collect();
    let indented: Vec<String> = replace_lines
        .iter()
        .map(|line| format!("{}{}", indentation, line))
        .collect();
    let replacement = indented.join("\n");

    // Replace only the first occurrence
    let new_content = format!(
        "{}{}{}",
        &content[..mat.start()],
        replacement,
        &content[mat.end()..],
    );

    Some(ReplacementResult {
        new_content,
        occurrences: 1,
        strategy: MatchStrategy::Regex,
    })
}

/// Read file contents
pub async fn read(args: &Value, workdir: &Path) -> ToolResult {
    let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'path' parameter");
    };

    let full_path = workdir.join(path);
    
    let content = match fs::read_to_string(&full_path).await {
        Ok(c) => c,
        Err(e) => return ToolResult::err(format!("Failed to read {path}: {e}")),
    };

    // Handle line range
    let start = args.get("start_line").and_then(|v| v.as_u64()).map(|n| n as usize);
    let end = args.get("end_line").and_then(|v| v.as_u64()).map(|n| n as usize);

    if start.is_some() || end.is_some() {
        let lines: Vec<&str> = content.lines().collect();
        let start = start.unwrap_or(1).saturating_sub(1);
        let end = end.unwrap_or(lines.len()).min(lines.len());
        
        let output: String = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:4}|{}", start + i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");
        
        ToolResult::ok(output)
    } else {
        // Add line numbers, truncate very large files to prevent context window overflow
        const MAX_READ_LINES: usize = 500;
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        let truncated = total_lines > MAX_READ_LINES;
        let display_lines = if truncated { MAX_READ_LINES } else { total_lines };
        
        let mut output: String = lines[..display_lines]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:4}|{}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");
        
        if truncated {
            output.push_str(&format!(
                "\n\n... ({} more lines not shown. Use start_line/end_line to read specific sections.)",
                total_lines - MAX_READ_LINES
            ));
        }
        
        ToolResult::ok(output)
    }
}

/// Write new file
pub async fn write(args: &Value, workdir: &Path) -> ToolResult {
    let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'path' parameter");
    };
    let Some(content) = args.get("content").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'content' parameter");
    };

    let full_path = workdir.join(path);

    // Capture old content for diff preview (empty if file doesn't exist)
    let old_content = std::fs::read_to_string(&full_path).unwrap_or_default();

    // Create parent directories
    if let Some(parent) = full_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return ToolResult::err(format!("Failed to create directories: {e}"));
        }
    }

    match std::fs::write(&full_path, content) {
        Ok(_) => {
            let meta = super::FileEditMeta {
                path: path.to_string(),
                old_content,
                new_content: content.to_string(),
            };
            ToolResult::ok(format!("Created {path} ({} bytes)", content.len()))
                .with_file_edit(meta)
        }
        Err(e) => ToolResult::err(format!("Failed to write: {e}")),
    }
}

/// Replace text in file.
///
/// Supports two modes:
/// 1. Exact match (old_str): finds the unique occurrence and replaces it.
///    If multiple matches, returns line numbers so the agent can add context.
/// 2. Line-range mode (start_line + end_line): replaces lines in that range
///    with new_str. Safer when old_str matching is ambiguous.
pub async fn replace(args: &Value, workdir: &Path) -> ToolResult {
    let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'path' parameter");
    };
    let Some(new_str) = args.get("new_str").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'new_str' parameter");
    };

    let full_path = workdir.join(path);

    let content = match std::fs::read_to_string(&full_path) {
        Ok(c) => c,
        Err(e) => return ToolResult::err(format!("Failed to read {path}: {e}")),
    };

    // ── Mode 2: Line-range replacement ──
    let start_line = args.get("start_line").and_then(|v| v.as_u64());
    let end_line = args.get("end_line").and_then(|v| v.as_u64());

    if let (Some(start), Some(end)) = (start_line, end_line) {
        let lines: Vec<&str> = content.lines().collect();
        let start = start as usize;
        let end = end as usize;

        if start == 0 || end == 0 {
            return ToolResult::err("start_line and end_line are 1-indexed (start from 1)");
        }
        if start > lines.len() || end > lines.len() {
            return ToolResult::err(format!(
                "Line range {start}-{end} out of bounds (file has {} lines)", lines.len()
            ));
        }
        if start > end {
            return ToolResult::err("start_line must be <= end_line");
        }

        // Build new content: lines before + new_str + lines after
        let mut new_content = String::new();
        for line in &lines[..start - 1] {
            new_content.push_str(line);
            new_content.push('\n');
        }
        new_content.push_str(new_str);
        if !new_str.ends_with('\n') {
            new_content.push('\n');
        }
        for line in &lines[end..] {
            new_content.push_str(line);
            new_content.push('\n');
        }

        return match std::fs::write(&full_path, &new_content) {
            Ok(_) => {
                let meta = super::FileEditMeta {
                    path: path.to_string(),
                    old_content: content.clone(),
                    new_content: new_content.clone(),
                };
                ToolResult::ok(format!("Updated {path} (replaced lines {start}-{end})"))
                    .with_file_edit(meta)
            }
            Err(e) => ToolResult::err(format!("Failed to write: {e}")),
        };
    }

    // ── Mode 1: Multi-strategy old_str match (Exact -> Flexible -> Regex) ──
    let Some(old_str) = args.get("old_str").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'old_str' parameter (or use start_line+end_line for line-range replacement)");
    };

    if old_str.is_empty() {
        return ToolResult::err("old_str cannot be empty");
    }

    // Try all 3 strategies: exact, flexible (whitespace-tolerant), regex
    if let Some(result) = try_replace(&content, old_str, new_str) {
        return match std::fs::write(&full_path, &result.new_content) {
            Ok(_) => {
                let strategy_note = match result.strategy {
                    MatchStrategy::Exact => String::new(),
                    other => format!(" ({other} match)"),
                };
                let meta = super::FileEditMeta {
                    path: path.to_string(),
                    old_content: content.clone(),
                    new_content: result.new_content.clone(),
                };
                ToolResult::ok(format!("Updated {path}{strategy_note}"))
                    .with_file_edit(meta)
            }
            Err(e) => ToolResult::err(format!("Failed to write: {e}")),
        };
    }

    // All strategies failed -- build a helpful error message
    // Check if exact match found multiple occurrences
    let exact_count = content.matches(old_str).count();
    if exact_count > 1 {
        let old_first_line = old_str.lines().next().unwrap_or("");
        let mut locations = Vec::new();
        for (i, line) in content.lines().enumerate() {
            if line.contains(old_first_line) {
                locations.push(format!("  line {}: {}", i + 1, line.trim()));
            }
        }
        return ToolResult::err(format!(
            "old_str matched {exact_count} times in {path}. Include more surrounding lines to make it unique, \
or use start_line+end_line to target a specific range.\n\
Matches found at:\n{}", locations.join("\n")
        ));
    }

    // No match at all -- provide fuzzy hints
    let first_line = old_str.lines().next().unwrap_or("").trim();
    let mut hints = Vec::new();
    if !first_line.is_empty() {
        for (i, line) in content.lines().enumerate() {
            if line.contains(first_line) {
                hints.push(format!("  line {}: {}", i + 1, line.trim()));
                if hints.len() >= 3 { break; }
            }
        }
    }
    let hint_msg = if hints.is_empty() {
        String::new()
    } else {
        format!("\nDid you mean one of these lines?\n{}", hints.join("\n"))
    };
    ToolResult::err(format!(
        "old_str not found in {path} (tried exact, flexible, and regex matching). \
Make sure the content is correct.{hint_msg}"
    ))
}

/// Apply unified diff patch
pub async fn apply_patch(args: &Value, workdir: &Path) -> ToolResult {
    // Check if this is V4A format (has "*** Begin Patch" or "*** Update File:")
    if let Some(input) = args.get("input").and_then(|v| v.as_str()) {
        if input.contains("*** Begin Patch") || input.contains("*** Update File:") 
            || input.contains("*** Add File:") || input.contains("*** Delete File:") {
            return apply_v4a_patch(args, workdir).await;
        }
    }
    
    // Traditional unified diff format
    let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'path' parameter");
    };
    let Some(patch) = args.get("patch").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'patch' parameter");
    };

    let full_path = workdir.join(path);

    let content = match std::fs::read_to_string(&full_path) {
        Ok(c) => c,
        Err(_) => String::new(), // New file
    };

    // Parse and apply unified diff
    match apply_unified_diff(&content, patch) {
        Ok(new_content) => {
            if let Err(e) = std::fs::write(&full_path, &new_content) {
                return ToolResult::err(format!("Failed to write: {e}"));
            }
            ToolResult::ok(format!("Patched {path}"))
        }
        Err(e) => ToolResult::err(format!("Failed to apply patch: {e}")),
    }
}

/// Delete a file or directory
pub async fn delete(args: &Value, workdir: &Path) -> ToolResult {
    let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'path' parameter");
    };
    
    let full_path = workdir.join(path);
    
    // Security: prevent deletion outside workdir
    if !full_path.starts_with(workdir) {
        return ToolResult::err("Cannot delete files outside workspace");
    }
    
    // Prevent deleting critical files
    let dangerous_paths = [".git", "node_modules", "target", ".env", "Cargo.toml", "package.json"];
    for dangerous in dangerous_paths {
        if path == dangerous || path.ends_with(&format!("/{}", dangerous)) {
            return ToolResult::err(format!("Refusing to delete protected path: {}", dangerous));
        }
    }
    
    if !full_path.exists() {
        return ToolResult::err(format!("Path does not exist: {}", path));
    }
    
    if full_path.is_dir() {
        // Check if directory is empty or small
        let entry_count: usize = std::fs::read_dir(&full_path)
            .map(|entries| entries.count())
            .unwrap_or(0);
        
        if entry_count > 10 {
            return ToolResult::err(format!(
                "Directory has {} entries. Use execute_command with 'rm -rf' for large deletions.",
                entry_count
            ));
        }
        
        match std::fs::remove_dir_all(&full_path) {
            Ok(_) => ToolResult::ok(format!("Deleted directory: {}", path)),
            Err(e) => ToolResult::err(format!("Failed to delete directory: {}", e)),
        }
    } else {
        match std::fs::remove_file(&full_path) {
            Ok(_) => ToolResult::ok(format!("Deleted file: {}", path)),
            Err(e) => ToolResult::err(format!("Failed to delete file: {}", e)),
        }
    }
}

/// List files in directory.
/// Filters out dot-files, dot-directories, and common non-project directories
/// (node_modules, target, .git, etc.) to match Cursor's list_dir behavior.
pub async fn list(args: &Value, workdir: &Path) -> ToolResult {
    use super::should_skip_dir;

    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let recursive = args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);

    let full_path = workdir.join(path);

    if !full_path.exists() {
        return ToolResult::err(format!("Path does not exist: {path}"));
    }

    let mut entries = Vec::new();

    if recursive {
        for entry in WalkDir::new(&full_path)
            .max_depth(10)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                if e.file_type().is_dir() {
                    return !should_skip_dir(&name);
                }
                // Skip hidden files
                !name.starts_with('.')
            })
            .filter_map(|e| e.ok())
        {
            if let Ok(rel) = entry.path().strip_prefix(&full_path) {
                let rel_str = rel.to_string_lossy();
                if !rel_str.is_empty() {
                    entries.push(rel_str.to_string());
                }
            }
        }
    } else {
        if let Ok(dir) = std::fs::read_dir(&full_path) {
            for entry in dir.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip dot-files/dirs and ignored directories
                if name.starts_with('.') {
                    continue;
                }
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir && should_skip_dir(&name) {
                    continue;
                }
                let suffix = if is_dir { "/" } else { "" };
                entries.push(format!("{name}{suffix}"));
            }
        }
    }

    entries.sort();
    ToolResult::ok(entries.join("\n"))
}

// ══════════════════════════════════════════════════════════════════
//  READ MANY FILES (ported from gemini-cli's read-many-files.ts)
// ══════════════════════════════════════════════════════════════════

/// Maximum total output size for batch file reading (in chars).
const READ_MANY_MAX_CHARS: usize = 50_000;
/// Maximum number of files to read in one batch.
const READ_MANY_MAX_FILES: usize = 30;

/// Read multiple files matching glob patterns, concatenating with separators.
///
/// Respects .gitignore and common ignore patterns. Returns file contents
/// separated by `--- {filepath} ---` headers.
pub async fn read_many(args: &Value, workdir: &Path) -> ToolResult {
    use super::should_skip_dir;

    let include_patterns: Vec<String> = match args.get("include") {
        Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => return ToolResult::err("Missing 'include' parameter (glob pattern or array of patterns)"),
    };

    let exclude_patterns: Vec<String> = match args.get("exclude") {
        Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    };

    // Collect matching files using WalkDir + glob pattern matching
    // (consistent with our glob_search tool implementation)
    let mut matched_files: Vec<std::path::PathBuf> = Vec::new();

    for pattern in &include_patterns {
        // Determine search root and file pattern from the glob pattern
        // e.g., "packages/core/src/tools/**/*.ts" -> root="packages/core/src/tools", file_pat="*.ts"
        let is_recursive = pattern.contains("**");

        // Extract the directory prefix (everything before the first glob char)
        let search_dir = pattern
            .find(|c: char| c == '*' || c == '?' || c == '[')
            .map(|pos| {
                let prefix = &pattern[..pos];
                prefix.rfind('/').map(|slash| &prefix[..slash]).unwrap_or(".")
            })
            .unwrap_or(pattern.as_str());

        // Extract the file extension/name pattern after the last /
        let file_filter = pattern.rsplit('/').next().unwrap_or(pattern);

        let search_path = workdir.join(search_dir);
        if !search_path.exists() {
            continue; // Skip non-existent directories
        }

        // Check if pattern is a specific file path (no glob chars)
        if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
            let file_path = workdir.join(pattern);
            if file_path.is_file() && !matched_files.contains(&file_path) {
                matched_files.push(file_path);
            }
            continue;
        }

        let walker = if is_recursive {
            WalkDir::new(&search_path).max_depth(10)
        } else {
            WalkDir::new(&search_path).max_depth(1)
        };

        for entry in walker
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                if e.file_type().is_dir() {
                    return !should_skip_dir(&name);
                }
                true
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let file_name = entry.file_name().to_string_lossy();

            // Match file name against the pattern's file filter
            let matches = if file_filter.starts_with("*.") {
                let ext = file_filter.trim_start_matches("*.");
                file_name.ends_with(&format!(".{}", ext))
            } else if file_filter == "*" {
                true
            } else {
                file_name.contains(file_filter)
            };

            if !matches {
                continue;
            }

            let abs_path = entry.into_path();

            // Check exclude patterns
            let rel_path = abs_path.strip_prefix(workdir).unwrap_or(&abs_path);
            let rel_str = rel_path.to_string_lossy();
            let excluded = exclude_patterns.iter().any(|exc| {
                let exc_file = exc.rsplit('/').next().unwrap_or(exc);
                if exc_file.starts_with("*.") {
                    let ext = exc_file.trim_start_matches("*.");
                    rel_str.ends_with(&format!(".{}", ext))
                } else if exc.contains("**") {
                    // Simple directory-based exclusion
                    let exc_dir = exc.split("**").next().unwrap_or("");
                    rel_str.starts_with(exc_dir.trim_end_matches('/'))
                } else {
                    rel_str.contains(exc)
                }
            });

            if excluded {
                continue;
            }

            if !matched_files.contains(&abs_path) {
                matched_files.push(abs_path);
            }
        }
    }

    if matched_files.is_empty() {
        return ToolResult::err(format!(
            "No files matched patterns: {:?}. Check that the patterns are correct and files exist.",
            include_patterns
        ));
    }

    // Sort by modification time (most recently modified first, like gemini-cli)
    matched_files.sort_by(|a, b| {
        let time_a = a.metadata().and_then(|m| m.modified()).ok();
        let time_b = b.metadata().and_then(|m| m.modified()).ok();
        time_b.cmp(&time_a) // Reverse order (newest first)
    });

    // Cap number of files
    let total_matched = matched_files.len();
    if matched_files.len() > READ_MANY_MAX_FILES {
        matched_files.truncate(READ_MANY_MAX_FILES);
    }

    // Read files and concatenate
    let mut output = String::new();
    let mut files_read = 0;
    let mut files_truncated = 0;

    for file_path in &matched_files {
        let rel_path = file_path
            .strip_prefix(workdir)
            .unwrap_or(file_path)
            .to_string_lossy();

        // Check if we're approaching the output limit
        if output.len() >= READ_MANY_MAX_CHARS {
            files_truncated = matched_files.len() - files_read;
            break;
        }

        // Try to read the file
        match std::fs::read_to_string(file_path) {
            Ok(content) => {
                output.push_str(&format!("--- {} ---\n", rel_path));

                let remaining = READ_MANY_MAX_CHARS.saturating_sub(output.len());
                if content.len() > remaining {
                    // Truncate this file's content
                    output.push_str(&content[..remaining.min(content.len())]);
                    output.push_str(&format!(
                        "\n[... file truncated ({} total chars) ...]\n\n",
                        content.len()
                    ));
                    files_truncated = matched_files.len() - files_read - 1;
                    files_read += 1;
                    break;
                } else {
                    output.push_str(&content);
                    if !content.ends_with('\n') {
                        output.push('\n');
                    }
                    output.push('\n');
                }
                files_read += 1;
            }
            Err(_) => {
                // Skip binary or unreadable files silently
                output.push_str(&format!("--- {} ---\n[binary or unreadable file]\n\n", rel_path));
                files_read += 1;
            }
        }
    }

    // Summary
    let mut summary = format!("Read {} files", files_read);
    if total_matched > READ_MANY_MAX_FILES {
        summary.push_str(&format!(
            " (showing {} of {} matches, capped at {})",
            READ_MANY_MAX_FILES, total_matched, READ_MANY_MAX_FILES
        ));
    }
    if files_truncated > 0 {
        summary.push_str(&format!(
            " ({} files omitted due to output size limit)",
            files_truncated
        ));
    }
    summary.push('\n');

    ToolResult::ok(format!("{}{}", summary, output))
}

/// Simple unified diff parser
fn apply_unified_diff(original: &str, patch: &str) -> Result<String, String> {
    let mut lines: Vec<String> = original.lines().map(|s| s.to_string()).collect();
    
    let patch_lines: Vec<&str> = patch.lines().collect();
    let mut i = 0;

    while i < patch_lines.len() {
        let line = patch_lines[i];
        
        // Parse hunk header: @@ -start,count +start,count @@
        if line.starts_with("@@") {
            // Extract the line numbers
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                i += 1;
                continue;
            }
            
            let old_range = parts[1].trim_start_matches('-');
            let old_start: usize = old_range
                .split(',')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            
            let mut current_line = old_start.saturating_sub(1);
            i += 1;

            // Process hunk lines
            while i < patch_lines.len() && !patch_lines[i].starts_with("@@") {
                let hunk_line = patch_lines[i];
                
                if hunk_line.starts_with('-') {
                    // Remove line
                    if current_line < lines.len() {
                        lines.remove(current_line);
                    }
                } else if hunk_line.starts_with('+') {
                    // Add line
                    let content = hunk_line.strip_prefix('+').unwrap_or("");
                    lines.insert(current_line, content.to_string());
                    current_line += 1;
                } else if hunk_line.starts_with(' ') || hunk_line.is_empty() {
                    // Context line
                    current_line += 1;
                }
                
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    Ok(lines.join("\n"))
}

/// Apply a V4A format patch (used by apply_patch tool)
/// Format:
/// *** Begin Patch
/// *** Update File: path/to/file
/// @@ class ClassName (optional context)
/// context line
/// - removed line
/// + added line
/// context line
/// *** End Patch
pub async fn apply_v4a_patch(args: &Value, workdir: &Path) -> ToolResult {
    let Some(input) = args.get("input").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'input' parameter");
    };
    
    // Extract patch content between *** Begin Patch and *** End Patch
    let patch_start = input.find("*** Begin Patch");
    let patch_end = input.find("*** End Patch");
    
    let patch_content = match (patch_start, patch_end) {
        (Some(start), Some(end)) => &input[start..end + "*** End Patch".len()],
        _ => input, // Try to parse the whole input
    };
    
    let mut results = Vec::new();
    let mut current_file: Option<String> = None;
    let mut current_action: Option<&str> = None;
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current_hunk = Hunk::default();
    
    for line in patch_content.lines() {
        let line = line.trim_end();
        
        // File headers
        if line.starts_with("*** Add File:") {
            // Process previous file if any
            if let Some(ref file) = current_file {
                let result = apply_hunks_to_file(file, &hunks, current_action, workdir);
                results.push(result);
            }
            current_file = Some(line.trim_start_matches("*** Add File:").trim().to_string());
            current_action = Some("add");
            hunks.clear();
            current_hunk = Hunk::default();
        } else if line.starts_with("*** Update File:") {
            if let Some(ref file) = current_file {
                let result = apply_hunks_to_file(file, &hunks, current_action, workdir);
                results.push(result);
            }
            current_file = Some(line.trim_start_matches("*** Update File:").trim().to_string());
            current_action = Some("update");
            hunks.clear();
            current_hunk = Hunk::default();
        } else if line.starts_with("*** Delete File:") {
            if let Some(ref file) = current_file {
                let result = apply_hunks_to_file(file, &hunks, current_action, workdir);
                results.push(result);
            }
            current_file = Some(line.trim_start_matches("*** Delete File:").trim().to_string());
            current_action = Some("delete");
            hunks.clear();
            current_hunk = Hunk::default();
        } else if line.starts_with("@@") {
            // Context marker - save current hunk if it has content
            if !current_hunk.is_empty() {
                hunks.push(current_hunk.clone());
                current_hunk = Hunk::default();
            }
            current_hunk.context_markers.push(line.trim_start_matches("@@").trim().to_string());
        } else if line.starts_with("*** Begin Patch") || line.starts_with("*** End Patch") {
            // Ignore markers
        } else if line.starts_with('-') && !line.starts_with("---") {
            current_hunk.removals.push(line[1..].to_string());
            current_hunk.lines.push(HunkLine::Remove(line[1..].to_string()));
        } else if line.starts_with('+') && !line.starts_with("+++") {
            current_hunk.additions.push(line[1..].to_string());
            current_hunk.lines.push(HunkLine::Add(line[1..].to_string()));
        } else if current_file.is_some() && !line.is_empty() {
            // Context line
            current_hunk.context.push(line.to_string());
            current_hunk.lines.push(HunkLine::Context(line.to_string()));
        }
    }
    
    // Process last file
    if let Some(ref file) = current_file {
        if !current_hunk.is_empty() {
            hunks.push(current_hunk);
        }
        let result = apply_hunks_to_file(file, &hunks, current_action, workdir);
        results.push(result);
    }
    
    if results.is_empty() {
        return ToolResult::err("No valid patch content found");
    }
    
    let success = results.iter().all(|r| r.0);
    let output = results.iter().map(|r| r.1.clone()).collect::<Vec<_>>().join("\n");
    
    if success {
        ToolResult::ok(output)
    } else {
        ToolResult { success: false, output, file_edit: None }
    }
}

#[derive(Default, Clone)]
struct Hunk {
    context_markers: Vec<String>,
    context: Vec<String>,
    removals: Vec<String>,
    additions: Vec<String>,
    lines: Vec<HunkLine>,
}

#[derive(Clone)]
enum HunkLine {
    Context(String),
    Remove(String),
    Add(String),
}

impl Hunk {
    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

fn apply_hunks_to_file(file_path: &str, hunks: &[Hunk], action: Option<&str>, workdir: &Path) -> (bool, String) {
    let full_path = workdir.join(file_path);
    
    match action {
        Some("delete") => {
            match std::fs::remove_file(&full_path) {
                Ok(_) => (true, format!("Deleted {}", file_path)),
                Err(e) => (false, format!("Failed to delete {}: {}", file_path, e)),
            }
        }
        Some("add") => {
            // Collect all addition lines
            let content: String = hunks.iter()
                .flat_map(|h| h.additions.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            
            // Create parent directories
            if let Some(parent) = full_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            
            match std::fs::write(&full_path, content) {
                Ok(_) => (true, format!("Created {}", file_path)),
                Err(e) => (false, format!("Failed to create {}: {}", file_path, e)),
            }
        }
        Some("update") | _ => {
            // Read existing file
            let content = match std::fs::read_to_string(&full_path) {
                Ok(c) => c,
                Err(e) => return (false, format!("Failed to read {}: {}", file_path, e)),
            };
            
            let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            
            for hunk in hunks {
                // Find the location to apply this hunk
                if let Some(pos) = find_hunk_position(&lines, hunk) {
                    // Apply the hunk
                    let mut new_lines = Vec::new();
                    let mut i = 0;
                    let mut in_hunk = false;
                    let mut hunk_line_idx = 0;
                    
                    while i < lines.len() {
                        if i == pos && !in_hunk {
                            in_hunk = true;
                        }
                        
                        if in_hunk && hunk_line_idx < hunk.lines.len() {
                            match &hunk.lines[hunk_line_idx] {
                                HunkLine::Context(_) => {
                                    new_lines.push(lines[i].clone());
                                    i += 1;
                                    hunk_line_idx += 1;
                                }
                                HunkLine::Remove(_) => {
                                    // Skip this line (remove it)
                                    i += 1;
                                    hunk_line_idx += 1;
                                }
                                HunkLine::Add(content) => {
                                    new_lines.push(content.clone());
                                    hunk_line_idx += 1;
                                    // Don't increment i - we're inserting
                                }
                            }
                        } else {
                            new_lines.push(lines[i].clone());
                            i += 1;
                            if in_hunk {
                                in_hunk = false;
                            }
                        }
                    }
                    
                    // Handle remaining additions at the end
                    while hunk_line_idx < hunk.lines.len() {
                        if let HunkLine::Add(content) = &hunk.lines[hunk_line_idx] {
                            new_lines.push(content.clone());
                        }
                        hunk_line_idx += 1;
                    }
                    
                    lines = new_lines;
                } else {
                    return (false, format!("Could not find location to apply hunk in {}", file_path));
                }
            }
            
            // Write back
            match std::fs::write(&full_path, lines.join("\n")) {
                Ok(_) => (true, format!("Updated {}", file_path)),
                Err(e) => (false, format!("Failed to write {}: {}", file_path, e)),
            }
        }
    }
}

fn find_hunk_position(lines: &[String], hunk: &Hunk) -> Option<usize> {
    // Get the context lines before the first change
    let mut context_before = Vec::new();
    for line in &hunk.lines {
        match line {
            HunkLine::Context(c) => context_before.push(c.clone()),
            HunkLine::Remove(_) | HunkLine::Add(_) => break,
        }
    }
    
    if context_before.is_empty() {
        // No context, try to match the first removal line
        if let Some(HunkLine::Remove(first_remove)) = hunk.lines.iter().find(|l| matches!(l, HunkLine::Remove(_))) {
            for (i, line) in lines.iter().enumerate() {
                if line.trim() == first_remove.trim() {
                    return Some(i);
                }
            }
        }
        return Some(0); // Default to start of file
    }
    
    // Search for matching context
    'outer: for i in 0..lines.len() {
        if i + context_before.len() > lines.len() {
            break;
        }
        
        for (j, ctx) in context_before.iter().enumerate() {
            if lines[i + j].trim() != ctx.trim() {
                continue 'outer;
            }
        }
        
        return Some(i);
    }
    
    None
}
