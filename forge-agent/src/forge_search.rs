//! Forge Search API client.
//!
//! Replaces local embedding providers, API key management, and LLM configuration.
//! Users sign in once (GitHub/Google OAuth), and forge-search handles everything:
//!   - Code embeddings (Jina AI)
//!   - Semantic search (pgvector)
//!   - Call chain tracing (recursive CTEs)
//!   - Impact analysis (blast radius)
//!   - AI chat (Groq Kimi-K2)
//!
//! No API keys needed in the IDE. Just a JWT token from SSO.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::OnceLock;
use tokio::sync::RwLock;
use walkdir::WalkDir;

// ── Config ───────────────────────────────────────────────────────

const DEFAULT_API_URL: &str = "https://forge-search-production.up.railway.app";
const TOKEN_FILE: &str = "forge-auth.json";

/// Global forge-search client (initialized once)
static CLIENT: OnceLock<ForgeSearchClient> = OnceLock::new();

pub fn client() -> &'static ForgeSearchClient {
    CLIENT.get_or_init(ForgeSearchClient::new)
}

/// Check if user has a forge-search auth token (sync, no network).
pub fn is_authenticated() -> bool {
    AuthToken::exists()
}

// ── Auth token persistence ───────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct AuthToken {
    token: String,
    email: String,
    name: String,
}

impl AuthToken {
    fn config_dir() -> Option<std::path::PathBuf> {
        // Use platform-specific config directory
        dirs::config_dir().map(|d| d.join("forge-ide"))
    }

    fn load() -> Self {
        if let Some(dir) = Self::config_dir() {
            let path: std::path::PathBuf = dir.join(TOKEN_FILE);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(auth) = serde_json::from_str(&content) {
                    return auth;
                }
            }
        }
        Self::default()
    }

    fn save(&self) {
        if let Some(dir) = Self::config_dir() {
            let path: std::path::PathBuf = dir.join(TOKEN_FILE);
            if let Ok(content) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, content);
            }
        }
    }

    fn clear() {
        if let Some(dir) = Self::config_dir() {
            let path: std::path::PathBuf = dir.join(TOKEN_FILE);
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn exists() -> bool {
        if let Some(dir) = Self::config_dir() {
            let path: std::path::PathBuf = dir.join(TOKEN_FILE);
            path.exists()
        } else {
            false
        }
    }
}

// ── Chat Response Types ──────────────────────────────────────────

/// Full chat response from the server
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub answer: Option<String>,
    pub tool_calls: Option<Vec<ToolCallInfo>>,
    pub history: Option<serde_json::Value>,
    pub status: String, // "done", "requires_action", "error"
}

/// Tool call info from the server
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
}

/// Streaming chat chunk types (for legacy streaming API)
#[derive(Debug, Clone)]
pub enum ChatChunk {
    Text(String),
    ToolCall(ToolCallChunk),
    Done,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct ToolCallChunk {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

// ── Client ───────────────────────────────────────────────────────

pub struct ForgeSearchClient {
    http: Client,
    base_url: String,
    auth: RwLock<AuthToken>,
}

impl ForgeSearchClient {
    pub fn new() -> Self {
        let base_url = std::env::var("FORGE_SEARCH_URL")
            .unwrap_or_else(|_| DEFAULT_API_URL.to_string());

        let auth = AuthToken::load();
        tracing::info!(
            "ForgeSearch client: {} (auth: {})",
            base_url,
            if auth.token.is_empty() { "not signed in" } else { &auth.email }
        );

        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base_url,
            auth: RwLock::new(auth),
        }
    }

    // ── Auth ─────────────────────────────────────────────────────

    /// URL to open in browser for sign-in
    pub fn login_url(&self) -> String {
        format!("{}/auth/github?state=forge-ide", self.base_url)
    }

    /// URL for Google sign-in
    pub fn google_login_url(&self) -> String {
        format!("{}/auth/google?state=forge-ide", self.base_url)
    }

    /// Store the JWT token received from OAuth callback
    pub async fn set_token(&self, token: String) {
        // Decode user info from token (without verification — server already verified)
        let email = jwt_claim(&token, "email").unwrap_or_default();
        let name = jwt_claim(&token, "name").unwrap_or_default();

        let auth = AuthToken { token, email, name };
        auth.save();
        *self.auth.write().await = auth;
    }

    /// Check if user is signed in
    pub async fn is_signed_in(&self) -> bool {
        !self.auth.read().await.token.is_empty()
    }

    /// Get current user info
    pub async fn user_info(&self) -> (String, String) {
        let auth = self.auth.read().await;
        (auth.email.clone(), auth.name.clone())
    }

    /// Sign out
    pub async fn sign_out(&self) {
        AuthToken::clear();
        *self.auth.write().await = AuthToken::default();
    }

    // ── API calls ────────────────────────────────────────────────

    async fn post(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let token = self.auth.read().await.token.clone();

        let mut req = self.http.post(&url).json(body);
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req.send().await?;

        if resp.status() == 401 {
            return Err(anyhow!("Not authenticated — please sign in"));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("API error {}: {}", status, &body[..body.len().min(200)]));
        }

        Ok(resp.json().await?)
    }

