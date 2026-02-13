use super::ToolResult;
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use walkdir::WalkDir;

// ── forge-search API ─────────────────────────────────────────────

/// Trigger background indexing of the workspace via forge-search.
/// Fire-and-forget: does not block the caller.
pub fn trigger_cloud_index(workdir: &Path) {
    let workspace_id = workdir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default")
        .to_string();

    let workdir = workdir.to_path_buf();

    tokio::spawn(async move {
        let client = crate::forge_search::client();
        match client.scan_directory(&workspace_id, &workdir).await {
            Ok(result) => {
                tracing::info!(
                    "forge-search indexing complete: {} files, {} symbols",
                    result.files_indexed,
                    result.nodes_created,
                );
            }
            Err(e) => {
                tracing::warn!("forge-search indexing failed: {}", e);
            }
        }
    });
}

/// Ensure workspace is indexed, indexing if needed.
/// Returns (was_already_indexed, symbol_count).
pub async fn ensure_indexed(workdir: &Path) -> (bool, i64) {
    let workspace_id = workdir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default");

    let client = crate::forge_search::client();

    // Check if already indexed
    match client.check_index_status(workspace_id).await {
        Ok((true, count)) => {
            tracing::debug!("Workspace {} already indexed ({} symbols)", workspace_id, count);
            return (true, count);
        }
        Ok((false, _)) => {
            tracing::info!("Workspace {} not indexed, starting indexing...", workspace_id);
        }
        Err(e) => {
            tracing::warn!("Could not check index status: {}", e);
            // Continue to index anyway
        }
    }

    // Index the workspace
    match client.scan_directory(workspace_id, workdir).await {
        Ok(result) => {
            tracing::info!(
                "Indexed {} files, {} symbols for workspace {}",
                result.files_indexed,
                result.nodes_created,
                workspace_id
            );
            (false, result.nodes_created as i64)
        }
        Err(e) => {
            tracing::error!("Failed to index workspace: {}", e);
            (false, 0)
        }
    }
}

// Re-export from forge_search for backwards compatibility
pub use crate::forge_search::{should_skip_dir, is_indexable_file};

// ── Semantic search (forge-search only) ──────────────────────────

/// Semantic search using forge-search backend (pgvector).
///
/// Always uses the forge-search API. Falls back to keyword search
/// only if the API is unreachable or returns no results.
pub async fn semantic(args: &Value, workdir: &Path) -> ToolResult {
    let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'query' parameter");
    };

    let client = crate::forge_search::client();

    // Derive workspace_id from the workspace path
    let workspace_id = workdir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default");

    tracing::info!("forge-search query: {}", query);

    let body: serde_json::Value = match client.search(workspace_id, query, 10).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("forge-search request failed: {}", e);
            return keyword_search(query, workdir).await;
        }
    };

    // Format results from the API response
    let results = match body.get("results").and_then(|r| r.as_array()) {
        Some(r) if !r.is_empty() => r,
        _ => return keyword_search(query, workdir).await,
    };

    let output: Vec<String> = results
        .iter()
        .filter_map(|r| {
            let file_path = r.get("file_path")?.as_str()?;
            let name = r.get("name")?.as_str().unwrap_or("");
            let symbol_type = r.get("symbol_type")?.as_str().unwrap_or("unknown");
            let content = r.get("content")?.as_str().unwrap_or("");
            let start_line = r.get("start_line").and_then(|v| v.as_i64()).unwrap_or(0);
            let end_line = r.get("end_line").and_then(|v| v.as_i64()).unwrap_or(0);
            let score = r.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);

            // Format related symbols if present
            let related_str = r
                .get("related")
                .and_then(|rel| rel.as_array())
                .map(|rels| {
                    let items: Vec<String> = rels
                        .iter()
                        .filter_map(|rel| {
                            let rname = rel.get("name")?.as_str()?;
                            let rrel = rel.get("relationship")?.as_str()?;
                            Some(format!("  - {} ({})", rname, rrel))
                        })
                        .take(5)
                        .collect();
                    if items.is_empty() {
                        String::new()
                    } else {
                        format!("\nRelated:\n{}", items.join("\n"))
                    }
                })
                .unwrap_or_default();

            Some(format!(
                "## {}:{}-{} [{} `{}`] (relevance: {:.0}%)\n```\n{}\n```{}",
                file_path,
                start_line,
                end_line,
                symbol_type,
                name,
                score * 100.0,
                truncate_lines(content, 30),
                related_str,
            ))
        })
        .collect();

    if output.is_empty() {
        return keyword_search(query, workdir).await;
    }

    tracing::info!("forge-search returned {} results", output.len());
    ToolResult::ok(output.join("\n\n"))
}

// ── Keyword search fallback ──────────────────────────────────────

