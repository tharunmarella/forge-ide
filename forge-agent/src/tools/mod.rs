mod execute;
pub mod files;
pub(crate) mod search;
mod code;
mod process;
mod treesitter;
pub mod lint;
mod display;
mod run_config;
mod git;
mod sdk_manager;
pub mod lsp;
pub mod web;

pub use lint::{lint_file, LintResult, LintError, LintSeverity};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

pub use files::*;
pub use search::*;
pub use code::*;
pub use process::*;
pub use display::*;
pub use run_config::*;
pub use git::*;
pub use sdk_manager::*;
pub use web::*;

// Re-export ensure_indexed for external callers (lapce-proxy)
pub use search::ensure_indexed;

/// All available tools
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    // File operations
    ReadFile,
    WriteFile,      // write_file (was write_to_file)
    EditFile,       // edit_file  (was replace_in_file)
    ApplyPatch,
    ListFiles,
    DeleteFile,

    // Search
    Grep,
    Glob,

    // Diagnostics
    Diagnostics,

    // Process management — consolidated
    Run,            // run(command, background?, timeout_secs?)
    Process,        // process(pid, action, lines?)
    Port,           // port(port_num, action, ...)

    // Code intelligence
    References,     // references(symbol, path?) — was find_symbol_references
    Lsp,            // lsp(action, path, line, column, new_name?)

    // Display
    ShowCode,
    ShowDiagram,

    // Run configuration
    RunProject,
    StopProject,
    ListRunConfigs,

    // Git operations
    Git,

    // SDK Management
    SdkManager,

    // Web
    Fetch,
    WorkspaceSymbols,

    // Interaction
    AttemptCompletion,
    AskFollowupQuestion,
    Think,

    // Mode control (internal)
    PlanModeRespond,
    ActModeRespond,
    FocusChain,
}

impl Tool {
    pub fn name(&self) -> &'static str {
        match self {
            // New canonical names
            Self::ReadFile => "read_file",
            Self::WriteFile => "write_file",
            Self::EditFile => "edit_file",
            Self::ApplyPatch => "apply_patch",
            Self::ListFiles => "list_files",
            Self::DeleteFile => "delete_file",
            Self::Grep => "grep",
            Self::Glob => "glob",
            Self::Diagnostics => "diagnostics",
            Self::Run => "run",
            Self::Process => "process",
            Self::Port => "port",
            Self::References => "references",
            Self::Lsp => "lsp",
            Self::ShowCode => "show_code",
            Self::ShowDiagram => "show_diagram",
            Self::RunProject => "run_project",
            Self::StopProject => "stop_project",
            Self::Git => "git",
            Self::SdkManager => "sdk_manager",
            Self::Fetch => "fetch",
            Self::WorkspaceSymbols => "workspace_symbols",
            Self::ListRunConfigs => "list_run_configs",
            // Interaction
            Self::AttemptCompletion => "attempt_completion",
            Self::AskFollowupQuestion => "ask_followup_question",
            Self::Think => "think",
            Self::PlanModeRespond => "plan_mode_respond",
            Self::ActModeRespond => "act_mode_respond",
            Self::FocusChain => "focus_chain",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            // New canonical names
            "read_file"    => Some(Self::ReadFile),
            "write_file"   => Some(Self::WriteFile),
            "edit_file"    => Some(Self::EditFile),
            "apply_patch"  => Some(Self::ApplyPatch),
            "list_files"   => Some(Self::ListFiles),
            "delete_file"  => Some(Self::DeleteFile),
            "grep"         => Some(Self::Grep),
            "glob"         => Some(Self::Glob),
            "diagnostics"  => Some(Self::Diagnostics),
            "run"          => Some(Self::Run),
            "process"      => Some(Self::Process),
            "port"         => Some(Self::Port),
            "references"   => Some(Self::References),
            "lsp"          => Some(Self::Lsp),
            "show_code"    => Some(Self::ShowCode),
            "show_diagram" => Some(Self::ShowDiagram),
            "run_project"  => Some(Self::RunProject),
            "stop_project" => Some(Self::StopProject),
            "git"          => Some(Self::Git),
            "sdk_manager"  => Some(Self::SdkManager),
            "fetch"             => Some(Self::Fetch),
            "workspace_symbols" => Some(Self::WorkspaceSymbols),
            "list_run_configs"  => Some(Self::ListRunConfigs),
            "attempt_completion"       => Some(Self::AttemptCompletion),
            "ask_followup_question"    => Some(Self::AskFollowupQuestion),
            "think"                    => Some(Self::Think),
            "plan_mode_respond"        => Some(Self::PlanModeRespond),
            "act_mode_respond"         => Some(Self::ActModeRespond),
            "focus_chain"              => Some(Self::FocusChain),
            _ => None,
        }
    }

    /// Returns true if tool modifies workspace
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::WriteFile
                | Self::EditFile
                | Self::ApplyPatch
                | Self::DeleteFile
                | Self::Run
                | Self::Process  // kill action
                | Self::Port     // kill action
                | Self::Lsp      // rename action
        )
    }
}

