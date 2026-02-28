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
    let _code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return ToolResult::err("Missing required parameter 'code'"),
    };
    
    ToolResult::ok("Code successfully displayed to the user in the IDE.".to_string())
}

/// Display a Mermaid diagram in the chat.
/// 
/// The agent can use this to:
/// - Show flowcharts, sequence diagrams, class diagrams
/// - Visualize system architecture
/// - Illustrate workflows and processes
/// - Display entity relationships
pub async fn show_diagram(args: &Value, _workdir: &std::path::Path) -> ToolResult {
    let _diagram_code = match args.get("diagram_code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return ToolResult::err("Missing required parameter 'diagram_code'"),
    };
    
    ToolResult::ok("Diagram successfully displayed to the user in the IDE.".to_string())
}
