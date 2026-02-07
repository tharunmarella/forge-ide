//! ForgeAgent -- the main AI coding agent built on rig-core.
//!
//! This wraps rig's Agent with our IDE-specific tools and system prompt,
//! providing a clean API for the IDE's chat panel.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use walkdir::WalkDir;

use crate::bridge::ProxyBridge;
use crate::rig_tools;
use crate::tools::should_skip_dir;

/// System prompt modeled after Cursor's architecture:
/// - XML-tagged sections for clear structure
/// - Explicit tool selection guidance
/// - Pre-fetched context awareness (RepoMap + pre-search)
/// - Budget-aware turn management
/// - Think-first discipline for edits
const SYSTEM_PROMPT: &str = r#"You are Forge, an expert AI coding assistant embedded in the user's IDE. You help with software engineering tasks including writing, editing, debugging, and explaining code.

Each time the user sends a message, contextual information is automatically attached:
- <project_memory>: persistent instructions from FORGE.md files (user preferences, project conventions)
- <project_layout>: file tree of the workspace
- <repo_map>: PageRank-ranked symbol map showing the MOST IMPORTANT functions, classes, and types across the codebase with their locations
- <relevant_context>: pre-searched code snippets matching keywords from the user's query
- <git_info>: current git status
- <user_info>: OS, workspace path, primary language

CRITICAL: Read ALL these context blocks BEFORE making any tool calls. The <project_memory> contains persistent instructions that override defaults. The <repo_map> tells you WHERE every important symbol lives. The <relevant_context> shows you code snippets that likely answer the query. Often you can act directly on this information without any search tools.

<project_memory_rules>
<project_memory> contains persistent instructions loaded from FORGE.md files:
- Global (~/.config/forge-ide/FORGE.md): foundational user preferences. Apply these broadly.
- Workspace Root (./FORGE.md): project-wide mandates. Supersedes global preferences.
- Sub-directories (./subdir/FORGE.md): highly specific overrides. Supersede all others for files in their scope.
These instructions override default operational behaviors (e.g., tech stack, style, workflows, tool preferences).
They CANNOT override safety rules or core mandates.
When the user asks you to remember something, use the save_memory tool to persist it.
</project_memory_rules>

<communication>
- Be concise. Explain what you're doing and why in 1-2 sentences, then act.
- Use backticks for file paths, function names, and code symbols.
- Do NOT fabricate file contents or paths. Always verify with tools first.
- After completing a code modification, do NOT provide summaries or explanations unless the user asks. State what you changed in 1-2 sentences and stop.
</communication>

<workflow>
For complex tasks, follow the Research -> Strategy -> Execution lifecycle:

1. RESEARCH: Use grep/glob/read_file/read_many_files to map the relevant parts of the codebase. Check <repo_map> and <relevant_context> first -- they often eliminate the need for research.
2. STRATEGY: Formulate a clear plan. For multi-step changes, use the think tool to organize your approach before acting.
3. EXECUTION: Implement with Plan -> Act -> Validate cycles:
   - Plan: Identify the exact file and location to change.
   - Act: Make the change with replace_in_file or write_to_file.
   - Validate: Run diagnostics/build. If it passes, move to the next change or respond.

For simple tasks (single-file edits, answering questions), skip directly to execution.
</workflow>

<tool_calling>
You have tools to explore, read, edit, and run code. Follow these rules:
1. NEVER guess file contents. Always read_file before editing.
2. Execute ALL independent tool calls in parallel. For example: reading 3 files = 3 parallel read_file calls. Searching for 2 different symbols = 2 parallel grep calls. NEVER serialize independent operations.
3. After edits, run diagnostics or the project's build command to verify correctness.
4. Use replace_in_file for surgical edits (preferred), write_to_file only for NEW files, apply_patch for multi-file changes.
5. Use read_many_files to batch-read multiple files by glob pattern instead of making individual read_file calls.
6. For shell commands: always prefer quiet flags (--silent, --quiet, -q). Always disable terminal pagination (git --no-pager, PAGER=cat). Keep output concise.
7. If replace_in_file fails, the system will automatically attempt to fix the match using flexible and regex strategies. Include an instruction field describing what the edit is trying to do for better self-correction context.
</tool_calling>

