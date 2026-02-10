//! Tool implementations using rig's `Tool` trait.
//!
//! Each tool wraps functionality from our `tools/` module and exposes it
//! to the rig agent framework.  Tools delegate to the real implementations
//! in `tools/{files,search,web,lint,execute}.rs` -- this avoids duplicating
//! logic and gives the agent the full feature set (line ranges, ripgrep,
//! security guards, semantic search, etc.).

use std::sync::Arc;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::bridge::ProxyBridge;
use crate::tools;

// ── Shared error type ────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ToolExecError(pub String);

/// Invalidate cached context after a file mutation (write, replace, delete, patch).
/// This ensures the RepoMap and pre-search results reflect the new state.
fn invalidate_cache_for(bridge: &Arc<dyn ProxyBridge>) {
    crate::context_cache::global().invalidate_workspace(bridge.workspace_root());
}

/// Convert a `tools::ToolResult` into a rig-compatible `Result`.
///
/// Tool "soft failures" (e.g. file not found, no matches) are returned
/// as `Ok` with the error message so the LLM can see and react to them.
/// Framework-level panics become `Err`.
///
/// Output is intelligently masked using head+tail truncation instead of a hard cut.
fn to_result(r: tools::ToolResult) -> Result<String, ToolExecError> {
    let output = if r.success {
        r.output
    } else {
        format!("ERROR: {}", r.output)
    };
    Ok(crate::output_masking::mask_output(&output, crate::output_masking::OutputKind::Default).text)
}

/// Convert a shell tool result, preserving exit codes and error info.
fn to_result_shell(r: tools::ToolResult) -> Result<String, ToolExecError> {
    let output = if r.success {
        r.output
    } else {
        format!("ERROR: {}", r.output)
    };
    Ok(crate::output_masking::mask_output(&output, crate::output_masking::OutputKind::Shell).text)
}

// ══════════════════════════════════════════════════════════════════
//  FILE OPERATIONS (5 tools)
// ══════════════════════════════════════════════════════════════════

// ── 1. ReadFile ──────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct ReadFileArgs {
    /// File path relative to workspace root
    pub path: String,
    /// Optional start line (1-indexed)
    #[serde(default)]
    pub start_line: Option<u64>,
    /// Optional end line (1-indexed)
    #[serde(default)]
    pub end_line: Option<u64>,
}

#[derive(Clone)]
pub struct ReadFileTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for ReadFileTool {
    const NAME: &'static str = "read_file";
    type Error = ToolExecError;
    type Args = ReadFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read file contents with line numbers. \
WHEN TO USE: Before editing any file, to understand file structure, to check specific code sections. \
TIPS: For large files (>300 lines), use start_line/end_line to read specific sections instead of the whole file. \
Output is auto-truncated for files over 300 lines when no range is specified.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to workspace root" },
                    "start_line": { "type": "integer", "description": "Optional start line (1-indexed)" },
                    "end_line": { "type": "integer", "description": "Optional end line (1-indexed)" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let has_range = args.start_line.is_some() || args.end_line.is_some();
        let mut json = serde_json::json!({ "path": args.path });
        if let Some(sl) = args.start_line {
            json["start_line"] = serde_json::json!(sl);
        }
        if let Some(el) = args.end_line {
            json["end_line"] = serde_json::json!(el);
        }
        let result = tools::read(&json, self.bridge.workspace_root()).await;

        // Auto-truncate large files when no line range was specified
        if result.success && !has_range {
            let line_count = result.output.lines().count();
            if line_count > 300 {
                let truncated: String = result.output
                    .lines()
                    .take(200)
                    .collect::<Vec<_>>()
                    .join("\n");
                return Ok(format!(
                    "{}\n\n[File has {} total lines. Showing first 200. Use start_line/end_line to read specific sections.]",
                    truncated, line_count
                ));
            }
        }

        to_result(result)
    }
}

// ── 2. WriteFile ─────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct WriteFileArgs {
    /// File path relative to workspace root
    pub path: String,
    /// Content to write
    pub content: String,
}

