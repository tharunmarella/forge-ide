//! AI-powered inline completion (FIM) endpoint.
//!
//! Calls a fast LLM with Fill-in-the-Middle context to generate
//! Copilot-style ghost text completions.

use std::path::Path;

use lapce_rpc::core::AiInlineCompletionItem;
use serde::{Deserialize, Serialize};

// ── Config loading ──────────────────────────────────────────────

const AI_KEYS_FILE: &str = "ai-keys.toml";

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct AiKeysConfig {
    #[serde(default)]
    keys: std::collections::HashMap<String, String>,
    #[serde(default)]
    defaults: AiDefaults,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AiDefaults {
    provider: String,
    model: String,
}

impl Default for AiDefaults {
    fn default() -> Self {
        Self {
            provider: "gemini".to_string(),
            model: "gemini-2.5-flash".to_string(),
        }
    }
}

impl AiKeysConfig {
    fn load() -> Self {
        let config_dir = lapce_core::directory::Directory::config_directory();
        let Some(path) = config_dir.map(|d| d.join(AI_KEYS_FILE)) else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }
}

// ── FIM Completion ──────────────────────────────────────────────

/// Generate inline completions using a fast AI model.
///
/// Reads the API key from `ai-keys.toml`, builds a FIM prompt,
/// calls the provider's API, and returns completion items.
pub async fn generate_completion(
    file_path: &Path,
    prefix: &str,
    suffix: &str,
) -> Vec<AiInlineCompletionItem> {
    let config = AiKeysConfig::load();
    let provider = &config.defaults.provider;
    let api_key = config
        .keys
        .get(provider)
        .cloned()
        .unwrap_or_default();

    if api_key.is_empty() {
        return vec![];
    }

    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    let lang_hint = detect_language(file_path);

    match provider.as_str() {
        "gemini" => {
            generate_gemini(&api_key, file_name, &lang_hint, prefix, suffix).await
        }
        "anthropic" => {
            generate_anthropic(&api_key, file_name, &lang_hint, prefix, suffix).await
        }
        "openai" => {
            generate_openai(&api_key, file_name, &lang_hint, prefix, suffix).await
        }
        _ => vec![],
    }
}

/// Detect programming language from file extension.
fn detect_language(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => "rust",
        "py" => "python",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "rb" => "ruby",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "cs" => "csharp",
        "php" => "php",
        "lua" => "lua",
        "zig" => "zig",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "md" => "markdown",
        "html" | "htm" => "html",
        "css" | "scss" | "sass" => "css",
        "sh" | "bash" | "zsh" => "shell",
        "sql" => "sql",
        _ => ext,
    }
    .to_string()
}

/// Build the FIM prompt for all providers.
fn build_fim_prompt(file_name: &str, lang: &str, prefix: &str, suffix: &str) -> String {
    format!(
        "You are a code completion engine. Complete the code at the cursor position marked <CURSOR>.\n\
        File: {file_name} (language: {lang})\n\
        Rules:\n\
        - Output ONLY the code to insert at <CURSOR>. No explanations, no markdown, no backticks.\n\
        - Be concise: complete the current statement or a small logical block (1-5 lines).\n\
        - Match the existing code style (indentation, naming, patterns).\n\
        - If the cursor is mid-line, complete to the end of the line or statement.\n\
        - Do NOT repeat any text from the prefix or suffix.\n\n\
        {prefix}<CURSOR>{suffix}"
    )
}

// ── Gemini ──────────────────────────────────────────────────────

async fn generate_gemini(
    api_key: &str,
    file_name: &str,
    lang: &str,
    prefix: &str,
    suffix: &str,
) -> Vec<AiInlineCompletionItem> {
    let prompt = build_fim_prompt(file_name, lang, prefix, suffix);

    // Use gemini-2.0-flash for speed
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key={api_key}"
    );

    let body = serde_json::json!({
        "contents": [{
            "parts": [{"text": prompt}]
        }],
        "generationConfig": {
            "temperature": 0.0,
            "maxOutputTokens": 256,
            "stopSequences": ["\n\n\n", "```"]
        }
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let resp = match client.post(&url).json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("Gemini FIM request failed: {e}");
            return vec![];
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            tracing::debug!("Gemini FIM response parse failed: {e}");
            return vec![];
        }
    };

    // Extract text from response
    let text = json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();

    if text.is_empty() {
        return vec![];
    }

    // Clean up: remove any leading/trailing whitespace artifacts
    let text = text.trim_end().to_string();

    vec![AiInlineCompletionItem {
        insert_text: text,
        start_offset: prefix.len(),
        end_offset: prefix.len(),
    }]
}

// ── Anthropic ───────────────────────────────────────────────────

async fn generate_anthropic(
    api_key: &str,
    file_name: &str,
    lang: &str,
    prefix: &str,
    suffix: &str,
) -> Vec<AiInlineCompletionItem> {
    let prompt = build_fim_prompt(file_name, lang, prefix, suffix);

    let body = serde_json::json!({
        "model": "claude-3-5-haiku-20241022",
        "max_tokens": 256,
        "temperature": 0.0,
        "messages": [{
            "role": "user",
            "content": prompt
        }]
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let resp = match client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("Anthropic FIM request failed: {e}");
            return vec![];
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            tracing::debug!("Anthropic FIM response parse failed: {e}");
            return vec![];
        }
    };

    let text = json["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .trim_end()
        .to_string();

    if text.is_empty() {
        return vec![];
    }

    vec![AiInlineCompletionItem {
        insert_text: text,
        start_offset: prefix.len(),
        end_offset: prefix.len(),
    }]
}

// ── OpenAI ──────────────────────────────────────────────────────

async fn generate_openai(
    api_key: &str,
    file_name: &str,
    lang: &str,
    prefix: &str,
    suffix: &str,
) -> Vec<AiInlineCompletionItem> {
    let prompt = build_fim_prompt(file_name, lang, prefix, suffix);

    let body = serde_json::json!({
        "model": "gpt-4o-mini",
        "max_tokens": 256,
        "temperature": 0.0,
        "messages": [{
            "role": "user",
            "content": prompt
        }]
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let resp = match client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("OpenAI FIM request failed: {e}");
            return vec![];
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            tracing::debug!("OpenAI FIM response parse failed: {e}");
            return vec![];
        }
    };

    let text = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim_end()
        .to_string();

    if text.is_empty() {
        return vec![];
    }

    vec![AiInlineCompletionItem {
        insert_text: text,
        start_offset: prefix.len(),
        end_offset: prefix.len(),
    }]
}