<search_strategy>
This is CRITICAL for finding the right code quickly:
1. FIRST: Check <repo_map> and <relevant_context>. These already contain the most important symbols and pre-searched matches for your query. Do NOT search for things that are already shown there.
2. Use grep ONLY when you know an EXACT symbol or string not already visible in the pre-fetched context.
3. Use codebase_search for CONCEPTUAL queries ("how does auth work") when the repo_map doesn't answer it.
4. Use glob to find files by name pattern ("*.rs", "**/*.test.ts").
5. Use read_many_files to batch-read all files matching a glob pattern (e.g., all TypeScript files in a directory).
6. Do NOT search the same thing twice with different tools.
7. STOP searching once you have enough information. Do not explore "just in case".
8. For most tasks, you should need at most 1-2 search tool calls because the pre-fetched context already points you to the right files.
</search_strategy>

<efficiency>
You have a LIMITED turn budget. Every tool call costs a turn. Be decisive:
- For READ-ONLY questions: The answer is usually in <repo_map> + <relevant_context>. Read 1-2 files at most, then respond.
- For EDIT tasks: Read target file -> edit -> verify. That's it. 3-5 turns total.
- NEVER explore aimlessly. If you've used 5+ turns without producing a result, you are wasting turns.
- When you have enough information, ACT immediately. Do not keep searching.
- After diagnostics/verify succeeds, RESPOND immediately with what you did. Do NOT read more files or run more searches after a successful verification.
- Do NOT summarize changes after completing them unless asked. A brief "Done. Updated X to Y." is sufficient.
</efficiency>

<scope_discipline>
CRITICAL: Make the SMALLEST change that satisfies the request.
- If the user says "add X", add ONLY X. Do not also refactor, add CLI commands, modify imports for unrelated things, or add features they didn't ask for.
- ONE change per file unless the user explicitly asks for more.
- If the request is ambiguous (e.g. "add a way to list commands" -- as a method? a CLI subcommand? a utility function?), make the simplest interpretation (a method) unless the user specifies otherwise.
- Do NOT write tests unless the user asks for tests.
- Do NOT modify files beyond what is strictly necessary for the requested change.
</scope_discipline>

<making_code_changes>
FOR ANY EDIT TASK, follow this exact sequence:
1. READ: Read the target file identified from <repo_map>.
2. EDIT: Make the change using replace_in_file (preferred) or write_to_file (new files only).
3. VERIFY: Run diagnostics. If it passes, STOP and respond.

replace_in_file supports 3 automatic matching strategies (exact -> flexible whitespace-tolerant -> regex).
If it still fails and old_str matches multiple times, the error will show line numbers.
Use those line numbers with start_line + end_line parameters instead of old_str to target the exact location.

Rules:
- Make minimal, focused changes. Match existing code style exactly.
- NEVER generate extremely long hashes, binary data, or non-textual content.
- Preserve the exact indentation (tabs/spaces) of the existing code.
- After verification succeeds, respond with what you changed. Do NOT continue exploring.
</making_code_changes>"#;

/// Configuration for creating a ForgeAgent.
#[derive(Clone, Debug)]
pub struct ForgeAgentConfig {
    /// LLM provider id (e.g. "anthropic", "openai", "gemini")
    pub provider: String,
    /// Model id (e.g. "claude-sonnet-4-20250514", "gpt-4o")
    pub model: String,
    /// API key for the provider
    pub api_key: String,
    /// Optional base URL override
    pub base_url: Option<String>,
    /// Temperature (0.0 - 1.0)
    pub temperature: Option<f64>,
    /// Max tokens for completion
    pub max_tokens: Option<u64>,
    /// Max agent turns (tool call rounds)
    pub max_turns: usize,
}

impl Default for ForgeAgentConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            api_key: String::new(),
            base_url: None,
            temperature: Some(0.0),
            max_tokens: Some(8192),
            max_turns: 25,
        }
    }
}

/// Register all 16 tools on an agent builder.
macro_rules! register_core_tools {
    ($builder:expr, $bridge:expr) => {{
        $builder
            // File operations (6)
            .tool(rig_tools::ReadFileTool { bridge: $bridge.clone() })
            .tool(rig_tools::WriteFileTool { bridge: $bridge.clone() })
            .tool(rig_tools::ReplaceInFileTool { bridge: $bridge.clone() })
            .tool(rig_tools::DeleteFileTool { bridge: $bridge.clone() })
            .tool(rig_tools::ApplyPatchTool { bridge: $bridge.clone() })
            .tool(rig_tools::ReadManyFilesTool { bridge: $bridge.clone() })
            // Search (4)
            .tool(rig_tools::ListFilesTool { bridge: $bridge.clone() })
            .tool(rig_tools::GrepTool { bridge: $bridge.clone() })
            .tool(rig_tools::GlobTool { bridge: $bridge.clone() })
            .tool(rig_tools::CodebaseSearchTool { bridge: $bridge.clone() })
            // Diagnostics (1)
            .tool(rig_tools::DiagnosticsTool { bridge: $bridge.clone() })
            // Web & documentation (3)
            .tool(rig_tools::WebSearchTool)
            .tool(rig_tools::WebFetchTool)
            .tool(rig_tools::FetchDocsTool)
            // Shell (1)
            .tool(rig_tools::ExecuteCommandTool { bridge: $bridge.clone() })
            // Agent control (2)
            .tool(rig_tools::ThinkTool)
            .tool(rig_tools::SaveMemoryTool)
    }};
}

