//! Display tools - show formatted content in the chat.
//!
//! These tools allow the agent to present information in a structured, readable way
//! without modifying files or executing commands.

use serde_json::Value;
use crate::tools::ToolResult;

/// Show a code block in the chat with syntax highlighting.
///
/// Args:
/// - code: The code string to display
/// - language: Optional language hint (e.g. "rust", "python")
/// - title: Optional title for the block
///
/// The lapce-app UI reads `code` directly from the tool-call arguments and
/// renders it as a syntax-highlighted block.  This function validates the args
/// and returns a short confirmation; the content itself is surfaced by the UI.
pub async fn show_code(args: &Value, _workdir: &std::path::Path) -> ToolResult {
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return ToolResult::err("Missing required parameter 'code'"),
    };

    let language = args
        .get("language")
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("code");

    ToolResult::ok(format!(
        "Displayed {title} ({language}, {} chars)",
        code.len()
    ))
}

/// Display a Mermaid diagram in the chat.
///
/// Args:
/// - diagram_code: The Mermaid source string
/// - title: Optional diagram title
///
/// The lapce-app UI reads `diagram_code` from the tool-call arguments and
/// renders it via the Mermaid renderer.  This function validates the args and
/// returns a confirmation.
pub async fn show_diagram(args: &Value, _workdir: &std::path::Path) -> ToolResult {
    let diagram_code = match args.get("diagram_code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return ToolResult::err("Missing required parameter 'diagram_code'"),
    };

    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("diagram");

    ToolResult::ok(format!(
        "Displayed {title} ({} chars of Mermaid source)",
        diagram_code.len()
    ))
}