#[derive(Clone)]
pub struct WriteFileTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for WriteFileTool {
    const NAME: &'static str = "write_to_file";
    type Error = ToolExecError;
    type Args = WriteFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "write_to_file".to_string(),
            description: "Create a NEW file or overwrite an existing file. Parent directories are created automatically. \
WHEN TO USE: Creating new files only. \
WHEN NOT TO USE: For editing existing files -- use replace_in_file instead. \
IMPORTANT: Always prefer editing existing files over creating new ones.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to workspace root" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let json = serde_json::json!({ "path": args.path, "content": args.content });
        let r = tools::write(&json, self.bridge.workspace_root()).await;
        if r.success { invalidate_cache_for(&self.bridge); }
        to_result(r)
    }
}

// ── 3. ReplaceInFile ─────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct ReplaceInFileArgs {
    /// File path relative to workspace root
    pub path: String,
    /// Exact text to find (must be unique in the file). Not needed if using start_line/end_line.
    #[serde(default)]
    pub old_str: Option<String>,
    /// Replacement text
    pub new_str: String,
    /// Optional: start line (1-indexed) for line-range replacement. Use with end_line instead of old_str when old_str matches multiple places.
    #[serde(default)]
    pub start_line: Option<u64>,
    /// Optional: end line (1-indexed, inclusive) for line-range replacement.
    #[serde(default)]
    pub end_line: Option<u64>,
    /// Optional: description of what this edit is trying to do (helps self-correction if the edit fails).
    #[serde(default)]
    pub instruction: Option<String>,
}

#[derive(Clone)]
pub struct ReplaceInFileTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for ReplaceInFileTool {
    const NAME: &'static str = "replace_in_file";
    type Error = ToolExecError;
    type Args = ReplaceInFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "replace_in_file".to_string(),
            description: "Replace text in a file. PREFERRED way to edit existing files. \
Uses 3 automatic matching strategies: exact -> flexible (whitespace-tolerant) -> regex. \
If all fail, an LLM self-correction attempt is made automatically. \
Two modes: (1) old_str mode: provide old_str (the system will try flexible matching if exact fails). \
If it matches multiple places, the error will show line numbers. \
(2) Line-range mode: use start_line + end_line (1-indexed) instead of old_str to replace a specific line range. \
ALWAYS read_file first to see exact content before editing.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to workspace root" },
                    "old_str": { "type": "string", "description": "Text to find (flexible matching applied automatically). Omit if using start_line/end_line." },
                    "new_str": { "type": "string", "description": "Replacement text" },
                    "start_line": { "type": "integer", "description": "Start line (1-indexed) for line-range replacement. Use with end_line." },
                    "end_line": { "type": "integer", "description": "End line (1-indexed, inclusive) for line-range replacement." },
                    "instruction": { "type": "string", "description": "Optional: describe what this edit does (helps self-correction if match fails)" }
                },
                "required": ["path", "new_str"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut json = serde_json::json!({
            "path": args.path,
            "new_str": args.new_str,
        });
        if let Some(old_str) = &args.old_str {
            json["old_str"] = serde_json::json!(old_str);
        }
        if let Some(sl) = args.start_line {
            json["start_line"] = serde_json::json!(sl);
        }
        if let Some(el) = args.end_line {
            json["end_line"] = serde_json::json!(el);
        }
        let r = tools::replace(&json, self.bridge.workspace_root()).await;
        if r.success {
            invalidate_cache_for(&self.bridge);
            return to_result(r);
        }

        // ── Edit self-correction via LLM (Feature 2) ──
        // Only attempt if we have old_str (not line-range mode)
        if let Some(old_str) = &args.old_str {
            let full_path = self.bridge.workspace_root().join(&args.path);
            if let Ok(file_content) = std::fs::read_to_string(&full_path) {
                // Determine API key and provider from environment
                let api_key = std::env::var("GEMINI_API_KEY")
                    .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
                    .or_else(|_| std::env::var("OPENAI_API_KEY"))
                    .unwrap_or_default();
                let provider = if std::env::var("GEMINI_API_KEY").is_ok() {
                    "gemini"
                } else if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                    "anthropic"
                } else if std::env::var("OPENAI_API_KEY").is_ok() {
                    "openai"
                } else {
                    ""
                };

                if !api_key.is_empty() {
                    tracing::info!("replace_in_file: attempting LLM self-correction for failed edit");
                    if let Some(fix) = crate::edit_fixer::fix_failed_edit(
                        &api_key,
                        provider,
                        &file_content,
                        old_str,
                        &args.new_str,
                        args.instruction.as_deref(),
                        &r.output,
                    )
                    .await
                    {
                        if fix.no_changes_required {
                            return Ok(format!(
                                "No changes needed: {} (file already contains the intended change)",
                                fix.explanation
                            ));
                        }
                        // Try applying the corrected edit
                        let fix_json = serde_json::json!({
                            "path": args.path,
                            "old_str": fix.search,
                            "new_str": fix.replace,
                        });
                        let fix_r = tools::replace(&fix_json, self.bridge.workspace_root()).await;
                        if fix_r.success {
                            invalidate_cache_for(&self.bridge);
                            return Ok(format!(
                                "Updated {} (auto-corrected: {})",
                                args.path, fix.explanation
                            ));
                        }
                        tracing::warn!("edit_fixer: corrected edit also failed: {}", fix_r.output);
                    }
                }
            }
        }

        // Return original error if self-correction failed or wasn't applicable
        to_result(r)
    }
}