    async fn get(&self, path: &str) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let token = self.auth.read().await.token.clone();

        let mut req = self.http.get(&url);
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req.send().await?;
        Ok(resp.json().await?)
    }

    // ── Search ───────────────────────────────────────────────────

    pub async fn search(&self, workspace_id: &str, query: &str, top_k: usize) -> Result<serde_json::Value> {
        self.post("/search", &serde_json::json!({
            "workspace_id": workspace_id,
            "query": query,
            "top_k": top_k,
        })).await
    }

    // ── Index ────────────────────────────────────────────────────

    pub async fn index_files(
        &self,
        workspace_id: &str,
        files: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        self.post("/index", &serde_json::json!({
            "workspace_id": workspace_id,
            "files": files,
        })).await
    }

    /// Index a single file (called on save)
    pub async fn index_file(&self, workspace_id: &str, path: &str, content: &str) -> Result<serde_json::Value> {
        self.index_files(workspace_id, vec![serde_json::json!({
            "path": path,
            "content": content,
        })]).await
    }

    // ── Trace ────────────────────────────────────────────────────

    pub async fn trace(
        &self,
        workspace_id: &str,
        symbol_name: &str,
        direction: &str,
        max_depth: usize,
    ) -> Result<serde_json::Value> {
        self.post("/trace", &serde_json::json!({
            "workspace_id": workspace_id,
            "symbol_name": symbol_name,
            "direction": direction,
            "max_depth": max_depth,
        })).await
    }

    // ── Impact ───────────────────────────────────────────────────

    pub async fn impact(
        &self,
        workspace_id: &str,
        symbol_name: &str,
        max_depth: usize,
    ) -> Result<serde_json::Value> {
        self.post("/impact", &serde_json::json!({
            "workspace_id": workspace_id,
            "symbol_name": symbol_name,
            "max_depth": max_depth,
        })).await
    }

    // ── Chat (AI) ────────────────────────────────────────────────

    pub async fn chat(
        &self,
        workspace_id: &str,
        question: &str,
        include_trace: bool,
        include_impact: bool,
    ) -> Result<serde_json::Value> {
        self.post("/chat", &serde_json::json!({
            "workspace_id": workspace_id,
            "question": question,
            "include_trace": include_trace,
            "include_impact": include_impact,
        })).await
    }

    /// Chat with multi-turn tool support.
    /// Returns a ChatResponse with answer, tool_calls, history, and status.
    pub async fn chat_multi_turn(&self, body: &serde_json::Value) -> Result<ChatResponse> {
        let url = format!("{}/chat", self.base_url);
        let token = self.auth.read().await.token.clone();

        let mut req = self.http.post(&url).json(body);
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        
        if !status.is_success() {
            return Err(anyhow!("Chat failed ({}): {}", status, text));
        }

        // Parse the JSON response
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse chat response: {}", e))?;

        let response = ChatResponse {
            answer: json.get("answer").and_then(|v| v.as_str()).map(|s| s.to_string()),
            tool_calls: json.get("tool_calls")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|tc| {
                    Some(ToolCallInfo {
                        id: tc.get("id")?.as_str()?.to_string(),
                        name: tc.get("name")?.as_str()?.to_string(),
                        args: tc.get("args").cloned().unwrap_or(serde_json::Value::Null),
                    })
                }).collect()),
            history: json.get("history").cloned(),
            status: json.get("status").and_then(|v| v.as_str()).unwrap_or("done").to_string(),
        };

        Ok(response)
    }

    /// Complex chat with streaming and tool support (legacy - kept for compatibility)
    pub async fn chat_complex(&self, body: &serde_json::Value) -> Result<impl futures_util::Stream<Item = ChatChunk>> {
        use futures_util::StreamExt;
        
        let url = format!("{}/chat", self.base_url);
        let token = self.auth.read().await.token.clone();

        let mut req = self.http.post(&url).json(body);
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(anyhow!("Chat failed: {}", resp.status()));
        }

        let stream = resp.bytes_stream().map(|res| {
            match res {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    
                    // Try to parse as complete JSON response first
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        // Check for tool_calls
                        if let Some(tool_calls) = json.get("tool_calls").and_then(|v| v.as_array()) {
                            if let Some(tc) = tool_calls.first() {
                                return ChatChunk::ToolCall(ToolCallChunk {
                                    id: tc.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                                    name: tc.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                                    arguments: tc.get("args").cloned().unwrap_or(serde_json::Value::Null),
                                });
                            }
                        }
                        
                        // Check for status
                        let status = json.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        if status == "done" {
                            return ChatChunk::Done;
                        }
                        if status == "requires_action" {
                            // IDE needs to execute tools, but we already handled tool_calls above
                            return ChatChunk::Done;
                        }
                        
                        // Return answer text
                        if let Some(answer) = json.get("answer").and_then(|v| v.as_str()) {
                            return ChatChunk::Text(answer.to_string());
                        }
                    }
                    
                    // Fallback: raw text
                    ChatChunk::Text(text.to_string())
                }
                Err(e) => ChatChunk::Error(e.to_string()),
            }
        });

        Ok(stream)
    }

    // ── Workspace Status ─────────────────────────────────────────

    /// Check if a workspace has been indexed (has symbols).
    /// Returns (is_indexed, symbol_count).
    pub async fn check_index_status(&self, workspace_id: &str) -> Result<(bool, i64)> {
        // Do a minimal search to check if the workspace has any indexed content
        let result = self.post("/search", &serde_json::json!({
            "workspace_id": workspace_id,
            "query": "_status_check_",
            "top_k": 1,
        })).await?;

        let total_nodes = result.get("total_nodes")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        Ok((total_nodes > 0, total_nodes))
    }

    // ── Scan (index whole project) ───────────────────────────────

    /// Scan and index a workspace directory.
    /// Returns IndexResult with stats.
    pub async fn scan_directory(&self, workspace_id: &str, workdir: &Path) -> Result<IndexResult> {
        self.scan_directory_with_progress(workspace_id, workdir, |_, _| {}).await
    }

    /// Scan and index with progress callback.
    /// Callback receives (files_sent, total_files).
    pub async fn scan_directory_with_progress<F>(
        &self,
        workspace_id: &str,
        workdir: &Path,
        mut on_progress: F,
    ) -> Result<IndexResult>
    where
        F: FnMut(usize, usize),
    {
        // Collect source files
        let files = collect_source_files(workdir);

        if files.is_empty() {
            return Ok(IndexResult {
                files_indexed: 0,
                nodes_created: 0,
                relationships_created: 0,
                embeddings_generated: 0,
            });
        }

        let total = files.len();
        tracing::info!("Scanning {} files for workspace {}", total, workspace_id);

        // Send in batches of 50 for better progress feedback
        const BATCH_SIZE: usize = 50;
        let mut result = IndexResult::default();
        let mut sent = 0usize;

        for batch in files.chunks(BATCH_SIZE) {
            let batch_vec: Vec<serde_json::Value> = batch.to_vec();
            
            match self.index_files(workspace_id, batch_vec).await {
                Ok(resp) => {
                    result.files_indexed += resp.get("files_indexed")
                        .and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    result.nodes_created += resp.get("nodes_created")
                        .and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    result.relationships_created += resp.get("relationships_created")
                        .and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    result.embeddings_generated += resp.get("embeddings_generated")
                        .and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                }
                Err(e) => {
                    tracing::warn!("Batch index failed: {}", e);
                    // Continue with other batches
                }
            }

            sent += batch.len();
            on_progress(sent, total);
        }

        tracing::info!(
            "Indexing complete: {} files, {} symbols, {} edges",
            result.files_indexed, result.nodes_created, result.relationships_created
        );

        Ok(result)
    }

    /// Start file watching for incremental updates.
    /// The server will track file changes and re-index as needed.
    pub async fn start_watching(&self, workspace_id: &str, root_path: &Path) -> Result<serde_json::Value> {
        self.post("/watch", &serde_json::json!({
            "workspace_id": workspace_id,
            "root_path": root_path.display().to_string(),
        })).await
    }

    /// Stop file watching for a workspace.
    pub async fn stop_watching(&self, workspace_id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/watch/{}", self.base_url, workspace_id);
        let token = self.auth.read().await.token.clone();

        let mut req = self.http.delete(&url);
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req.send().await?;
        Ok(resp.json().await?)
    }

    // ── Health ────────────────────────────────────────────────────

    pub async fn health(&self) -> Result<serde_json::Value> {
        self.get("/health").await
    }
}