/// Fallback keyword search -- used when embeddings are unavailable
async fn keyword_search(query: &str, workdir: &Path) -> ToolResult {
    let keywords: Vec<&str> = query.split_whitespace().collect();
    let mut results = Vec::new();

    for entry in WalkDir::new(workdir)
        .max_depth(5)
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
        let file_path = entry.path();
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if file_name.starts_with('.') || is_binary_extension(file_name) {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(file_path) {
            let content_lower = content.to_lowercase();
            let match_count = keywords
                .iter()
                .filter(|kw| content_lower.contains(&kw.to_lowercase()))
                .count();

            if match_count > 0 {
                let rel_path = file_path.strip_prefix(workdir).unwrap_or(file_path);
                results.push((match_count, rel_path.display().to_string(), content.clone()));
            }
        }
    }

    results.sort_by(|a, b| b.0.cmp(&a.0));

    let output: Vec<String> = results
        .iter()
        .take(10)
        .map(|(score, path, content)| {
            let preview: String = content.lines().take(5).collect::<Vec<_>>().join("\n");
            format!("## {path} (keyword matches: {score})\n```\n{preview}\n```")
        })
        .collect();

    if output.is_empty() {
        ToolResult::ok("No relevant code found")
    } else {
        ToolResult::ok(output.join("\n\n"))
    }
}

// ── Grep (ripgrep) ───────────────────────────────────────────────

/// Regex search (fallback when ripgrep not available)
pub async fn files(args: &Value, workdir: &Path) -> ToolResult {
    let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'pattern' parameter");
    };

    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    let file_pattern = args
        .get("file_pattern")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("glob").and_then(|v| v.as_str()));

    let regex = match Regex::new(pattern) {
        Ok(r) => r,
        Err(e) => return ToolResult::err(format!("Invalid regex: {e}")),
    };

    let search_path = workdir.join(path);
    let mut results = Vec::new();
    let mut match_count = 0;
    const MAX_MATCHES: usize = 30;

    for entry in WalkDir::new(&search_path)
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
        let file_path = entry.path();

        if let Some(fp) = file_pattern {
            let file_name = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if !glob_match(fp, file_name) {
                continue;
            }
        }

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if file_name.starts_with('.') || is_binary_extension(file_name) {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(file_path) {
            let rel_path = file_path.strip_prefix(workdir).unwrap_or(file_path);

            for (line_num, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    results.push(format!(
                        "{}:{}:{}",
                        rel_path.display(),
                        line_num + 1,
                        line.trim()
                    ));
                    match_count += 1;

                    if match_count >= MAX_MATCHES {
                        results.push(format!("... (truncated at {} matches)", MAX_MATCHES));
                        return ToolResult::ok(results.join("\n"));
                    }
                }
            }
        }
    }

    if results.is_empty() {
        ToolResult::ok("No matches found")
    } else {
        ToolResult::ok(results.join("\n"))
    }
}