/// Tool call from LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
    /// Gemini 3 thought signature (must be passed back for function calling)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

/// Metadata about a file edit performed by a tool (for diff preview).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEditMeta {
    /// Relative path within the workspace.
    pub path: String,
    /// Original file content before edit (empty for new files).
    pub old_content: String,
    /// New file content after edit.
    pub new_content: String,
}

/// Result of executing a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    /// If this tool wrote/modified a file, this contains the before/after content
    /// so the proxy can compute a diff preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_edit: Option<FileEditMeta>,
    /// Set to true when the tool was blocked and needs user approval.
    /// The caller should show a confirmation dialog and re-execute if approved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_approval: Option<bool>,
}

impl ToolResult {
    pub fn ok(output: impl Into<String>) -> Self {
        Self { success: true, output: output.into(), file_edit: None, needs_approval: None }
    }

    pub fn err(output: impl Into<String>) -> Self {
        Self { success: false, output: output.into(), file_edit: None, needs_approval: None }
    }

    /// Attach file edit metadata (for diff preview).
    pub fn with_file_edit(mut self, meta: FileEditMeta) -> Self {
        self.file_edit = Some(meta);
        self
    }

    /// Mark this result as needing user approval before execution.
    pub fn awaiting_approval(tool_name: &str, summary: &str) -> Self {
        Self {
            success: false,
            output: format!("[APPROVAL REQUIRED] Tool '{}' wants to: {}", tool_name, summary),
            file_edit: None,
            needs_approval: Some(true),
        }
    }
}

// ── Approval policy ─────────────────────────────────────────────────

/// Controls which tools require user approval before execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    /// All tools run without approval (current default).
    AutoApproveAll,
    /// Mutating tools (write, replace, patch, delete, execute_command)
    /// require approval. Read-only tools auto-approve.
    ApproveMutations,
    /// Every tool requires approval.
    ApproveAll,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self::AutoApproveAll
    }
}

/// Callback the caller provides so `execute()` can ask for approval.
/// Returns `true` if the user approved, `false` to reject.
///
/// When no callback is provided, the tool runs without approval.
pub type ApprovalCallback = Box<dyn Fn(&str, &str) -> bool + Send + Sync>;

/// Options for `execute()`.
pub struct ExecuteOptions {
    pub plan_mode: bool,
    pub approval_policy: ApprovalPolicy,
    /// Optional callback for synchronous approval.
    /// Receives (tool_name, human-readable summary).
    pub approval_callback: Option<ApprovalCallback>,
    /// Optional loop detector (shared across the agent session).
    pub loop_detector: Option<std::sync::Arc<std::sync::Mutex<crate::loop_detection::LoopDetector>>>,
}

impl Default for ExecuteOptions {
    fn default() -> Self {
        Self {
            plan_mode: false,
            approval_policy: ApprovalPolicy::AutoApproveAll,
            approval_callback: None,
            loop_detector: None,
        }
    }
}

/// Execute a tool call (simple API — no approval, no loop detection).
/// Kept for backward compatibility.
pub async fn execute(tool: &ToolCall, workdir: &Path, plan_mode: bool) -> ToolResult {
    execute_with_options(tool, workdir, &ExecuteOptions { plan_mode, ..Default::default() }).await
}