// ── 4. DeleteFile ────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct DeleteFileArgs {
    /// File or directory path relative to workspace root
    pub path: String,
}

#[derive(Clone)]
pub struct DeleteFileTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for DeleteFileTool {
    const NAME: &'static str = "delete_file";
    type Error = ToolExecError;
    type Args = DeleteFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "delete_file".to_string(),
            description: "Delete a file or empty directory. Protected paths (.git, node_modules, Cargo.toml) cannot be deleted. \
For large directories, use execute_command with rm -rf instead.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to delete (relative to workspace root)" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let json = serde_json::json!({ "path": args.path });
        let r = tools::delete(&json, self.bridge.workspace_root()).await;
        if r.success { invalidate_cache_for(&self.bridge); }
        to_result(r)
    }
}

// ── 5. ApplyPatch ────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct ApplyPatchArgs {
    /// V4A format patch (*** Begin Patch ... *** End Patch)
    #[serde(default)]
    pub input: Option<String>,
    /// File path (for unified diff format)
    #[serde(default)]
    pub path: Option<String>,
    /// Unified diff patch content (for single file)
    #[serde(default)]
    pub patch: Option<String>,
}

#[derive(Clone)]
pub struct ApplyPatchTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for ApplyPatchTool {
    const NAME: &'static str = "apply_patch";
    type Error = ToolExecError;
    type Args = ApplyPatchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "apply_patch".to_string(),
            description: "Apply a patch to one or more files. \
WHEN TO USE: Multi-file edits or complex changes where replace_in_file would be cumbersome. \
Supports V4A format (multi-file: 'input' with *** Begin Patch / *** Update File: markers) \
and unified diff (single file: 'path' + 'patch' parameters).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "V4A format patch (multi-file)" },
                    "path": { "type": "string", "description": "File path (for unified diff)" },
                    "patch": { "type": "string", "description": "Unified diff content" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut map = serde_json::Map::new();
        if let Some(input) = args.input {
            map.insert("input".into(), serde_json::json!(input));
        }
        if let Some(path) = args.path {
            map.insert("path".into(), serde_json::json!(path));
        }
        if let Some(patch) = args.patch {
            map.insert("patch".into(), serde_json::json!(patch));
        }
        let r = tools::apply_patch(&serde_json::Value::Object(map), self.bridge.workspace_root())
            .await;
        if r.success { invalidate_cache_for(&self.bridge); }
        to_result(r)
    }
}

// ── 6. ReadManyFiles ─────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct ReadManyFilesArgs {
    /// Glob patterns to include (e.g. ["src/**/*.rs", "*.toml"])
    pub include: Vec<String>,
    /// Optional glob patterns to exclude (e.g. ["*_test.rs", "*.generated.*"])
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
}

