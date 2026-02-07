//! ContextCache -- moka-backed cache for pre-fetched codebase context.
//!
//! This solves the "20-turn exploration" problem by:
//! 1. Building a RepoMap (Aider-style PageRank symbol map) once and caching it
//! 2. Pre-searching for keywords from the user's query via ripgrep
//! 3. Caching both so repeat/similar queries are instant
//!
//! The cached context is injected into the enriched prompt BEFORE the first
//! LLM turn, so the agent already knows where everything is.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

use moka::sync::Cache;

use crate::repomap::RepoMap;

/// Global context cache shared across all agent sessions.
/// Uses moka's concurrent cache with TTL-based expiration.
static CACHE: LazyLock<ContextCache> = LazyLock::new(ContextCache::new);

/// Get the global context cache.
pub fn global() -> &'static ContextCache {
    &CACHE
}

/// Cached context for workspaces. Thread-safe, lock-free reads.
pub struct ContextCache {
    /// RepoMap cache: workspace_path -> repo_map_string
    /// TTL 5 minutes -- stale after file changes, but cheap to rebuild
    repo_map: Cache<PathBuf, String>,

    /// Pre-search results: (workspace_path, sorted_keywords) -> formatted results
    /// TTL 2 minutes -- queries about the same code area hit cache
    pre_search: Cache<(PathBuf, String), String>,
}

impl ContextCache {
    fn new() -> Self {
        Self {
            repo_map: Cache::builder()
                .max_capacity(20) // up to 20 workspaces
                .time_to_live(Duration::from_secs(300)) // 5 min
                .build(),
            pre_search: Cache::builder()
                .max_capacity(100) // up to 100 query variants
                .time_to_live(Duration::from_secs(120)) // 2 min
                .build(),
        }
    }

    /// Get or build the RepoMap for a workspace.
    /// First call builds it (takes ~50-200ms), subsequent calls return cached.
    pub fn get_repo_map(&self, workspace: &Path) -> String {
        let key = workspace.to_path_buf();
        self.repo_map.get_with(key, || {
            let start = std::time::Instant::now();
            let mut builder = RepoMap::new(workspace.to_path_buf(), 8192);
            let map = builder.build_from_directory();
            tracing::info!(
                "[ContextCache] Built RepoMap in {:?} ({} chars)",
                start.elapsed(),
                map.len()
            );
            map
        })
    }

    /// Get or run pre-search for the user's query keywords.
    /// Extracts identifiers from the query, greps for them, returns formatted matches.
    pub fn get_pre_search(&self, workspace: &Path, user_query: &str) -> String {
        let keywords = extract_keywords(user_query);
        if keywords.is_empty() {
            return String::new();
        }

        // Cache key: workspace + sorted keywords
        let key_hash = keywords.join("|");
        let key = (workspace.to_path_buf(), key_hash);

        self.pre_search.get_with(key, || {
            let start = std::time::Instant::now();
            let result = pre_search_workspace(workspace, &keywords);
            tracing::info!(
                "[ContextCache] Pre-search for {:?} in {:?} ({} chars)",
                &keywords,
                start.elapsed(),
                result.len()
            );
            result
        })
    }

    /// Invalidate RepoMap for a workspace (call after file mutations).
    pub fn invalidate_repo_map(&self, workspace: &Path) {
        self.repo_map.invalidate(&workspace.to_path_buf());
    }

    /// Invalidate all pre-search results for a workspace.
    pub fn invalidate_searches(&self, workspace: &Path) {
        // Moka doesn't support prefix-based invalidation, so we invalidate all.
        // This is fine because pre-search is cheap to rebuild.
        self.pre_search.invalidate_all();
        let _ = workspace; // suppress unused warning
    }

    /// Invalidate everything for a workspace (call after significant file changes).
    pub fn invalidate_workspace(&self, workspace: &Path) {
        self.invalidate_repo_map(workspace);
        self.invalidate_searches(workspace);
    }
}

// ══════════════════════════════════════════════════════════════════
//  KEYWORD EXTRACTION
// ══════════════════════════════════════════════════════════════════

/// Stop words that are never useful as search keywords.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "can", "shall", "must", "need",
    "i", "me", "my", "we", "our", "you", "your", "he", "she", "it",
    "they", "them", "this", "that", "these", "those",
    "in", "on", "at", "to", "for", "of", "with", "by", "from", "into",
    "about", "between", "through", "after", "before", "during",
    "and", "or", "but", "not", "if", "then", "else", "when", "where",
    "how", "what", "which", "who", "whom", "why",
    "all", "each", "every", "both", "few", "more", "most", "other",
    "some", "such", "no", "nor", "only", "own", "same", "so", "than",
    "too", "very", "just", "because",
    // Task-oriented words the user might say but aren't code identifiers
    "add", "create", "make", "build", "write", "implement", "fix", "update",
    "change", "modify", "remove", "delete", "refactor", "move", "rename",
    "find", "search", "look", "show", "tell", "explain", "check",
    "file", "files", "code", "function", "method", "class", "module",
    "please", "thanks", "help", "want", "like", "using", "use",
];