/// Execute a tool call with full options (approval, loop detection).
pub async fn execute_with_options(tool: &ToolCall, workdir: &Path, opts: &ExecuteOptions) -> ToolResult {
    use std::time::Instant;
    let start = Instant::now();
    
    let Some(t) = Tool::from_name(&tool.name) else {
        return ToolResult::err(format!("Unknown tool: {}", tool.name));
    };

    // Block mutating tools in plan mode
    if opts.plan_mode && t.is_mutating() {
        return ToolResult::err("Cannot modify files in plan mode");
    }

    // ── Loop detection ──────────────────────────────────────────
    if let Some(ref detector) = opts.loop_detector {
        let args_json = serde_json::to_string(&tool.arguments).unwrap_or_default();
        if let Ok(mut d) = detector.lock() {
            let check = d.check_tool_call(&tool.name, &args_json);
            if check.is_loop() {
                tracing::warn!("🔄 {}", check.message());
                return ToolResult::err(check.message());
            }
        }
    }

    // ── Approval check ──────────────────────────────────────────
    let needs_approval = match opts.approval_policy {
        ApprovalPolicy::AutoApproveAll => false,
        ApprovalPolicy::ApproveMutations => t.is_mutating(),
        ApprovalPolicy::ApproveAll => true,
    };

    if needs_approval {
        let summary = make_approval_summary(tool);
        if let Some(ref callback) = opts.approval_callback {
            if !callback(&tool.name, &summary) {
                return ToolResult::err(format!(
                    "Tool '{}' was rejected by user. Try a different approach.",
                    tool.name
                ));
            }
        } else {
            // No callback — return a result that signals "needs approval".
            // The caller (dispatch.rs) can show a UI dialog and re-execute.
            return ToolResult::awaiting_approval(&tool.name, &summary);
        }
    }

    // ── Execute ─────────────────────────────────────────────────
    let result = match t {
        // ── New canonical tools ───────────────────────────────────────────
        Tool::ReadFile => files::read(&tool.arguments, workdir).await,
        Tool::WriteFile => files::write(&tool.arguments, workdir).await,
        Tool::EditFile => files::replace(&tool.arguments, workdir).await,
        Tool::ApplyPatch => files::apply_patch(&tool.arguments, workdir).await,
        Tool::ListFiles => files::list(&tool.arguments, workdir).await,
        Tool::DeleteFile => files::delete(&tool.arguments, workdir).await,
        Tool::Grep => search::grep(&tool.arguments, workdir).await,
        Tool::Glob => search::glob_search(&tool.arguments, workdir).await,
        Tool::Diagnostics => lint::diagnostics(&tool.arguments, workdir).await,
        Tool::Run => process::run_command(&tool.arguments, workdir).await,
        Tool::Process => process::manage_process(&tool.arguments, workdir).await,
        Tool::Port => process::manage_port(&tool.arguments, workdir).await,
        Tool::References => code::find_references(&tool.arguments, workdir).await,
        Tool::Lsp => ToolResult::err("lsp tool must be executed via ProxyBridge in dispatch.rs"),
        Tool::ShowCode => display::show_code(&tool.arguments, workdir).await,
        Tool::ShowDiagram => display::show_diagram(&tool.arguments, workdir).await,
        Tool::RunProject => run_config::run_project(&tool.arguments, workdir).await,
        Tool::StopProject => run_config::stop_project(&tool.arguments, workdir).await,
        Tool::Git => git::git(&tool.arguments, workdir).await,
        Tool::SdkManager => sdk_manager::sdk_manager(&tool.arguments, workdir).await,
        Tool::Fetch => web::fetch_webpage(&tool.arguments).await,
        Tool::WorkspaceSymbols => search::workspace_symbols(&tool.arguments, workdir).await,
        Tool::ListRunConfigs => run_config::list_run_configs(&tool.arguments, workdir).await,

        // Handled specially by the agent
        Tool::AttemptCompletion
        | Tool::AskFollowupQuestion
        | Tool::PlanModeRespond
        | Tool::ActModeRespond
        | Tool::FocusChain
        | Tool::Think => ToolResult::ok(""),
    };
    
    let elapsed = start.elapsed();
    if elapsed.as_millis() > 100 {
        tracing::info!("⏱ Tool {} completed in {:?}", tool.name, elapsed);
    } else {
        tracing::debug!("⏱ Tool {} completed in {:?}", tool.name, elapsed);
    }
    
    result
}

