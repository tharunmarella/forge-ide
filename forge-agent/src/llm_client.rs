//! Direct LLM client with native function calling.
//!
//! Calls Gemini / OpenAI / Anthropic APIs directly from the IDE,
//! using native tool/function calling — no XML parsing needed.
//! The model returns structured JSON tool calls as part of the API response.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

use crate::tools;

// ── Provider Endpoints ───────────────────────────────────────────

fn endpoint_for(provider: &str) -> &'static str {
    match provider {
        "gemini" => "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
        "openai" => "https://api.openai.com/v1/chat/completions",
        "anthropic" => "https://api.anthropic.com/v1/messages",
        _ => "https://api.openai.com/v1/chat/completions",
    }
}

// ── Types ────────────────────────────────────────────────────────

/// A message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<NativeToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A native tool call returned by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String, // JSON string
}

/// Parsed tool call ready for execution.
#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// Result of an LLM API call.
#[derive(Debug)]
pub enum LlmResponse {
    /// Model returned text content (may also have tool calls).
    Message {
        text: Option<String>,
        tool_calls: Vec<ParsedToolCall>,
    },
    /// Model signaled it's done (stop reason).
    Done {
        text: String,
    },
    /// Error from the API.
    Error {
        error: String,
    },
}

// ── Tool Definition Conversion ───────────────────────────────────

/// Convert our internal tool definitions to OpenAI function calling format.
/// Input:  `{"name": "read_file", "description": "...", "parameters": {...}}`
/// Output: `{"type": "function", "function": {"name": "...", "description": "...", "parameters": {...}}}`
fn to_openai_tools(plan_mode: bool) -> Vec<Value> {
    tools::definitions(plan_mode)
        .into_iter()
        .map(|def| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": def["name"],
                    "description": def["description"],
                    "parameters": def["parameters"],
                }
            })
        })
        .collect()
}

/// Convert our tool definitions to Anthropic format.
/// Input:  `{"name": "read_file", "description": "...", "parameters": {...}}`
/// Output: `{"name": "...", "description": "...", "input_schema": {...}}`
fn to_anthropic_tools(plan_mode: bool) -> Vec<Value> {
    tools::definitions(plan_mode)
        .into_iter()
        .map(|def| {
            serde_json::json!({
                "name": def["name"],
                "description": def["description"],
                "input_schema": def["parameters"],
            })
        })
        .collect()
}

// ── System Prompt ────────────────────────────────────────────────

fn system_prompt(workdir: &Path) -> String {
    let dir_name = workdir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    format!(
        r#"You are an expert AI coding assistant working in the project "{dir_name}".
You have access to tools for reading, writing, searching, and executing commands in the workspace.

RULES:
- Always read a file before editing it.
- Use codebase_search or grep to find relevant code before making changes.
- Use write_to_file to create new files, replace_in_file to edit existing files.
- After making changes, use diagnostics to check for errors.
- When done, use attempt_completion to summarize what you did.
- Be concise. Execute tools, don't just talk about what you'd do.
- The workspace root is: {workdir}

IMPORTANT: Use the provided tools directly. Do NOT output XML tool tags in your text."#,
        workdir = workdir.display()
    )
}

// ── LLM Client ───────────────────────────────────────────────────

pub struct LlmClient {
    http: Client,
    provider: String,
    model: String,
    api_key: String,
}