// ── Index Result ─────────────────────────────────────────────────

/// Result of indexing operation
#[derive(Debug, Clone, Default)]
pub struct IndexResult {
    pub files_indexed: usize,
    pub nodes_created: usize,
    pub relationships_created: usize,
    pub embeddings_generated: usize,
}

// ── File Collection ──────────────────────────────────────────────

/// Directories to skip when scanning for source files.
const IGNORED_DIRS: &[&str] = &[
    "node_modules", "target", "dist", "build", ".git", "__pycache__",
    "vendor", ".next", "out", "coverage", ".cache", ".turbo",
    "reference-repos", "venv", ".venv", "env", ".idea", ".vscode",
];

/// File extensions to index.
const CODE_EXTENSIONS: &[&str] = &[
    ".rs", ".py", ".js", ".ts", ".tsx", ".jsx", ".go", ".java",
    ".c", ".cpp", ".h", ".hpp", ".cs", ".rb", ".php", ".swift",
    ".kt", ".scala", ".ex", ".exs", ".erl", ".hs",
];

/// Maximum file size to index (100KB).
const MAX_FILE_SIZE: usize = 100_000;

/// Maximum number of files to index per scan.
const MAX_FILES: usize = 500;

/// Check if a directory should be skipped.
pub fn should_skip_dir(name: &str) -> bool {
    name.starts_with('.') || IGNORED_DIRS.contains(&name)
}