/// Build a rig agent for Anthropic (Claude).
pub fn create_agent_anthropic(
    config: &ForgeAgentConfig,
    bridge: Arc<dyn ProxyBridge>,
) -> Result<rig::agent::Agent<rig::providers::anthropic::completion::CompletionModel>> {
    use rig::client::CompletionClient;

    let client = rig::providers::anthropic::Client::new(&config.api_key)
        .map_err(|e| anyhow::anyhow!("Failed to create Anthropic client: {e}"))?;

    let mut builder = register_core_tools!(
        client.agent(&config.model).preamble(SYSTEM_PROMPT),
        bridge
    )
    .default_max_turns(config.max_turns);

    if let Some(temp) = config.temperature {
        builder = builder.temperature(temp);
    }
    if let Some(max_tok) = config.max_tokens {
        builder = builder.max_tokens(max_tok);
    }

    Ok(builder.build())
}

/// Build a rig agent for OpenAI (uses the Responses API by default).
pub fn create_agent_openai(
    config: &ForgeAgentConfig,
    bridge: Arc<dyn ProxyBridge>,
) -> Result<rig::agent::Agent<rig::providers::openai::responses_api::ResponsesCompletionModel>> {
    use rig::client::CompletionClient;

    let client = rig::providers::openai::Client::new(&config.api_key)
        .map_err(|e| anyhow::anyhow!("Failed to create OpenAI client: {e}"))?;

    let mut builder = register_core_tools!(
        client.agent(&config.model).preamble(SYSTEM_PROMPT),
        bridge
    )
    .default_max_turns(config.max_turns);

    if let Some(temp) = config.temperature {
        builder = builder.temperature(temp);
    }
    if let Some(max_tok) = config.max_tokens {
        builder = builder.max_tokens(max_tok);
    }

    Ok(builder.build())
}

/// Build a rig agent for Google Gemini.
pub fn create_agent_gemini(
    config: &ForgeAgentConfig,
    bridge: Arc<dyn ProxyBridge>,
) -> Result<rig::agent::Agent<rig::providers::gemini::completion::CompletionModel>> {
    use rig::client::CompletionClient;

    let client = rig::providers::gemini::Client::new(&config.api_key)
        .map_err(|e| anyhow::anyhow!("Failed to create Gemini client: {e}"))?;

    let mut builder = register_core_tools!(
        client.agent(&config.model).preamble(SYSTEM_PROMPT),
        bridge
    )
    .default_max_turns(config.max_turns);

    if let Some(temp) = config.temperature {
        builder = builder.temperature(temp);
    }
    if let Some(max_tok) = config.max_tokens {
        builder = builder.max_tokens(max_tok);
    }

    Ok(builder.build())
}

// ══════════════════════════════════════════════════════════════════
//  PROMPT ENRICHMENT  (used by both IDE dispatch and CLI harness)
// ══════════════════════════════════════════════════════════════════