/// Extract likely code identifiers from a user's natural language query.
///
/// Returns keywords sorted by specificity (most specific first), deduped.
/// Keeps at most 5 keywords to avoid flooding with grep calls.
fn extract_keywords(query: &str) -> Vec<String> {
    let stop_set: std::collections::HashSet<&str> = STOP_WORDS.iter().copied().collect();

    let mut keywords: Vec<(String, u32)> = Vec::new();

    // 1. Extract quoted strings first (highest priority)
    let quote_re = regex::Regex::new(r#"["'`]([^"'`]+)["'`]"#).unwrap();
    for cap in quote_re.captures_iter(query) {
        if let Some(m) = cap.get(1) {
            let s = m.as_str().trim().to_string();
            if s.len() >= 2 {
                keywords.push((s, 100)); // highest priority
            }
        }
    }

    // 2. Extract words that look like code identifiers
    for word in query.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        let w = word.trim();
        if w.len() < 2 || w.len() > 60 {
            continue;
        }

        let lower = w.to_lowercase();
        if stop_set.contains(lower.as_str()) {
            continue;
        }

        // Score by how "code-like" the word looks
        let mut score: u32 = 0;

        // snake_case or kebab-case
        if w.contains('_') || w.contains('-') {
            score += 50;
        }

        // CamelCase or PascalCase
        let has_upper = w.chars().any(|c| c.is_uppercase());
        let has_lower = w.chars().any(|c| c.is_lowercase());
        if has_upper && has_lower && w.len() > 2 {
            score += 40;
        }

        // Contains digits (likely a specific identifier)
        if w.chars().any(|c| c.is_ascii_digit()) {
            score += 20;
        }

        // Longer words are more specific
        if w.len() >= 6 {
            score += 15;
        }
        if w.len() >= 10 {
            score += 10;
        }

        // Known code patterns (flags, options, etc.)
        if w.starts_with("--") || w.starts_with('-') {
            score += 30;
        }

        // If it didn't score on any code-like heuristic, give a baseline
        // but only if it's not a pure common English word
        if score == 0 {
            // Skip very short non-code words
            if w.len() < 4 {
                continue;
            }
            score = 5; // low priority fallback
        }

        keywords.push((w.to_string(), score));
    }

    // Deduplicate (case-insensitive)
    let mut seen = std::collections::HashSet::new();
    keywords.retain(|(w, _)| seen.insert(w.to_lowercase()));

    // Sort by score descending
    keywords.sort_by(|a, b| b.1.cmp(&a.1));

    // Take top 5
    keywords.into_iter().take(5).map(|(w, _)| w).collect()
}

// ══════════════════════════════════════════════════════════════════
//  PRE-SEARCH
// ══════════════════════════════════════════════════════════════════

/// Run ripgrep for each keyword and collect formatted results.
fn pre_search_workspace(workspace: &Path, keywords: &[String]) -> String {
    let mut sections = Vec::new();

    for keyword in keywords {
        let matches = rg_search(workspace, keyword, 8);
        if !matches.is_empty() {
            sections.push(format!(
                "Results for \"{}\":\n{}",
                keyword,
                matches.join("\n")
            ));
        }
    }

    if sections.is_empty() {
        return String::new();
    }

    sections.join("\n\n")
}

/// Run ripgrep for a single keyword, returning up to `max_results` formatted match lines.
fn rg_search(workspace: &Path, pattern: &str, max_results: usize) -> Vec<String> {
    let output = Command::new("rg")
        .args([
            "--no-heading",
            "--line-number",
            "--max-count", "3",      // max 3 matches per file
            "--max-filesize", "100K", // skip huge files
            "--max-depth", "8",
            "-i",                     // case insensitive
            "--color", "never",
            pattern,
        ])
        .current_dir(workspace)
        .output();

    let output = match output {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() && output.stdout.is_empty() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results: Vec<String> = Vec::new();

    for line in stdout.lines() {
        if results.len() >= max_results {
            break;
        }
        // Skip lines from noise directories
        if line.contains("node_modules/")
            || line.contains("/target/")
            || line.contains("/.git/")
            || line.contains("/reference-repos/")
            || line.contains("/__pycache__/")
            || line.contains("/.venv/")
        {
            continue;
        }
        // Truncate very long match lines
        let display = if line.len() > 150 {
            format!("{}...", &line[..150])
        } else {
            line.to_string()
        };
        results.push(format!("  {}", display));
    }

    results
}

// ══════════════════════════════════════════════════════════════════
//  TESTS
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_keywords_code_identifiers() {
        let kw = extract_keywords("add a --version flag to the CLI");
        assert!(kw.contains(&"--version".to_string()) || kw.contains(&"version".to_string()));
    }

    #[test]
    fn test_extract_keywords_camel_case() {
        let kw = extract_keywords("where is AuthService defined?");
        assert!(kw.iter().any(|k| k == "AuthService"));
    }

    #[test]
    fn test_extract_keywords_snake_case() {
        let kw = extract_keywords("fix the build_project_tree function");
        assert!(kw.iter().any(|k| k == "build_project_tree"));
    }

    #[test]
    fn test_extract_keywords_stop_words_filtered() {
        let kw = extract_keywords("what is the main function");
        // "what", "is", "the" should be filtered out; "main" might remain
        assert!(!kw.iter().any(|k| k == "what" || k == "is" || k == "the"));
    }

    #[test]
    fn test_extract_keywords_quoted() {
        let kw = extract_keywords("find the error 'connection refused' in logs");
        assert!(kw.iter().any(|k| k == "connection refused"));
    }

    #[test]
    fn test_extract_keywords_max_five() {
        let kw = extract_keywords(
            "refactor AuthService UserController PaymentHandler OrderProcessor NotificationManager EventBus",
        );
        assert!(kw.len() <= 5);
    }

    #[test]
    fn test_cache_returns_same_value() {
        let cache = ContextCache::new();
        // Pre-search for a nonsense query should return empty
        let r1 = cache.get_pre_search(Path::new("/nonexistent"), "xyzzy");
        let r2 = cache.get_pre_search(Path::new("/nonexistent"), "xyzzy");
        assert_eq!(r1, r2);
    }
}
