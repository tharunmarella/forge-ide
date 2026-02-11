use super::embeddings::{EmbeddingProvider, EmbeddingStore};
use super::ToolResult;
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use std::sync::{OnceLock, RwLock as StdRwLock};
use walkdir::WalkDir;

// ── Cloud search API (forge-search) ──────────────────────────────

/// Try forge-search cloud API first (handles auth automatically).
/// Returns None if not configured or fails (caller should fall back).
async fn try_cloud_search(query: &str, workdir: &Path) -> Option<ToolResult> {
    let client = crate::forge_search::client();

    // Derive workspace_id from the workspace path
    let workspace_id = workdir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default");

    tracing::info!("Trying forge-search for query: {}", query);

    let body: serde_json::Value = match client.search(workspace_id, query, 10).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("forge-search request failed: {}", e);
            return None;
        }
    };

    // Format results from the API response
    let results = body.get("results").and_then(|r: &serde_json::Value| r.as_array())?;
    if results.is_empty() {
        return None;
    }

    let output: Vec<String> = results
        .iter()
        .filter_map(|r: &serde_json::Value| {
            let file_path = r.get("file_path")?.as_str()?;
            let name = r.get("name")?.as_str().unwrap_or("");
            let symbol_type = r.get("symbol_type")?.as_str().unwrap_or("unknown");
            let content = r.get("content")?.as_str().unwrap_or("");
            let start_line = r.get("start_line").and_then(|v: &serde_json::Value| v.as_i64()).unwrap_or(0);
            let end_line = r.get("end_line").and_then(|v: &serde_json::Value| v.as_i64()).unwrap_or(0);
            let score = r.get("score").and_then(|v: &serde_json::Value| v.as_f64()).unwrap_or(0.0);

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
        return None;
    }

    tracing::info!("Cloud search returned {} results", output.len());
    Some(ToolResult::ok(output.join("\n\n")))
}

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

// Global embedding provider config (set once on startup)
static EMBEDDING_PROVIDER: OnceLock<StdRwLock<Option<EmbeddingProvider>>> = OnceLock::new();

fn get_provider_config() -> &'static StdRwLock<Option<EmbeddingProvider>> {
    EMBEDDING_PROVIDER.get_or_init(|| StdRwLock::new(None))
}

/// Initialize embedding provider from LLM config (call once at startup)
pub fn init_embedding_provider(provider: &str, api_key: Option<&str>, base_url: Option<&str>) {
    let embedding_provider = EmbeddingProvider::from_config(provider, api_key, base_url);
    if let Ok(mut guard) = get_provider_config().write() {
        *guard = Some(embedding_provider);
    }
}

/// Resolve the embedding provider: configured > env-based Gemini > Ollama fallback
fn resolve_provider() -> EmbeddingProvider {
    // 1. Check configured provider
    if let Some(provider) = get_provider_config()
        .read()
        .ok()
        .and_then(|g| g.clone())
    {
        // Skip Ollama default -- prefer Gemini from env if available
        if !matches!(&provider, EmbeddingProvider::Ollama { .. }) {
            return provider;
        }
    }

    // 2. Check for GEMINI_API_KEY in environment (most users have this)
    if let Ok(key) = std::env::var("GEMINI_API_KEY") {
        if !key.is_empty() {
            tracing::info!("Using Gemini text-embedding-004 for semantic search");
            return EmbeddingProvider::Gemini { api_key: key };
        }
    }

    // 3. Check for OPENAI_API_KEY
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        if !key.is_empty() {
            return EmbeddingProvider::OpenAI {
                api_key: key,
                base_url: "https://api.openai.com/v1".to_string(),
            };
        }
    }

    // 4. Fallback to Ollama
    EmbeddingProvider::Ollama {
        base_url: "http://localhost:11434".to_string(),
    }
}

// ── Semantic search (hybrid: embeddings + project tree) ──────────

