//! Agent configuration setup utilities.
//!
//! In the IDE context, setup is done through the settings UI, not a TUI wizard.
//! This module provides provider/model data and validation helpers.

use crate::config::Config;

/// Provider info for setup UI
#[derive(Clone, Debug)]
pub struct Provider {
    pub name: &'static str,
    pub id: &'static str,
    pub models: &'static [&'static str],
    pub env_var: &'static str,
    pub base_url: Option<&'static str>,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        name: "Anthropic Claude",
        id: "anthropic",
        models: &[
            "claude-sonnet-4-20250514",
            "claude-opus-4-20250514",
            "claude-haiku-4-20250514",
            "claude-sonnet-4.5-20251101",
            "claude-opus-4.5-20251115",
            "claude-haiku-4.5-20251022",
        ],
        env_var: "ANTHROPIC_API_KEY",
        base_url: None,
    },
    Provider {
        name: "Google Gemini",
        id: "gemini",
        models: &[
            "gemini-2.5-flash",
            "gemini-2.5-pro",
            "gemini-3-flash",
            "gemini-3-pro",
            "gemini-2.0-flash",
            "gemini-2.0-flash-thinking-exp",
        ],
        env_var: "GEMINI_API_KEY",
        base_url: None,
    },
    Provider {
        name: "OpenAI",
        id: "openai",
        models: &[
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-5",
            "gpt-5.1-codex",
            "gpt-5.2",
            "o3-mini",
        ],
        env_var: "OPENAI_API_KEY",
        base_url: None,
    },
    Provider {
        name: "Groq (Fastest)",
        id: "groq",
        models: &[
            "llama-3.3-70b-versatile",
            "llama-3-groq-70b-tool-use",
            "llama-3-groq-8b-tool-use",
            "llama-3.1-70b-instant",
            "llama-3.1-8b-instant",
            "compound",
            "compound-mini",
        ],
        env_var: "GROQ_API_KEY",
        base_url: Some("https://api.groq.com/openai/v1"),
    },
    Provider {
        name: "Together AI",
        id: "together",
        models: &[
            "Qwen/Qwen3-Coder-480B-A35B",
            "Qwen/Qwen2.5-Coder-32B-Instruct",
            "deepseek-ai/DeepSeek-R1",
            "deepseek-ai/DeepSeek-V3",
            "deepseek-ai/DeepCoder-14B",
            "meta-llama/Llama-4-Scout",
            "meta-llama/Llama-3.3-70B-Instruct-Turbo",
            "Qwen/QwQ-32B",
        ],
        env_var: "TOGETHER_API_KEY",
        base_url: Some("https://api.together.xyz/v1"),
    },
    Provider {
        name: "OpenRouter",
        id: "openrouter",
        models: &[
            "anthropic/claude-sonnet-4.5",
            "anthropic/claude-opus-4.5",
            "openai/gpt-5.2",
            "google/gemini-3-pro",
            "deepseek/deepseek-r1",
            "qwen/qwen3-coder-480b",
            "meta-llama/llama-4",
        ],
        env_var: "OPENROUTER_API_KEY",
        base_url: Some("https://openrouter.ai/api/v1"),
    },
];

/// Check if setup is needed.
/// Returns false if signed into forge-search (no API key needed).
pub fn needs_setup(config: &Config) -> bool {
    // If forge-search auth token exists, setup is not needed
    if crate::forge_search::is_authenticated() {
        return false;
    }
    // Fallback: check for direct API key
    config.api_key().is_none()
}

/// Get provider info by id
pub fn get_provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}
