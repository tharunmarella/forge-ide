//! Display tools - show formatted content in the chat.
//!
//! These tools allow the agent to present information in a structured, readable way
//! without modifying files or executing commands.

use serde_json::Value;
use crate::tools::ToolResult;

/// Show a code block in the chat with syntax highlighting.
/// 
/// The agent can use this to:
/// - Display code examples
/// - Show snippets for explanation
/// - Present generated code before writing it
/// - Highlight important sections of code
pub async fn show_code(args: &Value, _workdir: &std::path::Path) -> ToolResult {
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return ToolResult::err("Missing required parameter 'code'"),
    };
    
    let language = args.get("language")
        .and_then(|v| v.as_str())
        .unwrap_or("plaintext");
    
    let title = args.get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    
    // Format the response with metadata that the UI can parse
    let mut response = String::new();
    
    if let Some(t) = title {
        response.push_str(&format!("=== {} ===\n\n", t));
    }
    
    response.push_str(&format!("[CODE:{}]\n", language));
    response.push_str(code);
    response.push_str("\n[/CODE]");
    
    ToolResult::ok(response)
}

/// Display a Mermaid diagram in the chat.
/// 
/// The agent can use this to:
/// - Show flowcharts, sequence diagrams, class diagrams
/// - Visualize system architecture
/// - Illustrate workflows and processes
/// - Display entity relationships
pub async fn show_diagram(args: &Value, _workdir: &std::path::Path) -> ToolResult {
    let diagram_code = match args.get("diagram_code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return ToolResult::err("Missing required parameter 'diagram_code'"),
    };
    
    let title = args.get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    
    // Format the response with metadata that the UI can parse
    let mut response = String::new();
    
    if let Some(t) = title {
        response.push_str(&format!("=== {} ===\n\n", t));
    }
    
    response.push_str("[MERMAID]\n");
    response.push_str(diagram_code);
    response.push_str("\n[/MERMAID]");
    
    ToolResult::ok(response)
}