/// Semantic search using Gemini embeddings with SQLite persistence.
///
/// This is the "hybrid" search:
/// 1. Embeds the query using Gemini text-embedding-004
/// 2. Searches the persistent embedding database for semantically similar chunks
/// 3. Augments results with file-path context so the model knows where things are
///
/// If the database is empty, indexes the workspace first (one-time cost ~5-10s).
/// Falls back to keyword search if embeddings are unavailable.
pub async fn semantic(args: &Value, workdir: &Path) -> ToolResult {
    let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'query' parameter");
    };

    // Try cloud search API first (if FORGE_SEARCH_URL is set)
    if let Some(result) = try_cloud_search(query, workdir).await {
        return result;
    }

    // Fall back to local embedding search
    let provider = resolve_provider();

    // Open (or create) persistent embedding database
    let db = match super::embeddings_store::EmbeddingDb::open(workdir, provider.clone()) {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!("Failed to open embedding database: {}", e);
            return keyword_search(query, workdir).await;
        }
    };

    // Index if empty or very small
    let chunk_count = db.chunk_count().await;
    if chunk_count < 10 {
        tracing::info!(
            "Embedding database has {} chunks, indexing workspace with {:?}...",
            chunk_count,
            provider_label(&provider)
        );

        if let Err(e) = index_workspace(&db, &provider, workdir).await {
            tracing::warn!("Indexing failed: {}", e);
            return keyword_search(query, workdir).await;
        }
    }

    // Embed the query
    let store = match EmbeddingStore::new(provider.clone()) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to create embedding store: {}", e);
            return keyword_search(query, workdir).await;
        }
    };

    let query_embedding = match store.embed_texts_public(&[query]).await {
        Ok(embs) if !embs.is_empty() => embs[0].clone(),
        Ok(_) => {
            tracing::warn!("Empty embedding returned for query");
            return keyword_search(query, workdir).await;
        }
        Err(e) => {
            tracing::warn!("Failed to embed query: {}", e);
            return keyword_search(query, workdir).await;
        }
    };

    // Search
    match db.search(&query_embedding, 10).await {
        Ok(results) => {
            if results.is_empty() {
                return keyword_search(query, workdir).await;
            }

            // Filter out very low relevance results (score < 0.3)
            let good_results: Vec<_> = results
                .into_iter()
                .filter(|(score, _)| *score > 0.30)
                .collect();

            if good_results.is_empty() {
                // All results below threshold -- fall back to keyword
                return keyword_search(query, workdir).await;
            }

            let output: Vec<String> = good_results
                .iter()
                .map(|(score, chunk)| {
                    let type_info = if let Some(ref name) = chunk.name {
                        format!("{} `{}`", chunk.chunk_type, name)
                    } else {
                        chunk.chunk_type.clone()
                    };
                    format!(
                        "## {}:{}-{} [{}] (relevance: {:.0}%)\n```\n{}\n```",
                        chunk.file_path,
                        chunk.start_line,
                        chunk.end_line,
                        type_info,
                        score * 100.0,
                        truncate_lines(&chunk.content, 30)
                    )
                })
                .collect();

            ToolResult::ok(output.join("\n\n"))
        }
        Err(e) => {
            tracing::warn!("Database search failed: {}", e);
            keyword_search(query, workdir).await
        }
    }
}

fn provider_label(p: &EmbeddingProvider) -> &'static str {
    match p {
        EmbeddingProvider::Gemini { .. } => "Gemini",
        EmbeddingProvider::OpenAI { .. } => "OpenAI",
        EmbeddingProvider::Ollama { .. } => "Ollama",
        EmbeddingProvider::None => "None",
    }
}

// ── Workspace indexing ───────────────────────────────────────────

/// Index workspace source files into the embedding database.
///
/// - Uses tree-sitter for smart function/struct-level chunking
/// - Prepends file path + signature to each chunk for better embedding quality
/// - Filters out test/bench files, non-project files, generated code
/// - Batches embedding API calls for efficiency
async fn index_workspace(
    db: &super::embeddings_store::EmbeddingDb,
    provider: &EmbeddingProvider,
    workdir: &Path,
) -> anyhow::Result<()> {
    let store = EmbeddingStore::new(provider.clone())?;

    // Collect source files -- be selective
    let source_files: Vec<_> = WalkDir::new(workdir)
        .max_depth(8)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                let name = e.file_name().to_string_lossy();
                return !should_skip_dir(&name);
            }
            true
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let name = e.file_name().to_string_lossy();
            is_indexable_file(&name)
        })
        .take(300) // Safety cap
        .collect();

    tracing::info!("Indexing {} source files...", source_files.len());

    let mut total_chunks = 0;
    let mut total_files = 0;

    for entry in &source_files {
        let path = entry.path();

        match db.index_file(path, &store).await {
            Ok(n) => {
                total_chunks += n;
                if n > 0 {
                    total_files += 1;
                }
            }
            Err(e) => {
                tracing::debug!("Skip {}: {}", path.display(), e);
            }
        }
    }

    tracing::info!(
        "Indexed {} chunks from {} files (total files scanned: {})",
        total_chunks,
        total_files,
        source_files.len()
    );

    Ok(())
}

// is_indexable_file is now in forge_search.rs and re-exported above

fn is_code_file(name: &str) -> bool {
    let code_ext = [
        ".rs", ".py", ".js", ".ts", ".tsx", ".jsx", ".go", ".java",
        ".c", ".cpp", ".h", ".hpp", ".cs", ".rb", ".php", ".swift",
    ];
    code_ext.iter().any(|ext| name.ends_with(ext))
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
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_init_embedding_provider() {
        init_embedding_provider("anthropic", Some("test-key"), None);
        let provider = get_provider_config().read().unwrap();
        assert!(matches!(
            provider.as_ref(),
            Some(EmbeddingProvider::Ollama { .. })
        ));
    }

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
