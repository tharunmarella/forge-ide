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

    // ── Scan (index whole project) ───────────────────────────────

    pub async fn scan_directory(&self, workspace_id: &str, workdir: &Path) -> Result<serde_json::Value> {
        // Collect source files
        let mut files = Vec::new();
        for entry in WalkDir::new(workdir)
            .max_depth(8)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    let name = e.file_name().to_string_lossy();
                    return !super::tools::search::should_skip_dir(&name);
                }
                true
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                let name = e.file_name().to_string_lossy();
                super::tools::search::is_indexable_file(&name)
            })
            .take(500)
        {
            let path = entry.path();
            let rel_path = path.strip_prefix(workdir).unwrap_or(path);
            if let Ok(content) = std::fs::read_to_string(path) {
                files.push(serde_json::json!({
                    "path": rel_path.display().to_string(),
                    "content": content,
                }));
            }
        }

        if files.is_empty() {
            return Ok(serde_json::json!({"files_indexed": 0}));
        }

        tracing::info!("Scanning {} files for workspace {}", files.len(), workspace_id);
        self.index_files(workspace_id, files).await
    }

    // ── Health ────────────────────────────────────────────────────

    pub async fn health(&self) -> Result<serde_json::Value> {
        self.get("/health").await
    }
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
