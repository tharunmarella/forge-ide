//! LLM-based edit self-correction, ported from gemini-cli's `llm-edit-fixer.ts`.
//!
//! When all match strategies (exact, flexible, regex) fail in `replace_in_file`,
//! this module calls a fast LLM to produce a corrected `search` string.
//! Results are cached with moka to avoid redundant LLM calls.

use moka::sync::Cache;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

/// Cache for edit fixes: keyed by hash(file_content, old_str, new_str)
static FIX_CACHE: LazyLock<Cache<u64, Option<EditFixResult>>> = LazyLock::new(|| {
    Cache::builder()
        .max_capacity(100)
        .time_to_live(std::time::Duration::from_secs(300)) // 5 min TTL
        .build()
});

/// Result from the edit fixer LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditFixResult {
    /// Corrected search string that should match the file content
    pub search: String,
    /// Corrected replacement string
    pub replace: String,
    /// Brief explanation of what was corrected
    pub explanation: String,
    /// If true, the LLM determined no changes are needed
    pub no_changes_required: bool,
}

/// System prompt for the edit fixer LLM (adapted from gemini-cli).
const EDIT_FIX_SYSTEM_PROMPT: &str = r#"You are an expert code-editing assistant specializing in debugging and correcting failed search-and-replace operations.

# Primary Goal
Your task is to analyze a failed edit attempt and provide a corrected `search` string that will match the text in the file precisely. The correction should be as minimal as possible, staying very close to the original, failed `search` string. Do NOT invent a completely new edit based on the instruction; your job is to fix the provided parameters.

It is important that you do not try to figure out if the instruction is correct. DO NOT GIVE ADVICE. Your only goal here is to do your best to perform the search and replace task!

# Input
You will receive:
1. `file_content`: The full content of the target file.
2. `failed_search`: The search string that did not match any text in the file.
3. `intended_replace`: The replacement string.
4. `instruction`: (optional) A description of what the edit is trying to do.
5. `error`: The error message from the failed attempt.

# Output
Respond ONLY with a JSON object (no markdown fences):
{
  "search": "<corrected search string>",
  "replace": "<corrected replace string>",
  "explanation": "<brief explanation>",
  "no_changes_required": false
}

If the file already contains the intended change, set no_changes_required to true and use empty strings for search and replace.

# Rules
1. The corrected `search` MUST be an exact substring of `file_content`.
2. Keep corrections MINIMAL -- fix whitespace, indentation, or small text differences only.
3. Do NOT re-implement the change from scratch. Fix the failed parameters.
4. The corrected `replace` should be the same as `intended_replace` unless minor adjustments are needed to fit the context.
5. Pay close attention to indentation -- it must match the file exactly."#;

/// Attempt to fix a failed edit using an LLM call.
///
/// This function:
/// 1. Checks the cache first
/// 2. Constructs a prompt with file content + failed edit parameters
/// 3. Calls the LLM to produce a corrected search/replace pair
/// 4. Caches and returns the result
///
/// The caller should apply the corrected search/replace using exact match.
pub async fn fix_failed_edit(
    api_key: &str,
    provider: &str,
    file_content: &str,
    failed_search: &str,
    intended_replace: &str,
    instruction: Option<&str>,
    error_msg: &str,
) -> Option<EditFixResult> {
    // Compute cache key
    let cache_key = {
        let mut hasher = DefaultHasher::new();
        file_content.hash(&mut hasher);
        failed_search.hash(&mut hasher);
        intended_replace.hash(&mut hasher);
        hasher.finish()
    };

    // Check cache
    if let Some(cached) = FIX_CACHE.get(&cache_key) {
        tracing::debug!("edit_fixer: cache hit");
        return cached;
    }

    // Cap file content to avoid huge prompts (keep first 8000 + last 2000 chars)
    let capped_content = if file_content.len() > 10000 {
        format!(
            "{}\n\n[... {} chars omitted ...]\n\n{}",
            &file_content[..8000],
            file_content.len() - 10000,
            &file_content[file_content.len() - 2000..],
        )
    } else {
        file_content.to_string()
    };

    let user_prompt = format!(
        "file_content:\n```\n{}\n```\n\nfailed_search:\n```\n{}\n```\n\nintended_replace:\n```\n{}\n```\n\ninstruction: {}\n\nerror: {}",
        capped_content,
        failed_search,
        intended_replace,
        instruction.unwrap_or("(not provided)"),
        error_msg,
    );

    // Make the LLM call using the appropriate provider
    let result = call_fixer_llm(api_key, provider, &user_prompt).await;

    // Cache the result
    FIX_CACHE.insert(cache_key, result.clone());

    result
}

/// Call the fixer LLM. Uses a fast/cheap model variant.
async fn call_fixer_llm(
    api_key: &str,
    provider: &str,
    user_prompt: &str,
) -> Option<EditFixResult> {
    // Determine which fast model to use
    let model = match provider {
        "gemini" => "gemini-2.0-flash",
        "anthropic" => "claude-3-5-haiku-20241022",
        "openai" => "gpt-4o-mini",
        _ => return None,
    };

    tracing::info!("edit_fixer: calling {} / {} for self-correction", provider, model);

    // Build the request body based on provider
    let client = reqwest::Client::new();

    let response_text = match provider {
        "gemini" => {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                model, api_key
            );
            let body = serde_json::json!({
                "contents": [{
                    "parts": [{"text": user_prompt}]
                }],
                "systemInstruction": {
                    "parts": [{"text": EDIT_FIX_SYSTEM_PROMPT}]
                },
                "generationConfig": {
                    "temperature": 0.0,
                    "maxOutputTokens": 4096
                }
            });

            let resp = client.post(&url).json(&body).send().await.ok()?;
            let json: serde_json::Value = resp.json().await.ok()?;
            json["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()?
                .to_string()
        }
        "anthropic" => {
            let body = serde_json::json!({
                "model": model,
                "max_tokens": 4096,
                "temperature": 0.0,
                "system": EDIT_FIX_SYSTEM_PROMPT,
                "messages": [{"role": "user", "content": user_prompt}]
            });

            let resp = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .ok()?;
            let json: serde_json::Value = resp.json().await.ok()?;
            json["content"][0]["text"].as_str()?.to_string()
        }
        "openai" => {
            let body = serde_json::json!({
                "model": model,
                "temperature": 0.0,
                "max_tokens": 4096,
                "messages": [
                    {"role": "system", "content": EDIT_FIX_SYSTEM_PROMPT},
                    {"role": "user", "content": user_prompt}
                ]
            });

            let resp = client
                .post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", api_key))
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .ok()?;
            let json: serde_json::Value = resp.json().await.ok()?;
            json["choices"][0]["message"]["content"]
                .as_str()?
                .to_string()
        }
        _ => return None,
    };

    // Parse the response -- strip markdown fences if present
    let cleaned = response_text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    match serde_json::from_str::<EditFixResult>(cleaned) {
        Ok(result) => {
            tracing::info!("edit_fixer: got correction -- {}", result.explanation);
            Some(result)
        }
        Err(e) => {
            tracing::warn!("edit_fixer: failed to parse LLM response: {}", e);
            None
        }
    }
}
