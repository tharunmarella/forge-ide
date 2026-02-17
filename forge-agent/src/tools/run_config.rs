//! Run configuration tools - integrate with IDE's run system.
//!
//! These tools allow the agent to:
//! - List detected run configurations (npm scripts, cargo bins, etc.)
//! - Run projects through the IDE's proper run system
//! - Stop running processes

use serde_json::Value;
use crate::tools::ToolResult;

/// List all available run configurations detected from the project.
///
/// This automatically detects:
/// - npm/yarn/pnpm scripts from package.json
/// - Cargo workspace members and binary targets
/// - Python main modules
/// - Go main packages
/// - Maven/Gradle tasks
/// - VSCode launch.json configurations
///
/// Returns a formatted list of available run configurations.
pub async fn list_run_configs(args: &Value, _workdir: &std::path::Path) -> ToolResult {
    // This will be handled by the IDE via RPC
    // The IDE has the full context of detected configs
    ToolResult::ok("PENDING_IDE_EXECUTION")
}

/// Run a project using the IDE's run configuration system.
///
/// This is BETTER than execute_command because:
/// - Opens in a proper terminal tab with UI
/// - Respects the project's environment setup
/// - Integrates with the debug system
/// - Provides better output handling
/// - Auto-detects the correct working directory
///
/// The agent should:
/// 1. First call list_run_configs() to see available options
/// 2. Then call run_project() with a config name or custom command
///
/// Examples:
/// - run_project with config_name="npm run dev"
/// - run_project with config_name="cargo run --release"
/// - run_project with command="python -m pytest tests/"
pub async fn run_project(args: &Value, _workdir: &std::path::Path) -> ToolResult {
    let config_name = args.get("config_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    
    let command = args.get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    
    let mode = args.get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("run");
    
    // Validate inputs
    if config_name.is_empty() && command.is_empty() {
        return ToolResult::err("Either 'config_name' or 'command' must be provided");
    }
    
    // Validate mode
    if mode != "run" && mode != "debug" {
        return ToolResult::err("mode must be 'run' or 'debug'");
    }
    
    // This will be executed by the IDE
    ToolResult::ok("PENDING_IDE_EXECUTION")
}

/// Stop a running project/process.
///
/// This stops processes started with run_project().
/// If no config_name is provided, stops the most recently started process.
pub async fn stop_project(args: &Value, _workdir: &std::path::Path) -> ToolResult {
    let _config_name = args.get("config_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    
    // This will be executed by the IDE
    ToolResult::ok("PENDING_IDE_EXECUTION")
}