#[derive(Clone)]
pub struct ReadManyFilesTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for ReadManyFilesTool {
    const NAME: &'static str = "read_many_files";
    type Error = ToolExecError;
    type Args = ReadManyFilesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read_many_files".to_string(),
            description: "Read multiple files at once using glob patterns. Returns concatenated file contents with separators. \
WHEN TO USE: To batch-read all files matching a pattern (e.g., all .rs files in a directory, all config files). \
Much more efficient than making multiple individual read_file calls. \
WHEN NOT TO USE: For a single specific file -- use read_file instead. \
TIPS: Max 30 files, output capped at 50k chars. Files sorted by recency (newest first). \
Respects .gitignore and skips binary files automatically.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "include": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Glob patterns to match files (e.g. ['src/**/*.rs', '*.toml'])"
                    },
                    "exclude": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional: glob patterns to exclude (e.g. ['*_test.rs'])"
                    }
                },
                "required": ["include"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut json = serde_json::json!({ "include": args.include });
        if let Some(exc) = args.exclude {
            json["exclude"] = serde_json::json!(exc);
        }
        to_result(tools::read_many(&json, self.bridge.workspace_root()).await)
    }
}

// ══════════════════════════════════════════════════════════════════
//  SEARCH (4 tools)
// ══════════════════════════════════════════════════════════════════

// ── 6. ListFiles ─────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct ListFilesArgs {
    /// Directory path relative to workspace root (use "." for root)
    pub path: String,
    /// List recursively (max depth 10)
    #[serde(default)]
    pub recursive: Option<bool>,
}

#[derive(Clone)]
pub struct ListFilesTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for ListFilesTool {
    const NAME: &'static str = "list_files";
    type Error = ToolExecError;
    type Args = ListFilesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "list_files".to_string(),
            description: "List files and directories at a given path. Dot-files and common noise directories (node_modules, target, .git) are automatically hidden. \
WHEN TO USE: To see directory structure, find files in a specific folder, or orient yourself in the project. \
TIPS: Use recursive=true sparingly -- prefer list_files on specific subdirectories instead of recursive listing from root.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path relative to workspace root" },
                    "recursive": { "type": "boolean", "description": "List recursively (default: false)" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut json = serde_json::json!({ "path": args.path });
        if let Some(r) = args.recursive {
            json["recursive"] = serde_json::json!(r);
        }
        to_result(tools::list(&json, self.bridge.workspace_root()).await)
    }
}

// ── 7. Grep ──────────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct GrepArgs {
    /// Regex pattern to search for
    pub pattern: String,
    /// Directory or file to search in (relative to workspace root, default ".")
    #[serde(default)]
    pub path: Option<String>,
    /// File filter glob, e.g. "*.rs"
    #[serde(default)]
    pub glob: Option<String>,
    /// Case-insensitive search
    #[serde(default)]
    pub case_insensitive: Option<bool>,
    /// Number of context lines around each match (0-5)
    #[serde(default)]
    pub context: Option<u64>,
}

#[derive(Clone)]
pub struct GrepTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for GrepTool {
    const NAME: &'static str = "grep";
    type Error = ToolExecError;
    type Args = GrepArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "grep".to_string(),
            description: "Fast LITERAL text search using ripgrep. Respects .gitignore automatically. \
WHEN TO USE: When you know the EXACT string or symbol to find -- function names, error messages, import statements, variable names. \
WHEN NOT TO USE: For conceptual/meaning-based queries like 'how does auth work' -- use codebase_search instead. \
TIPS: Use 'glob' parameter to narrow to file types (e.g. '*.rs'). Use 'path' to search specific directories. Results are capped at 50 lines.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for" },
                    "path": { "type": "string", "description": "Directory or file to search (default: '.')" },
                    "glob": { "type": "string", "description": "File filter, e.g. '*.rs', '*.py'" },
                    "case_insensitive": { "type": "boolean", "description": "Ignore case" },
                    "context": { "type": "integer", "description": "Context lines around matches (0-5)" }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut json = serde_json::json!({ "pattern": args.pattern });
        if let Some(p) = args.path {
            json["path"] = serde_json::json!(p);
        }
        if let Some(g) = args.glob {
            json["glob"] = serde_json::json!(g);
        }
        if let Some(ci) = args.case_insensitive {
            json["case_insensitive"] = serde_json::json!(ci);
        }
        if let Some(c) = args.context {
            json["context"] = serde_json::json!(c);
        }
        to_result(tools::grep(&json, self.bridge.workspace_root()).await)
    }
}