impl LlmClient {
    pub fn new(provider: &str, model: &str, api_key: &str) -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            provider: provider.to_string(),
            model: model.to_string(),
            api_key: api_key.to_string(),
        }
    }

    /// Send a conversation to the LLM with native function calling.
    /// Returns the model's response (text and/or tool calls).
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        workdir: &Path,
        plan_mode: bool,
    ) -> Result<LlmResponse> {
        match self.provider.as_str() {
            "anthropic" => self.chat_anthropic(messages, workdir, plan_mode).await,
            _ => self.chat_openai_compat(messages, workdir, plan_mode).await,
        }
    }

    /// OpenAI-compatible API (works for OpenAI, Gemini, and other compatible providers).
    async fn chat_openai_compat(
        &self,
        messages: &[ChatMessage],
        workdir: &Path,
        plan_mode: bool,
    ) -> Result<LlmResponse> {
        let endpoint = endpoint_for(&self.provider);
        let tools = to_openai_tools(plan_mode);

        // Build messages array with system prompt
        let mut api_messages = vec![serde_json::json!({
            "role": "system",
            "content": system_prompt(workdir),
        })];

        for msg in messages {
            api_messages.push(serde_json::to_value(msg)?);
        }

        let body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
            "tools": tools,
            "max_tokens": 16384,
        });

        let resp = self
            .http
            .post(endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Ok(LlmResponse::Error {
                error: format!("API error {}: {}", status, &text[..text.len().min(500)]),
            });
        }

        let json: Value = serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse response: {}", e))?;

        // Parse OpenAI response format
        let choice = json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| anyhow!("No choices in response"))?;

        let message = choice
            .get("message")
            .ok_or_else(|| anyhow!("No message in choice"))?;

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .unwrap_or("stop");

        // Extract text content
        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        // Extract tool calls
        let tool_calls = if let Some(tcs) = message.get("tool_calls").and_then(|t| t.as_array()) {
            tcs.iter()
                .filter_map(|tc| {
                    let id = tc.get("id")?.as_str()?.to_string();
                    let func = tc.get("function")?;
                    let name = func.get("name")?.as_str()?.to_string();
                    let args_str = func.get("arguments")?.as_str().unwrap_or("{}");
                    let args: Value = serde_json::from_str(args_str).unwrap_or(Value::Object(Default::default()));
                    Some(ParsedToolCall { id, name, args })
                })
                .collect()
        } else {
            Vec::new()
        };

        if !tool_calls.is_empty() || finish_reason == "tool_calls" {
            Ok(LlmResponse::Message {
                text: content,
                tool_calls,
            })
        } else {
            Ok(LlmResponse::Done {
                text: content.unwrap_or_default(),
            })
        }
    }

    /// Anthropic Messages API (native tool calling).
    async fn chat_anthropic(
        &self,
        messages: &[ChatMessage],
        workdir: &Path,
        plan_mode: bool,
    ) -> Result<LlmResponse> {
        let tools = to_anthropic_tools(plan_mode);

        // Convert messages to Anthropic format
        let mut api_messages: Vec<Value> = Vec::new();
        for msg in messages {
            match msg.role.as_str() {
                "assistant" => {
                    let mut content_blocks: Vec<Value> = Vec::new();

                    // Add text content
                    if let Some(text) = &msg.content {
                        if !text.is_empty() {
                            content_blocks.push(serde_json::json!({"type": "text", "text": text}));
                        }
                    }

                    // Add tool use blocks
                    if let Some(tcs) = &msg.tool_calls {
                        for tc in tcs {
                            let args: Value = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(Value::Object(Default::default()));
                            content_blocks.push(serde_json::json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.function.name,
                                "input": args,
                            }));
                        }
                    }

                    if !content_blocks.is_empty() {
                        api_messages.push(serde_json::json!({
                            "role": "assistant",
                            "content": content_blocks,
                        }));
                    }
                }
                "tool" => {
                    // Tool result message
                    api_messages.push(serde_json::json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": msg.tool_call_id,
                            "content": msg.content.as_deref().unwrap_or(""),
                        }],
                    }));
                }
                role => {
                    api_messages.push(serde_json::json!({
                        "role": role,
                        "content": msg.content.as_deref().unwrap_or(""),
                    }));
                }
            }
        }

        let body = serde_json::json!({
            "model": self.model,
            "system": system_prompt(workdir),
            "messages": api_messages,
            "tools": tools,
            "max_tokens": 16384,
        });

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Ok(LlmResponse::Error {
                error: format!("Anthropic error {}: {}", status, &text[..text.len().min(500)]),
            });
        }

        let json: Value = serde_json::from_str(&text)?;

        let stop_reason = json
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .unwrap_or("end_turn");

        // Parse Anthropic content blocks
        let content_blocks = json
            .get("content")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();

        let mut response_text = String::new();
        let mut tool_calls: Vec<ParsedToolCall> = Vec::new();

        for block in &content_blocks {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                        response_text.push_str(t);
                    }
                }
                Some("tool_use") => {
                    if let (Some(id), Some(name), Some(input)) = (
                        block.get("id").and_then(|i| i.as_str()),
                        block.get("name").and_then(|n| n.as_str()),
                        block.get("input"),
                    ) {
                        tool_calls.push(ParsedToolCall {
                            id: id.to_string(),
                            name: name.to_string(),
                            args: input.clone(),
                        });
                    }
                }
                _ => {}
            }
        }

        if !tool_calls.is_empty() || stop_reason == "tool_use" {
            Ok(LlmResponse::Message {
                text: if response_text.is_empty() {
                    None
                } else {
                    Some(response_text)
                },
                tool_calls,
            })
        } else {
            Ok(LlmResponse::Done {
                text: response_text,
            })
        }
    }

    /// Build a ChatMessage for the assistant's response (including tool calls)
    /// so it can be added back to conversation history.
    pub fn assistant_message(text: Option<&str>, tool_calls: &[ParsedToolCall]) -> ChatMessage {
        let native_tcs: Vec<NativeToolCall> = tool_calls
            .iter()
            .map(|tc| NativeToolCall {
                id: tc.id.clone(),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: tc.name.clone(),
                    arguments: serde_json::to_string(&tc.args).unwrap_or_default(),
                },
            })
            .collect();

        ChatMessage {
            role: "assistant".to_string(),
            content: text.map(|t| t.to_string()),
            tool_calls: if native_tcs.is_empty() {
                None
            } else {
                Some(native_tcs)
            },
            tool_call_id: None,
        }
    }

    /// Build a tool result message to send back to the model.
    pub fn tool_result_message(tool_call_id: &str, output: &str) -> ChatMessage {
        ChatMessage {
            role: "tool".to_string(),
            content: Some(output.to_string()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
        }
    }

    /// Build a user message.
    pub fn user_message(content: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}