/// Check if a file should be indexed based on extension.
pub fn is_indexable_file(name: &str) -> bool {
    CODE_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
        && !name.starts_with("test_")
        && !name.starts_with("bench_")
        && !name.ends_with("_test.go")
        && !name.ends_with(".test.ts")
        && !name.ends_with(".test.js")
        && !name.ends_with(".spec.ts")
        && !name.ends_with(".spec.js")
}

/// Collect source files from a directory for indexing.
pub fn collect_source_files(workdir: &Path) -> Vec<serde_json::Value> {
    let mut files = Vec::new();

    for entry in WalkDir::new(workdir)
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
        .take(MAX_FILES)
    {
        let path = entry.path();
        let rel_path = path.strip_prefix(workdir).unwrap_or(path);

        // Check file size before reading
        if let Ok(metadata) = path.metadata() {
            if metadata.len() as usize > MAX_FILE_SIZE {
                continue;
            }
        }

        if let Ok(content) = std::fs::read_to_string(path) {
            files.push(serde_json::json!({
                "path": rel_path.display().to_string(),
                "content": content,
            }));
        }
    }

    files
}

// ── JWT helper (decode claim without verification) ───────────────

fn jwt_claim(token: &str, claim: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    // Decode the payload (second part)
    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let payload_bytes = engine.decode(parts[1]).ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).ok()?;
    payload.get(claim)?.as_str().map(|s| s.to_string())
}