/// Fast grep using ripgrep binary
pub async fn grep(args: &Value, workdir: &Path) -> ToolResult {
    let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'pattern' parameter");
    };

    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let file_glob = args.get("glob").and_then(|v| v.as_str());
    let case_insensitive = args
        .get("case_insensitive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let context_lines = args
        .get("context")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    let search_path = workdir.join(path);

    // Build ripgrep command
    // NOTE: rg respects .gitignore by default when run from a git repo,
    // so we do NOT add manual --glob=! exclusions.
    let mut cmd = std::process::Command::new("rg");
    cmd.arg("--line-number")
        .arg("--no-heading")
        .arg("--color=never")
        .arg("--max-count=30")
        .arg("--max-filesize=100K");

    if case_insensitive {
        cmd.arg("-i");
    }

    if context_lines > 0 {
        cmd.arg(format!("-C{}", context_lines.min(5)));
    }

    if let Some(glob) = file_glob {
        cmd.arg("-g").arg(glob);
    }

    cmd.arg(pattern).arg(&search_path).current_dir(workdir);

    match cmd.output() {
        Ok(output) => {
            if output.status.success() || output.status.code() == Some(1) {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.is_empty() {
                    ToolResult::ok("No matches found")
                } else {
                    let result = stdout
                        .lines()
                        .take(50)
                        .map(|line| {
                            if let Some(rel) =
                                line.strip_prefix(&search_path.to_string_lossy().to_string())
                            {
                                rel.trim_start_matches('/').to_string()
                            } else {
                                line.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    ToolResult::ok(result)
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                ToolResult::err(format!("ripgrep error: {}", stderr))
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                tracing::debug!("ripgrep not found, falling back to regex search");
                return files(args, workdir).await;
            }
            ToolResult::err(format!("Failed to run ripgrep: {}", e))
        }
    }
}

// ── Glob search ──────────────────────────────────────────────────

/// Find files matching a glob pattern
pub async fn glob_search(args: &Value, workdir: &Path) -> ToolResult {
    let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'pattern' parameter");
    };

    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let search_path = workdir.join(path);

    let mut results = Vec::new();
    let max_results = 100;

    let is_recursive = pattern.contains("**");
    let file_pattern = pattern.trim_start_matches("**/");

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

        if glob_match(file_pattern, &file_name) {
            let rel_path = entry
                .path()
                .strip_prefix(workdir)
                .unwrap_or(entry.path());
            results.push(rel_path.display().to_string());

            if results.len() >= max_results {
                results.push(format!("... (truncated at {} results)", max_results));
                break;
            }
        }
    }

    if results.is_empty() {
        ToolResult::ok("No files found matching pattern")
    } else {
        ToolResult::ok(format!(
            "Found {} files:\n{}",
            results.len(),
            results.join("\n")
        ))
    }
}

// ── Utilities ────────────────────────────────────────────────────

fn truncate_lines(s: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = s.lines().take(max_lines).collect();
    if s.lines().count() > max_lines {
        format!("{}\n... (truncated)", lines.join("\n"))
    } else {
        lines.join("\n")
    }
}

fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern.starts_with("*.") {
        let ext = pattern.trim_start_matches("*.");
        name.ends_with(&format!(".{ext}"))
    } else {
        name.contains(pattern)
    }
}

fn is_binary_extension(name: &str) -> bool {
    let binary_ext = [
        ".png", ".jpg", ".jpeg", ".gif", ".ico", ".webp", ".exe", ".dll", ".so", ".dylib",
        ".zip", ".tar", ".gz", ".rar", ".pdf", ".doc", ".docx", ".mp3", ".mp4", ".avi", ".mov",
        ".wasm", ".o", ".a",
    ];
    binary_ext.iter().any(|ext| name.ends_with(ext))
}

// ── Pre-search for enriched prompt ───────────────────────────────

/// Extract search keywords from a user query for pre-searching.
pub fn extract_search_keywords(query: &str) -> Vec<String> {
    let stop_words: std::collections::HashSet<&str> = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "can", "shall", "must", "need",
        "i", "me", "my", "we", "our", "you", "your", "he", "she", "it",
        "they", "them", "their", "this", "that", "these", "those",
        "in", "on", "at", "to", "for", "of", "with", "by", "from",
        "and", "or", "but", "not", "if", "then", "else", "when",
        "how", "what", "where", "why", "which", "who", "whom",
        "tell", "explain", "show", "describe", "work", "works",
        "about", "help", "please", "just", "also",
    ]
    .into_iter()
    .collect();

    query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|w| w.len() >= 3 && !stop_words.contains(&w.to_lowercase().as_str()))
        .map(|w| w.to_string())
        .collect()
}

/// Pre-search workspace for relevant code snippets based on keywords.
/// Used by build_enriched_prompt to inject relevant context.
pub fn pre_search_workspace(query: &str, workdir: &Path) -> String {
    let keywords = extract_search_keywords(query);
    if keywords.is_empty() {
        return String::new();
    }

    // Build a ripgrep-compatible pattern: keyword1|keyword2|...
    let pattern = keywords.join("|");

    let mut cmd = std::process::Command::new("rg");
    cmd.arg("--line-number")
        .arg("--no-heading")
        .arg("--color=never")
        .arg("--max-count=5")
        .arg("--max-filesize=100K")
        .arg("-i")
        .arg(&pattern)
        .arg(workdir);

    match cmd.output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = stdout.lines().take(30).collect();
            if lines.is_empty() {
                return String::new();
            }

            // Make paths relative
            let workdir_str = workdir.to_string_lossy();
            lines
                .iter()
                .map(|line| {
                    if let Some(rel) = line.strip_prefix(workdir_str.as_ref()) {
                        rel.trim_start_matches('/').to_string()
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_search_keywords() {
        let kws = extract_search_keywords("how does the natural language to query feature work");
        assert!(kws.contains(&"natural".to_string()));
        assert!(kws.contains(&"language".to_string()));
        assert!(kws.contains(&"query".to_string()));
        assert!(kws.contains(&"feature".to_string()));
        // "how", "does", "the", "to", "work" should be filtered
        assert!(!kws.contains(&"how".to_string()));
        assert!(!kws.contains(&"the".to_string()));
    }

    #[test]
    fn test_is_indexable_file() {
        assert!(is_indexable_file("main.rs"));
        assert!(is_indexable_file("forge_agent.rs"));
        assert!(!is_indexable_file("test_agent.rs"));
        assert!(!is_indexable_file("bench_agent.rs"));
        assert!(!is_indexable_file("README.md"));
        assert!(!is_indexable_file("Cargo.toml"));
    }
}