// ── 8. Glob ──────────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct GlobArgs {
    /// Glob pattern, e.g. "*.rs", "**/*.test.ts"
    pub pattern: String,
    /// Base directory (relative to workspace root, default ".")
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Clone)]
pub struct GlobTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for GlobTool {
    const NAME: &'static str = "glob";
    type Error = ToolExecError;
    type Args = GlobArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "glob".to_string(),
            description: "Find files by name/extension pattern. Returns file paths only. \
WHEN TO USE: To find files by name pattern ('*.rs', '**/*.test.ts', 'Cargo.toml'). \
WHEN NOT TO USE: To search file contents -- use grep instead. \
TIPS: Use '**/' prefix for recursive search. Dot-files and noise directories are auto-filtered.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern like '*.rs', '**/*.test.ts'" },
                    "path": { "type": "string", "description": "Base directory (default: '.')" }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut json = serde_json::json!({ "pattern": args.pattern });
        if let Some(p) = args.path {
            json["path"] = serde_json::json!(p);
        }
        to_result(tools::glob_search(&json, self.bridge.workspace_root()).await)
    }
}

// ── 9. CodebaseSearch ────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct CodebaseSearchArgs {
    /// Natural language query describing what you're looking for
    pub query: String,
    /// Optional: limit search to a subdirectory
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Clone)]
pub struct CodebaseSearchTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for CodebaseSearchTool {
    const NAME: &'static str = "codebase_search";
    type Error = ToolExecError;
    type Args = CodebaseSearchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "codebase_search".to_string(),
            description: "SEMANTIC search -- find code by meaning, not exact text. Uses embeddings to match conceptually. \
WHEN TO USE: For conceptual queries ('how does authentication work', 'where is payment processed'), \
exploring unfamiliar code areas, or when you don't know the exact symbol name. \
WHEN NOT TO USE: When you know the exact symbol/string -- use grep instead (faster and more precise). \
TIPS: Write queries as natural language questions. Use 'path' to scope to a directory if you have a rough idea where to look.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural language query" },
                    "path": { "type": "string", "description": "Optional: limit search to directory" }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut json = serde_json::json!({ "query": args.query });
        if let Some(p) = args.path {
            json["path"] = serde_json::json!(p);
        }
        // tools::semantic() uses EmbeddingDb (rusqlite Connection) which is !Send.
        // We spawn a dedicated thread with its own single-threaded tokio runtime
        // so this works on both multi_thread and current_thread runtimes.
        let workdir = self.bridge.workspace_root().to_path_buf();
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime for codebase_search");
            let result = rt.block_on(tools::semantic(&json, &workdir));
            let _ = tx.send(result);
        });
        let result = rx.await.unwrap_or_else(|_| {
            tools::ToolResult::err("codebase_search thread panicked")
        });
        to_result(result)
    }
}

// ══════════════════════════════════════════════════════════════════
//  DIAGNOSTICS (1 tool)
// ══════════════════════════════════════════════════════════════════

// ── 10. Diagnostics ──────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct DiagnosticsArgs {
    /// File or directory to check for errors
    pub path: String,
    /// Attempt to auto-fix issues (when supported by the linter)
    #[serde(default)]
    pub fix: Option<bool>,
}

#[derive(Clone)]
pub struct DiagnosticsTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for DiagnosticsTool {
    const NAME: &'static str = "diagnostics";
    type Error = ToolExecError;
    type Args = DiagnosticsArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "diagnostics".to_string(),
            description: "Get compiler/linter errors and warnings for a file or directory. \
WHEN TO USE: After making code changes to verify correctness. Also useful before editing to see existing errors. \
For files: runs the appropriate linter (rustc, tsc, eslint, ruff, go vet). \
For directories: runs project-level checks (cargo check, tsc --noEmit, etc.).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File or directory to check" },
                    "fix": { "type": "boolean", "description": "Attempt auto-fix (when supported)" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut json = serde_json::json!({ "path": args.path });
        if let Some(fix) = args.fix {
            json["fix"] = serde_json::json!(fix);
        }
        to_result(tools::lint::diagnostics(&json, self.bridge.workspace_root()).await)
    }
}

