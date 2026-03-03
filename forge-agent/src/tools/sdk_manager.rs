//! SDK Manager tool - manage development tools and runtimes via proto.
//!
//! This provides agent access to the IDE's SDK management system for:
//! - Installing missing tools (Node.js, Python, Rust, Go, etc.)
//! - Detecting project requirements automatically
//! - Managing tool versions consistently across platforms
//! - Avoiding command hallucinations for tool installation

use serde_json::Value;
use crate::tools::ToolResult;
use tokio::process::Command;
use std::path::{Path, PathBuf};

/// Resolve the proto binary path.
fn proto_bin() -> PathBuf {
    // 1. Respect explicit override
    if let Ok(p) = std::env::var("PROTO_BIN") {
        return PathBuf::from(p);
    }

    // 2. Check ~/.proto/bin/proto (default install location)
    if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
        let default = home.join(".proto").join("bin").join("proto");
        if default.exists() {
            return default;
        }
    }

    // 3. Fallback — rely on whatever PATH is available
    PathBuf::from("proto")
}

/// SDK Manager tool for development tool management.
///
/// Operations:
/// - install: Install a specific tool and version
/// - list_installed: List currently installed tools
/// - list_available: List available tools/plugins
/// - detect_project: Detect tools needed for current project
/// - uninstall: Remove a tool version
/// - versions: List available versions for a tool
///
/// This uses the proto tool manager for consistent, cross-platform installation.
pub async fn sdk_manager(args: &Value, workdir: &std::path::Path) -> ToolResult {
    let operation = args.get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    
    match operation {
        "install" => {
            let tool = match args.get("tool").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return ToolResult::err("Missing 'tool' parameter for install operation"),
            };
            let version = args.get("version").and_then(|v| v.as_str()).unwrap_or("latest");
            let pin = args.get("pin").and_then(|v| v.as_bool()).unwrap_or(true);
            
            install_tool(tool, version, pin, workdir).await
        }
        "list_installed" => {
            list_installed_tools().await
        }
        "list_available" => {
            let tool = args.get("tool").and_then(|v| v.as_str());
            list_available_tools(tool).await
        }
        "detect_project" => {
            detect_project_tools(workdir).await
        }
        "uninstall" => {
            let tool = match args.get("tool").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return ToolResult::err("Missing 'tool' parameter for uninstall operation"),
            };
            let version = match args.get("version").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => return ToolResult::err("Missing 'version' parameter for uninstall operation"),
            };
            
            uninstall_tool(tool, version).await
        }
        "versions" => {
            let tool = match args.get("tool").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return ToolResult::err("Missing 'tool' parameter for versions operation"),
            };
            get_tool_versions(tool).await
        }
        _ => ToolResult::err(&format!(
            "Unknown SDK manager operation: '{}'. Valid operations: install, list_installed, list_available, detect_project, uninstall, versions", 
            operation
        )),
    }
}

fn is_proto_tool(tool: &str) -> bool {
    matches!(tool, "node" | "npm" | "pnpm" | "yarn" | "bun" | "deno" | "python" | "poetry" | "uv" | "go" | "rust" | "ruby")
}

async fn install_tool(tool: &str, version: &str, pin: bool, workdir: &std::path::Path) -> ToolResult {
    if is_proto_tool(tool) {
        install_proto_tool(tool, version, pin, workdir).await
    } else {
        install_system_tool(tool).await
    }
}

async fn install_proto_tool(tool: &str, version: &str, pin: bool, workdir: &std::path::Path) -> ToolResult {
    let mut cmd = Command::new(proto_bin());
    cmd.arg("install");
    cmd.current_dir(workdir);
    
    // Add tool name first
    cmd.arg(tool);
    
    // Add version as separate argument if specified
    // proto install <tool> <version> [--pin]
    let version_display = if version != "latest" && !version.is_empty() {
        cmd.arg(version);
        format!(" {}", version)
    } else {
        String::new()
    };
    
    if pin {
        cmd.arg("--pin");
    }
    
    let tool_display = format!("{}{}", tool, version_display);
    
    match cmd.output().await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            
            if output.status.success() {
                ToolResult::ok(&format!("✅ Successfully installed {}\n{}", tool_display, stdout))
            } else {
                ToolResult::err(&format!("❌ Failed to install {}: {}\n{}", tool_display, stderr, stdout))
            }
        }
        Err(e) => ToolResult::err(&format!("❌ Failed to execute proto install: {}", e)),
    }
}