/// Build a human-readable summary of what a tool call will do (for approval dialogs).
fn make_approval_summary(tool: &ToolCall) -> String {
    match tool.name.as_str() {
        "run" => {
            let cmd = tool.arguments.get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            let bg = tool.arguments.get("background").and_then(|v| v.as_bool()).unwrap_or(false);
            if bg {
                format!("Run in background: {}", &cmd[..cmd.len().min(200)])
            } else {
                format!("Run command: {}", &cmd[..cmd.len().min(200)])
            }
        }
        "write_file" => {
            let path = tool.arguments.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            let size = tool.arguments.get("content")
                .and_then(|v| v.as_str())
                .map(|c| c.len())
                .unwrap_or(0);
            format!("Write {} ({} bytes)", path, size)
        }
        "edit_file" => {
            let path = tool.arguments.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            format!("Edit {}", path)
        }
        "apply_patch" => "Apply multi-file patch".to_string(),
        "delete_file" => {
            let path = tool.arguments.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            format!("Delete {}", path)
        }
        "process" => {
            let action = tool.arguments.get("action").and_then(|v| v.as_str()).unwrap_or("?");
            let pid = tool.arguments.get("pid").and_then(|v| v.as_u64())
                .map(|p| p.to_string()).unwrap_or_else(|| "<unknown>".to_string());
            format!("Process {} PID {}", action, pid)
        }
        "port" => {
            let action = tool.arguments.get("action").and_then(|v| v.as_str()).unwrap_or("?");
            let port = tool.arguments.get("port").and_then(|v| v.as_u64())
                .map(|p| p.to_string()).unwrap_or_else(|| "<unknown>".to_string());
            format!("Port {} :{}", action, port)
        }
        "lsp" => {
            let action = tool.arguments.get("action").and_then(|v| v.as_str()).unwrap_or("?");
            let path = tool.arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
            format!("LSP {} in {}", action, path)
        }
        _ => format!("Execute tool '{}'", tool.name),
    }
}