// ══════════════════════════════════════════════════════════════════
//  WEB & DOCUMENTATION (3 tools)
// ══════════════════════════════════════════════════════════════════

// ── 11. WebSearch ────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct WebSearchArgs {
    /// Search query
    pub query: String,
}

#[derive(Clone)]
pub struct WebSearchTool;

impl Tool for WebSearchTool {
    const NAME: &'static str = "web_search";
    type Error = ToolExecError;
    type Args = WebSearchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web for information. Returns titles, URLs, and snippets. \
WHEN TO USE: For current events, non-code information, general programming questions not specific to a library. \
WHEN NOT TO USE: For library/framework API docs -- use fetch_documentation instead (more accurate).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let json = serde_json::json!({ "query": args.query });
        // Use qualified path to avoid collision with private `search` module.
        to_result(tools::web::search(&json).await)
    }
}

// ── 12. WebFetch ─────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct WebFetchArgs {
    /// URL to fetch
    pub url: String,
}

#[derive(Clone)]
pub struct WebFetchTool;

impl Tool for WebFetchTool {
    const NAME: &'static str = "web_fetch";
    type Error = ToolExecError;
    type Args = WebFetchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "web_fetch".to_string(),
            description: "Fetch and read content from a URL. Extracts readable text from HTML pages, returns JSON/text as-is. \
WHEN TO USE: To read a specific web page, documentation page, or API endpoint. \
WHEN NOT TO USE: For general web search -- use web_search instead.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let json = serde_json::json!({ "url": args.url });
        to_result(tools::web::fetch(&json).await)
    }
}

// ── 13. FetchDocumentation ───────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct FetchDocsArgs {
    /// Library or framework name (e.g. "react", "tokio", "fastapi")
    pub library: String,
    /// Optional topic to focus on (e.g. "hooks", "async", "middleware")
    #[serde(default)]
    pub topic: Option<String>,
}

#[derive(Clone)]
pub struct FetchDocsTool;

impl Tool for FetchDocsTool {
    const NAME: &'static str = "fetch_documentation";
    type Error = ToolExecError;
    type Args = FetchDocsArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "fetch_documentation".to_string(),
            description: "Fetch official library/framework documentation. PREFERRED over web_search for programming libraries. \
WHEN TO USE: When you need API details, usage patterns, or aren't familiar with a library's API. \
Examples: fetch_documentation('tokio', 'spawn'), fetch_documentation('react', 'hooks'). \
Returns curated, accurate documentation -- much better than raw web search for coding questions.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "library": { "type": "string", "description": "Library name (e.g. 'react', 'tokio')" },
                    "topic": { "type": "string", "description": "Optional: specific topic" }
                },
                "required": ["library"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut json = serde_json::json!({ "library": args.library });
        if let Some(t) = args.topic {
            json["topic"] = serde_json::json!(t);
        }
        to_result(tools::web::fetch_docs(&json).await)
    }
}

// ══════════════════════════════════════════════════════════════════
//  SHELL (1 tool)
// ══════════════════════════════════════════════════════════════════

// ── 14. ExecuteCommand ───────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct ExecuteCommandArgs {
    /// Shell command to execute
    pub command: String,
}

#[derive(Clone)]
pub struct ExecuteCommandTool {
    pub bridge: Arc<dyn ProxyBridge>,
}

impl Tool for ExecuteCommandTool {
    const NAME: &'static str = "execute_command";
    type Error = ToolExecError;
    type Args = ExecuteCommandArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "execute_command".to_string(),
            description: "Execute a shell command in the workspace directory. \
WHEN TO USE: For builds (cargo build), tests (cargo test), git commands, package managers (npm install), or any CLI tool. \
TIPS: Commands run with a 30-second timeout. For long-running commands, consider running them in background. \
IMPORTANT: Be careful with destructive commands. Never run rm -rf on project root or critical directories.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let json = serde_json::json!({ "command": args.command });
        to_result_shell(tools::run(&json, self.bridge.workspace_root()).await)
    }
}