/// Build an enriched user prompt by prepending project context.
///
/// This is the key to eliminating exploration turns. Before the LLM
/// sees anything, we inject:
/// 0. `<project_memory>` -- persistent FORGE.md instructions (global + workspace)
/// 1. `<user_info>` -- OS, workspace, language
/// 2. `<project_layout>` -- file tree (3 levels deep)
/// 3. `<repo_map>` -- PageRank-ranked symbol map (functions, classes, types)
/// 4. `<relevant_context>` -- pre-searched grep results for query keywords
/// 5. `<git_info>` -- current git status
/// 6. `<user_query>` -- the actual user prompt
///
/// Items 3 & 4 are cached with [moka](https://github.com/moka-rs/moka)
/// so repeat/similar queries within the TTL window are instant.
pub fn build_enriched_prompt(user_prompt: &str, workspace_path: &str) -> String {
    let workspace = Path::new(workspace_path);
    let cache = crate::context_cache::global();

    // ── 0. Project memory (FORGE.md files) ──
    let global_memory = crate::project_memory::load_global();
    let workspace_memory = crate::project_memory::load_workspace(workspace);

    // ── 1. User info ──
    let os_info = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let today = chrono::Local::now().format("%A %b %d, %Y").to_string();

    // ── 2. Compact project file tree (3 levels for better orientation) ──
    let project_tree = build_project_tree(workspace, 3);

    // ── 3. RepoMap -- PageRank-ranked symbol map (CACHED) ──
    let repo_map = cache.get_repo_map(workspace);

    // ── 4. Pre-search -- grep for query keywords (CACHED) ──
    let pre_search = cache.get_pre_search(workspace, user_prompt);

    // ── 5. Git status (short) ──
    let git_status = std::process::Command::new("git")
        .args(["status", "--short", "--branch"])
        .current_dir(workspace)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    // ── 6. Detect primary language ──
    let primary_lang = detect_primary_language(workspace);

    // ── Assemble ──
    let mut enriched = String::with_capacity(user_prompt.len() + 12288);

    // Project memory goes first (highest influence on behavior)
    let memory_rendered = crate::project_memory::render_memory(
        &global_memory,
        &workspace_memory,
        &[], // JIT subdirectory memory is loaded dynamically during tool calls
        workspace,
    );
    if !memory_rendered.is_empty() {
        enriched.push_str("<project_memory>\n");
        enriched.push_str(&memory_rendered);
        enriched.push_str("</project_memory>\n\n");
    }

    enriched.push_str("<user_info>\n");
    enriched.push_str(&format!("OS: {} {}\n", os_info, arch));
    enriched.push_str(&format!("Workspace: {}\n", workspace_path));
    enriched.push_str(&format!("Today: {}\n", today));
    if !primary_lang.is_empty() {
        enriched.push_str(&format!("Primary language: {}\n", primary_lang));
    }
    enriched.push_str("</user_info>\n\n");

    if !project_tree.is_empty() {
        enriched.push_str("<project_layout>\n");
        enriched.push_str(&project_tree);
        enriched.push_str("\n</project_layout>\n\n");
    }

    if !repo_map.is_empty() {
        enriched.push_str("<repo_map>\n");
        enriched.push_str("Symbol map ranked by importance (PageRank). Use this to find where code lives:\n");
        enriched.push_str(&repo_map);
        enriched.push_str("</repo_map>\n\n");
    }

    if !pre_search.is_empty() {
        enriched.push_str("<relevant_context>\n");
        enriched.push_str("Pre-searched code matching keywords from your query:\n");
        enriched.push_str(&pre_search);
        enriched.push_str("\n</relevant_context>\n\n");
    }

    if !git_status.is_empty() {
        enriched.push_str("<git_info>\n");
        enriched.push_str(&git_status);
        enriched.push_str("</git_info>\n\n");
    }

    enriched.push_str("<user_query>\n");
    enriched.push_str(user_prompt);
    enriched.push_str("\n</user_query>");

    enriched
}

/// Build a compact file tree representation (dirs and files) up to max_depth.
pub fn build_project_tree(workspace: &Path, max_depth: usize) -> String {
    let mut lines = Vec::new();

    for entry in WalkDir::new(workspace)
        .max_depth(max_depth)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            if e.depth() == 0 { return true; }
            if e.file_type().is_dir() {
                return !should_skip_dir(&name);
            }
            !name.starts_with('.')
        })
        .filter_map(|e| e.ok())
    {
        if entry.depth() == 0 { continue; }
        if let Ok(rel) = entry.path().strip_prefix(workspace) {
            let indent = "  ".repeat(entry.depth() - 1);
            let name = rel.file_name().unwrap_or_default().to_string_lossy();
            if entry.file_type().is_dir() {
                lines.push(format!("{}{}/", indent, name));
            } else {
                lines.push(format!("{}{}", indent, name));
            }
        }
    }

    // Cap at 80 lines to keep the prompt manageable
    if lines.len() > 80 {
        lines.truncate(80);
        lines.push("  ... (truncated)".to_string());
    }

    lines.join("\n")
}

/// Detect the primary language based on config files present.
pub fn detect_primary_language(workspace: &Path) -> &'static str {
    if workspace.join("Cargo.toml").exists() { return "Rust"; }
    if workspace.join("package.json").exists() { return "JavaScript/TypeScript"; }
    if workspace.join("pyproject.toml").exists() || workspace.join("setup.py").exists() { return "Python"; }
    if workspace.join("go.mod").exists() { return "Go"; }
    if workspace.join("pom.xml").exists() || workspace.join("build.gradle").exists() { return "Java"; }
    if workspace.join("Gemfile").exists() { return "Ruby"; }
    if workspace.join("mix.exs").exists() { return "Elixir"; }
    ""
}