/// Generate tool definitions for LLM
pub fn definitions(plan_mode: bool) -> Vec<Value> {
    let mut tools = vec![
        serde_json::json!({
            "name": "run",
            "description": "Execute a shell command. Commands time out after 120 seconds by default. Set background=true for long-running processes (dev servers, watchers). CRITICAL: Do not use for interactive commands that require user input.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to execute" },
                    "background": { "type": "boolean", "description": "Run in background and return immediately with PID (default: false)" },
                    "timeout_secs": { "type": "integer", "description": "Optional timeout in seconds (default 120, max 600)" }
                },
                "required": ["command"]
            }
        }),
        serde_json::json!({
            "name": "read_file",
            "description": "Read the contents of a file",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" },
                    "start_line": { "type": "integer", "description": "Optional start line (1-indexed)" },
                    "end_line": { "type": "integer", "description": "Optional end line" }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "write_file",
            "description": "Create or overwrite a file with the given content.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path for the file" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }
        }),
        serde_json::json!({
            "name": "edit_file",
            "description": "Replace an exact string in a file. old_str must match exactly (including whitespace and indentation).",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" },
                    "old_str": { "type": "string", "description": "Exact text to find and replace" },
                    "new_str": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "old_str", "new_str"]
            }
        }),
        serde_json::json!({
            "name": "apply_patch",
            "description": "Apply a patch to one or more files. Supports two formats:\n1. V4A format (multi-file): Use 'input' parameter with *** Begin Patch / *** Update File: / *** End Patch markers\n2. Unified diff format (single file): Use 'path' and 'patch' parameters",
            "parameters": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "V4A format patch with *** Begin Patch, *** Update File:, - removals, + additions" },
                    "path": { "type": "string", "description": "Path to file (for unified diff format)" },
                    "patch": { "type": "string", "description": "Unified diff patch content (for single file)" }
                }
            }
        }),
        serde_json::json!({
            "name": "list_files",
            "description": "List files in a directory",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path" },
                    "recursive": { "type": "boolean", "description": "List recursively" }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "delete_file",
            "description": "Delete a file or empty directory. Protected paths like .git, node_modules, Cargo.toml cannot be deleted.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to file or directory to delete" }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "process",
            "description": "Manage background processes. Actions: output (read stdout/stderr from PID), status (check if running), kill (terminate by PID).",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["output", "status", "kill"], "description": "Process action to perform" },
                    "pid": { "type": "integer", "description": "Process ID (required for output/kill; omit for status to list all)" },
                    "tail_lines": { "type": "integer", "description": "Lines from end to return for output (default: 100)" },
                    "force": { "type": "boolean", "description": "Use SIGKILL instead of SIGTERM for kill (default: false)" }
                },
                "required": ["action"]
            }
        }),
        serde_json::json!({
            "name": "port",
            "description": "Manage ports. Actions: check (is port in use?), wait (block until port accepts connections), kill (terminate process using port).",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["check", "wait", "kill"], "description": "Port action to perform" },
                    "port": { "type": "integer", "description": "Port number" },
                    "host": { "type": "string", "description": "Host (default: localhost)" },
                    "timeout": { "type": "integer", "description": "Max seconds to wait (for wait action, default: 30)" },
                    "http_check": { "type": "boolean", "description": "Also verify HTTP 2xx/3xx (for wait action)" },
                    "force": { "type": "boolean", "description": "Use SIGKILL for kill action (default: false)" }
                },
                "required": ["action", "port"]
            }
        }),
        // SEARCH TOOLS - order matters for model selection
        serde_json::json!({
            "name": "codebase_search",
            "description": "SEMANTIC/CONCEPTUAL search - find code by meaning. Use for understanding ('how does X work'), finding related code ('authentication logic'), or exploring unfamiliar areas. This is the PRIMARY search tool.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural language query describing what you're looking for" },
                    "path": { "type": "string", "description": "Optional: limit search to directory" }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "grep",
            "description": "LITERAL text search - use ONLY when you know the exact string to find (specific function name, error message, import statement). Fast but requires exact match.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Exact text or regex pattern" },
                    "path": { "type": "string", "description": "Directory to search (default: current)" },
                    "glob": { "type": "string", "description": "File filter, e.g., '*.rs'" },
                    "case_insensitive": { "type": "boolean", "description": "Ignore case" },
                    "context": { "type": "integer", "description": "Context lines (0-5)" }
                },
                "required": ["pattern"]
            }
        }),
        serde_json::json!({
            "name": "glob",
            "description": "Find files by name/extension pattern. Returns file paths only.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Pattern like '*.rs', '**/*.test.ts'" },
                    "path": { "type": "string", "description": "Base directory" }
                },
                "required": ["pattern"]
            }
        }),
        serde_json::json!({
            "name": "diagnostics",
            "description": "Get compiler/linter errors and warnings for a file or directory. Use this to check code for errors before or after making changes.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File or directory to check. For directories, runs the appropriate build tool (cargo check, tsc, etc.)" }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "lsp",
            "description": "Language server operations — 100% accurate code intelligence from the IDE's LSP client. Actions: definition (jump to exact definition), references (find all usages), hover (get type info and docs), rename (atomically rename symbol everywhere).",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["definition", "references", "hover", "rename"], "description": "LSP action to perform" },
                    "path": { "type": "string", "description": "File path (relative to workspace root)" },
                    "line": { "type": "integer", "description": "1-indexed line number" },
                    "column": { "type": "integer", "description": "1-indexed column number" },
                    "new_name": { "type": "string", "description": "New identifier name (for rename action only)" }
                },
                "required": ["action", "path", "line", "column"]
            }
        }),
        serde_json::json!({
            "name": "workspace_symbols",
            "description": "Search for symbols (functions, classes, variables) across the entire workspace using LSP.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Symbol name or partial name to search for" }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "show_code",
            "description": "Display a code block in the chat with syntax highlighting. Use this to show code examples, explain code snippets, or present generated code before writing it to a file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "The code to display" },
                    "language": { "type": "string", "description": "Programming language for syntax highlighting (e.g., 'rust', 'python', 'javascript', 'typescript', 'json'). Default: 'plaintext'" },
                    "title": { "type": "string", "description": "Optional title/description for the code block" }
                },
                "required": ["code"]
            }
        }),
        serde_json::json!({
            "name": "show_diagram",
            "description": "Display a Mermaid diagram in the chat. Use this to visualize system architecture, workflows, class diagrams, sequence diagrams, or any concept that benefits from visual representation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "diagram_code": { "type": "string", "description": "Mermaid diagram code (e.g., 'graph TD; A-->B; B-->C;')" },
                    "title": { "type": "string", "description": "Optional title/description for the diagram" }
                },
                "required": ["diagram_code"]
            }
        }),
        serde_json::json!({
            "name": "list_run_configs",
            "description": "List all available run configurations detected from the project. This automatically finds npm/yarn scripts, cargo bins, python modules, go packages, and other runnable targets. Use this BEFORE running a project to see what's available.",
            "parameters": {
                "type": "object",
                "properties": {}
            }
        }),
        serde_json::json!({
            "name": "run_project",
            "description": "Run the project using the IDE's run configuration system. This opens a proper terminal tab with UI integration. BETTER than execute_command for running apps, servers, tests, or build scripts. First call list_run_configs() to see available options, then use this tool.",
            "parameters": {
                "type": "object",
                "properties": {
                    "config_name": { "type": "string", "description": "Name of detected run configuration (e.g., 'npm run dev', 'cargo run', 'python main.py'). Get this from list_run_configs()." },
                    "command": { "type": "string", "description": "Custom command to run if config_name not provided. Use config_name when possible." },
                    "mode": { "type": "string", "enum": ["run", "debug"], "description": "Run mode: 'run' for normal execution, 'debug' to enable breakpoints. Default: 'run'" }
                }
            }
        }),
        serde_json::json!({
            "name": "stop_project",
            "description": "Stop a running project/process started with run_project(). If no config_name provided, stops the most recently started process.",
            "parameters": {
                "type": "object",
                "properties": {
                    "config_name": { "type": "string", "description": "Name of the run configuration to stop. If not provided, stops the most recent one." }
                }
            }
        }),
        serde_json::json!({
            "name": "git",
            "description": "Unified git tool for essential source control operations. Integrates with IDE's native git for proper UI updates. Operations: status (check repo), stage/unstage (paths), commit (message), push/pull, branch (list/create/switch), log (history), diff (file changes).",
            "parameters": {
                "type": "object",
                "properties": {
                    "operation": { 
                        "type": "string", 
                        "enum": ["status", "stage", "unstage", "commit", "push", "pull", "branch", "log", "diff"],
                        "description": "Git operation to perform" 
                    },
                    "paths": { 
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "File paths (for stage/unstage operations)" 
                    },
                    "message": { 
                        "type": "string", 
                        "description": "Commit message (for commit operation)" 
                    },
                    "action": { 
                        "type": "string", 
                        "enum": ["list", "create", "switch"],
                        "description": "Branch action (for branch operation)" 
                    },
                    "name": { 
                        "type": "string", 
                        "description": "Branch name (for branch create/switch)" 
                    },
                    "limit": { 
                        "type": "integer", 
                        "description": "Number of commits to show (for log operation, default: 10)" 
                    },
                    "path": { 
                        "type": "string", 
                        "description": "File path (for diff operation)" 
                    },
                    "staged": { 
                        "type": "boolean", 
                        "description": "Show staged changes (for diff operation, default: false)" 
                    }
                },
                "required": ["operation"]
            }
        }),
        serde_json::json!({
            "name": "sdk_manager",
            "description": "Manage development tools and runtimes (Node.js, Python, Rust, Go, etc.) via proto. Better than raw commands - handles cross-platform installation, version management, and project detection automatically. Operations: install, list_installed, list_available, detect_project, uninstall, versions.",
            "parameters": {
                "type": "object",
                "properties": {
                    "operation": { 
                        "type": "string", 
                        "enum": ["install", "list_installed", "list_available", "detect_project", "uninstall", "versions"],
                        "description": "SDK management operation to perform" 
                    },
                    "tool": { 
                        "type": "string", 
                        "description": "Tool name (e.g., 'node', 'python', 'rust', 'go')" 
                    },
                    "version": { 
                        "type": "string",
                        "description": "Specific version to install/uninstall (e.g., '18.0.0', 'latest')" 
                    },
                    "pin": {
                        "type": "boolean",
                        "description": "Whether to pin the installed version (make it available in PATH). Defaults to true."
                    }
                },
                "required": ["operation"]
            }
        }),
        serde_json::json!({
            "name": "ask_followup_question",
            "description": "Ask the user a clarifying question",
            "parameters": {
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "The question to ask" }
                },
                "required": ["question"]
            }
        }),
        serde_json::json!({
            "name": "think",
            "description": "Write out your reasoning or thoughts about the current task",
            "parameters": {
                "type": "object",
                "properties": {
                    "thought": { "type": "string", "description": "Your reasoning or thoughts" }
                },
                "required": ["thought"]
            }
        }),
        serde_json::json!({
            "name": "attempt_completion",
            "description": "Signal task completion with a result message",
            "parameters": {
                "type": "object",
                "properties": {
                    "result": { "type": "string", "description": "Summary of what was done" }
                },
                "required": ["result"]
            }
        }),
    ];

    // Filter out mutating tools in plan mode
    if plan_mode {
        tools.retain(|t| {
            let name = t["name"].as_str().unwrap_or("");
            !matches!(name, "run" | "write_file" | "edit_file" | "apply_patch" | "delete_file")
        });
    }

    tools
}