// ══════════════════════════════════════════════════════════════════
//  AGENT CONTROL (1 tool)
// ══════════════════════════════════════════════════════════════════

// ── 15. Think ────────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct ThinkArgs {
    /// Your reasoning or analysis of the current situation
    pub thought: String,
}

#[derive(Clone)]
pub struct ThinkTool;

impl Tool for ThinkTool {
    const NAME: &'static str = "think";
    type Error = ToolExecError;
    type Args = ThinkArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "think".to_string(),
            description: "Think through complex problems step by step before acting. \
WHEN TO USE: Before making complex changes, when planning a multi-step approach, \
or when you need to reason about trade-offs. \
WHEN NOT TO USE: For simple, obvious tasks -- just act directly.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "thought": { "type": "string", "description": "Your reasoning or analysis" }
                },
                "required": ["thought"]
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        // The think tool simply acknowledges the reasoning.  The LLM uses
        // it to organise its thoughts before taking action.
        Ok("Thought recorded.".to_string())
    }
}

// ══════════════════════════════════════════════════════════════════
//  SAVE MEMORY TOOL
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize, JsonSchema)]
pub struct SaveMemoryArgs {
    /// The fact or preference to remember persistently across sessions.
    /// Examples: "User prefers tabs over spaces", "Always use pytest for testing",
    /// "Project uses PostgreSQL 15".
    pub fact: String,
}

pub struct SaveMemoryTool;

impl Tool for SaveMemoryTool {
    const NAME: &'static str = "save_memory";
    type Error = ToolExecError;
    type Args = SaveMemoryArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "save_memory".to_string(),
            description: "Saves a specific piece of information to your long-term memory (persisted in ~/.config/forge-ide/FORGE.md). \
The saved fact will be loaded automatically in all future sessions. \
WHEN TO USE: When the user explicitly asks you to remember something, states a clear preference, \
or corrects your behavior and you should remember the correction. \
Examples: 'remember I prefer tabs', 'always use npm not yarn in this project'. \
WHEN NOT TO USE: For temporary session context, file contents, or information already in FORGE.md.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "fact": {
                        "type": "string",
                        "description": "The fact or preference to remember. Should be a clear, concise statement."
                    }
                },
                "required": ["fact"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        crate::project_memory::save_memory(&args.fact)
            .map_err(|e| ToolExecError(e))
    }
}

// ══════════════════════════════════════════════════════════════════
//  BUILD TOOLSET
// ══════════════════════════════════════════════════════════════════

/// Create a rig `ToolSet` with all 17 available tools.
pub fn build_toolset(bridge: Arc<dyn ProxyBridge>) -> rig::tool::ToolSet {
    let mut ts = rig::tool::ToolSet::default();

    // File operations (6)
    ts.add_tool(ReadFileTool { bridge: bridge.clone() });
    ts.add_tool(WriteFileTool { bridge: bridge.clone() });
    ts.add_tool(ReplaceInFileTool { bridge: bridge.clone() });
    ts.add_tool(DeleteFileTool { bridge: bridge.clone() });
    ts.add_tool(ApplyPatchTool { bridge: bridge.clone() });
    ts.add_tool(ReadManyFilesTool { bridge: bridge.clone() });

    // Search (4)
    ts.add_tool(ListFilesTool { bridge: bridge.clone() });
    ts.add_tool(GrepTool { bridge: bridge.clone() });
    ts.add_tool(GlobTool { bridge: bridge.clone() });
    ts.add_tool(CodebaseSearchTool { bridge: bridge.clone() });

    // Diagnostics (1)
    ts.add_tool(DiagnosticsTool { bridge: bridge.clone() });

    // Web & documentation (3)
    ts.add_tool(WebSearchTool);
    ts.add_tool(WebFetchTool);
    ts.add_tool(FetchDocsTool);

    // Shell (1)
    ts.add_tool(ExecuteCommandTool { bridge: bridge.clone() });

    // Agent control (2)
    ts.add_tool(ThinkTool);
    ts.add_tool(SaveMemoryTool);

    ts
}