async fn install_system_tool(tool: &str) -> ToolResult {
    let os = std::env::consts::OS;
    
    let (cmd_name, args) = match os {
        "macos" => {
            let pkg = match tool {
                "java" | "jdk" => "openjdk",
                "c++" | "cpp" | "gcc" | "g++" => "gcc",
                "cmake" => "cmake",
                "php" => "php",
                _ => tool,
            };
            ("brew", vec!["install", pkg])
        }
        "linux" => {
            let pkg = match tool {
                "java" | "jdk" => "default-jdk",
                "c++" | "cpp" | "gcc" | "g++" => "build-essential",
                "cmake" => "cmake",
                "php" => "php",
                _ => tool,
            };
            ("sudo", vec!["apt-get", "install", "-y", pkg])
        }
        "windows" => {
            let pkg = match tool {
                "java" | "jdk" => "Microsoft.OpenJDK",
                "c++" | "cpp" | "gcc" | "g++" => "Microsoft.VisualStudio.Workloads.VCTools",
                "cmake" => "Kitware.CMake",
                "php" => "PHP.PHP",
                _ => tool,
            };
            ("winget", vec!["install", "-e", "--id", pkg, "--accept-package-agreements", "--accept-source-agreements"])
        }
        _ => return ToolResult::err(&format!("❌ Unsupported OS for system package manager: {}", os)),
    };

    let mut cmd = Command::new(cmd_name);
    cmd.args(&args);
    
    let cmd_display = format!("{} {}", cmd_name, args.join(" "));
    
    match cmd.output().await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            
            if output.status.success() {
                ToolResult::ok(&format!("✅ Successfully installed via system package manager:\n$ {}\n{}", cmd_display, stdout))
            } else {
                ToolResult::err(&format!("❌ Failed to install via system package manager:\n$ {}\n{}\n{}", cmd_display, stderr, stdout))
            }
        }
        Err(e) => ToolResult::err(&format!("❌ Failed to execute '{}': {}", cmd_display, e)),
    }
}

async fn list_installed_tools() -> ToolResult {
    match Command::new(proto_bin()).arg("plugin").arg("list").arg("--versions").output().await {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                ToolResult::ok(&format!("📦 Installed tools:\n{}", stdout))
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                ToolResult::err(&format!("❌ Failed to list installed tools: {}", stderr))
            }
        }
        Err(e) => ToolResult::err(&format!("❌ Failed to execute proto plugin list: {}", e)),
    }
}

async fn list_available_tools(tool: Option<&str>) -> ToolResult {
    let mut cmd = Command::new(proto_bin());
    cmd.arg("plugin").arg("list");
    
    if let Some(t) = tool {
        cmd.arg(t);
    }
    
    match cmd.output().await {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                ToolResult::ok(&format!("🔍 Available tools:\n{}", stdout))
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                ToolResult::err(&format!("❌ Failed to list available tools: {}", stderr))
            }
        }
        Err(e) => ToolResult::err(&format!("❌ Failed to execute proto plugin list: {}", e)),
    }
}

async fn get_tool_versions(tool: &str) -> ToolResult {
    match Command::new(proto_bin()).arg("versions").arg(tool).output().await {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                ToolResult::ok(&format!("📋 Available versions for {}:\n{}", tool, stdout))
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                ToolResult::err(&format!("❌ Failed to get versions for {}: {}", tool, stderr))
            }
        }
        Err(e) => ToolResult::err(&format!("❌ Failed to execute proto versions: {}", e)),
    }
}

async fn uninstall_tool(tool: &str, version: &str) -> ToolResult {
    match Command::new(proto_bin()).arg("uninstall").arg(tool).arg(version).output().await {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                ToolResult::ok(&format!("🗑️ Successfully uninstalled {} {}\n{}", tool, version, stdout))
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                ToolResult::err(&format!("❌ Failed to uninstall {} {}: {}", tool, version, stderr))
            }
        }
        Err(e) => ToolResult::err(&format!("❌ Failed to execute proto uninstall: {}", e)),
    }
}

async fn detect_project_tools(workdir: &std::path::Path) -> ToolResult {
    let mut suggestions = Vec::new();
    
    if workdir.join("Cargo.toml").exists() {
        suggestions.push("🦀 Rust project detected - suggested tool: rust (stable)");
    }
    
    if workdir.join("package.json").exists() {
        suggestions.push("📦 Node.js project detected - suggested tool: node (lts)");
    }
    
    if workdir.join("requirements.txt").exists() || 
       workdir.join("pyproject.toml").exists() ||
       workdir.join("setup.py").exists() {
        suggestions.push("🐍 Python project detected - suggested tool: python (3.12)");
    }
    
    if workdir.join("go.mod").exists() {
        suggestions.push("🐹 Go project detected - suggested tool: go (1.22)");
    }
    
    if workdir.join("deno.json").exists() || workdir.join("deno.jsonc").exists() {
        suggestions.push("🦕 Deno project detected - suggested tool: deno (latest)");
    }
    
    if workdir.join("bun.lockb").exists() {
        suggestions.push("🧄 Bun project detected - suggested tool: bun (latest)");
    }
    
    if workdir.join("pom.xml").exists() || workdir.join("build.gradle").exists() {
        suggestions.push("☕ Java project detected - suggested tool: java");
    }
    
    if workdir.join("CMakeLists.txt").exists() || workdir.join("Makefile").exists() {
        suggestions.push("⚙️ C/C++ project detected - suggested tool: cpp");
    }
    
    if workdir.join("composer.json").exists() {
        suggestions.push("🐘 PHP project detected - suggested tool: php");
    }
    
    if suggestions.is_empty() {
        ToolResult::ok("🔍 No specific project tools detected in current directory")
    } else {
        ToolResult::ok(&format!("🎯 Project analysis:\n{}", suggestions.join("\n")))
    }
}