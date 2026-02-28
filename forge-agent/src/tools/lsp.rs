use serde_json::Value;
use std::path::Path;
use crate::tools::ToolResult;
use crate::bridge::ProxyBridge;

/// Go to definition using LSP.
pub async fn lsp_go_to_definition(
    args: &Value,
    workdir: &Path,
    bridge: &dyn ProxyBridge,
) -> ToolResult {
    let Some(path_str) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'path' parameter");
    };
    let Some(line) = args.get("line").and_then(|v| v.as_u64()) else {
        return ToolResult::err("Missing 'line' parameter");
    };
    let Some(column) = args.get("column").and_then(|v| v.as_u64()) else {
        return ToolResult::err("Missing 'column' parameter");
    };

    let path = workdir.join(path_str);
    match bridge.get_definition(&path, line as u32, column as u32).await {
        Ok(locations) => {
            if locations.is_empty() {
                ToolResult::ok("No definition found")
            } else {
                let mut output = String::from("Found definitions:\n");
                for (i, loc) in locations.iter().enumerate() {
                    if i >= 10 {
                        output.push_str(&format!("... and {} more\n", locations.len() - 10));
                        break;
                    }
                    let rel_path = loc.path.strip_prefix(workdir).unwrap_or(&loc.path);
                    output.push_str(&format!(
                        "- {}:{}:{}\n",
                        rel_path.display(),
                        loc.line,
                        loc.column
                    ));
                }
                ToolResult::ok(output)
            }
        }
        Err(e) => ToolResult::err(format!("LSP go-to-definition failed: {e}")),
    }
}

/// Find references using LSP.
pub async fn lsp_find_references(
    args: &Value,
    workdir: &Path,
    bridge: &dyn ProxyBridge,
) -> ToolResult {
    let Some(path_str) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'path' parameter");
    };
    let Some(line) = args.get("line").and_then(|v| v.as_u64()) else {
        return ToolResult::err("Missing 'line' parameter");
    };
    let Some(column) = args.get("column").and_then(|v| v.as_u64()) else {
        return ToolResult::err("Missing 'column' parameter");
    };

    let path = workdir.join(path_str);
    match bridge.get_references(&path, line as u32, column as u32).await {
        Ok(locations) => {
            if locations.is_empty() {
                ToolResult::ok("No references found")
            } else {
                let mut output = String::from("Found references:\n");
                for (i, loc) in locations.iter().enumerate() {
                    if i >= 15 {
                        output.push_str(&format!("... and {} more\n", locations.len() - 15));
                        break;
                    }
                    let rel_path = loc.path.strip_prefix(workdir).unwrap_or(&loc.path);
                    output.push_str(&format!(
                        "- {}:{}:{}\n",
                        rel_path.display(),
                        loc.line,
                        loc.column
                    ));
                }
                ToolResult::ok(output)
            }
        }
        Err(e) => ToolResult::err(format!("LSP find-references failed: {e}")),
    }
}

/// Get hover info using LSP.
pub async fn lsp_hover(
    args: &Value,
    workdir: &Path,
    bridge: &dyn ProxyBridge,
) -> ToolResult {
    let Some(path_str) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'path' parameter");
    };
    let Some(line) = args.get("line").and_then(|v| v.as_u64()) else {
        return ToolResult::err("Missing 'line' parameter");
    };
    let Some(column) = args.get("column").and_then(|v| v.as_u64()) else {
        return ToolResult::err("Missing 'column' parameter");
    };

    let path = workdir.join(path_str);
    match bridge.get_hover(&path, line as u32, column as u32).await {
        Ok(Some(hover)) => {
            let mut contents = hover.contents;
            if contents.len() > 1500 {
                contents.truncate(1500);
                contents.push_str("\n... (truncated)");
            }
            ToolResult::ok(contents)
        }
        Ok(None) => ToolResult::ok("No hover information available"),
        Err(e) => ToolResult::err(format!("LSP hover failed: {e}")),
    }
}

/// Rename symbol using LSP.
pub async fn lsp_rename(
    args: &Value,
    workdir: &Path,
    bridge: &dyn ProxyBridge,
) -> ToolResult {
    let Some(path_str) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'path' parameter");
    };
    let Some(line) = args.get("line").and_then(|v| v.as_u64()) else {
        return ToolResult::err("Missing 'line' parameter");
    };
    let Some(column) = args.get("column").and_then(|v| v.as_u64()) else {
        return ToolResult::err("Missing 'column' parameter");
    };
    let Some(new_name) = args.get("new_name").and_then(|v| v.as_str()) else {
        return ToolResult::err("Missing 'new_name' parameter");
    };

    let path = workdir.join(path_str);
    match bridge.rename_symbol(&path, line as u32, column as u32, new_name).await {
        Ok(_) => ToolResult::ok(format!("Successfully renamed symbol to '{}'", new_name)),
        Err(e) => ToolResult::err(format!("LSP rename failed: {e}")),
    }
}
