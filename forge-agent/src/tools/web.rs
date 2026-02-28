use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use crate::tools::ToolResult;

pub async fn fetch_webpage(args: &Value) -> ToolResult {
    let url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => return ToolResult::err("Missing or invalid 'url' parameter"),
    };

    let client = match Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build() {
            Ok(c) => c,
            Err(e) => return ToolResult::err(format!("Failed to build HTTP client: {}", e)),
        };

    let response = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => return ToolResult::err(format!("Failed to fetch URL {}: {}", url, e)),
    };

    let status = response.status();
    if !status.is_success() {
        return ToolResult::err(format!("Server returned error status: {}", status));
    }

    let html = match response.text().await {
        Ok(t) => t,
        Err(e) => return ToolResult::err(format!("Failed to read response body: {}", e)),
    };

    // Convert HTML to Markdown
    let markdown = match html_to_markdown_rs::convert(&html, None) {
        Ok(md) => md,
        Err(e) => return ToolResult::err(format!("Failed to convert HTML to Markdown: {:?}", e)),
    };

    let result_json = json!({
        "markdown": markdown,
        "url": url,
    });

    ToolResult::ok(serde_json::to_string_pretty(&result_json).unwrap_or_default())
}
