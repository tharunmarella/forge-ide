//! forge-cli -- standalone CLI harness for testing the Forge AI agent.
//!
//! Usage:
//!   cargo run --release --bin forge-cli -- \
//!     --provider gemini --model gemini-3-flash-preview \
//!     --api-key "$GEMINI_API_KEY" \
//!     --workspace /path/to/project \
//!     "what vision model are we using?"

use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use futures_util::StreamExt;

use forge_agent::rig::agent::MultiTurnStreamItem;
use forge_agent::rig::completion::CompletionModel;
use forge_agent::rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingPrompt};
use forge_agent::{
    ForgeAgentConfig, StandaloneBridge, TracingHook,
    build_enriched_prompt,
};

// ── ANSI colors ──────────────────────────────────────────────────
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[derive(Parser)]
#[command(name = "forge-cli", about = "Test the Forge AI agent from the command line")]
struct Cli {
    /// LLM provider: gemini, anthropic, openai
    #[arg(long, default_value = "gemini")]
    provider: String,

    /// Model identifier
    #[arg(long, default_value = "gemini-2.5-flash")]
    model: String,

    /// API key (falls back to GEMINI_API_KEY / ANTHROPIC_API_KEY / OPENAI_API_KEY env vars)
    #[arg(long)]
    api_key: Option<String>,

    /// Workspace directory the agent will operate on
    #[arg(long, default_value = ".")]
    workspace: String,

    /// Maximum agent turns (tool call rounds)
    #[arg(long, default_value = "25")]
    max_turns: usize,

    /// Initialize a FORGE.md by having the agent explore the project
    #[arg(long)]
    init: bool,

    /// The prompt to send to the agent (not required when --init is used)
    prompt: Option<String>,
}

fn resolve_api_key(cli_key: Option<&str>, provider: &str) -> String {
    if let Some(k) = cli_key {
        return k.to_string();
    }
    let env_var = match provider {
        "gemini" => "GEMINI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        _ => "LLM_API_KEY",
    };
    std::env::var(env_var).unwrap_or_else(|_| {
        eprintln!("{RED}Error:{RESET} No API key. Pass --api-key or set {env_var}");
        std::process::exit(1);
    })
}

#[tokio::main]
async fn main() {
    // Initialize tracing (respect RUST_LOG, default to warn)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // Resolve workspace to absolute path (needed before --init check)
    let workspace_path = std::fs::canonicalize(&cli.workspace)
        .unwrap_or_else(|e| {
            eprintln!("{RED}Error:{RESET} Bad workspace path '{}': {e}", cli.workspace);
            std::process::exit(1);
        });
    let workspace_display = workspace_path.display().to_string();

    // ── Handle --init mode (check for existing FORGE.md before API key) ──
    let user_prompt = if cli.init {
        // Check if FORGE.md already exists
        let forge_md_path = workspace_path.join("FORGE.md");
        if forge_md_path.exists() {
            eprintln!(
                "{YELLOW}[init]{RESET} FORGE.md already exists at {}",
                forge_md_path.display(),
            );
            eprintln!(
                "{YELLOW}[init]{RESET} Delete it first if you want to regenerate. Exiting.",
            );
            std::process::exit(0);
        }
        eprintln!(
            "{CYAN}{BOLD}[init]{RESET} Generating FORGE.md by exploring the project...",
        );
        forge_agent::project_memory::INIT_PROMPT.to_string()
    } else {
        match cli.prompt {
            Some(p) => p,
            None => {
                eprintln!("{RED}Error:{RESET} No prompt provided. Use --init or pass a prompt.");
                std::process::exit(1);
            }
        }
    };

    let api_key = resolve_api_key(cli.api_key.as_deref(), &cli.provider);

    // ── Print context summary ──
    let lang = forge_agent::detect_primary_language(&workspace_path);
    let tree = forge_agent::build_project_tree(&workspace_path, 3);
    let tree_lines = tree.lines().count();
    eprintln!(
        "{CYAN}[context]{RESET} OS: {} {} | Workspace: {} | Lang: {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        workspace_display,
        if lang.is_empty() { "unknown" } else { lang },
    );
    eprintln!(
        "{CYAN}[context]{RESET} File tree: {} entries | Provider: {} | Model: {}",
        tree_lines, cli.provider, cli.model,
    );

    // ── Build enriched prompt (with RepoMap + pre-search, moka-cached) ──
    let ctx_start = Instant::now();
    let enriched_prompt = build_enriched_prompt(&user_prompt, &workspace_display);
    let ctx_ms = ctx_start.elapsed().as_millis();

    // Show cache stats
    let cache = forge_agent::context_cache::global();
    let repo_map = cache.get_repo_map(&workspace_path);
    let repo_map_symbols = repo_map.matches('│').count();
    let pre_search = cache.get_pre_search(&workspace_path, &user_prompt);
    let pre_search_lines = if pre_search.is_empty() { 0 } else { pre_search.lines().count() };
    eprintln!(
        "{CYAN}[context]{RESET} RepoMap: {} symbols | Pre-search: {} lines | Context built in {}ms",
        repo_map_symbols, pre_search_lines, ctx_ms,
    );
    eprintln!(
        "{CYAN}[context]{RESET} Enriched prompt: {} chars",
        enriched_prompt.len(),
    );
    eprintln!();

    // ── Create agent ──
    let config = ForgeAgentConfig {
        provider: cli.provider.clone(),
        model: cli.model.clone(),
        api_key,
        max_turns: cli.max_turns,
        ..Default::default()
    };

    let bridge: Arc<dyn forge_agent::bridge::ProxyBridge> =
        Arc::new(StandaloneBridge::new(workspace_path.clone()));

    // ── Create trace file ──
    let trace_dir = directories::BaseDirs::new()
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("forge-ide")
        .join("traces");
    let trace_filename = format!(
        "cli-{}.jsonl",
        chrono::Utc::now().format("%Y%m%d-%H%M%S"),
    );
    let hook = TracingHook::new(trace_dir.join(&trace_filename))
        .unwrap_or_else(|e| {
            eprintln!("{YELLOW}Warning:{RESET} Failed to create trace file: {e}");
            TracingHook::new(
                std::path::PathBuf::from("/tmp")
                    .join("forge-traces")
                    .join(&trace_filename),
            )
            .expect("fallback trace must work")
        });
    eprintln!(
        "{DIM}[trace] {}{RESET}",
        hook.trace_path.display(),
    );
    eprintln!();

    hook.write_event(
        "session_start",
        serde_json::json!({
            "provider": &cli.provider,
            "model": &cli.model,
            "prompt_len": enriched_prompt.len(),
            "workspace": workspace_display,
        }),
    );

    // ── Run agent ──
    let session_start = Instant::now();
    let result = match cli.provider.as_str() {
        "gemini" => {
            match forge_agent::create_agent_gemini(&config, bridge) {
                Ok(agent) => run_cli_agent(agent, &enriched_prompt, hook.clone()).await,
                Err(e) => Err(format!("Failed to create Gemini agent: {e}")),
            }
        }
        "anthropic" => {
            match forge_agent::create_agent_anthropic(&config, bridge) {
                Ok(agent) => run_cli_agent(agent, &enriched_prompt, hook.clone()).await,
                Err(e) => Err(format!("Failed to create Anthropic agent: {e}")),
            }
        }
        "openai" => {
            match forge_agent::create_agent_openai(&config, bridge) {
                Ok(agent) => run_cli_agent(agent, &enriched_prompt, hook.clone()).await,
                Err(e) => Err(format!("Failed to create OpenAI agent: {e}")),
            }
        }
        other => Err(format!("Unknown provider: {other}")),
    };

    let total_s = session_start.elapsed().as_secs_f64();

    match result {
        Ok((response, turns, tool_calls)) => {
            eprintln!();
            eprintln!(
                "{GREEN}{BOLD}[done]{RESET} {turns} turns, {tool_calls} tool calls, {total_s:.1}s total",
            );
            eprintln!();
            // Print the actual response to stdout (not stderr) so it can be piped
            println!("{response}");
        }
        Err(e) => {
            eprintln!();
            eprintln!("{RED}{BOLD}[error]{RESET} {e}");
            eprintln!(
                "{DIM}[done] {total_s:.1}s total{RESET}",
            );
            std::process::exit(1);
        }
    }
}

/// Run the streaming agent loop, printing tool calls and text to stderr.
/// Returns (response_text, turn_count, tool_call_count).
///
/// Includes loop detection: if the agent repeats the same tool call 4+ times
/// or produces repetitive text, the loop is broken with an error message.
async fn run_cli_agent<M>(
    agent: forge_agent::rig::agent::Agent<M>,
    prompt: &str,
    hook: TracingHook,
) -> Result<(String, usize, usize), String>
where
    M: CompletionModel + 'static,
{
    let mut stream = agent
        .stream_prompt(prompt)
        .with_hook(hook.clone())
        .await;

    let mut full_response = String::new();
    let mut turn = 0usize;
    let mut tool_call_count = 0usize;
    let mut last_tool_start = Instant::now();
    let mut loop_detector = forge_agent::loop_detection::LoopDetector::new();

    while let Some(item) = stream.next().await {
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::Text(text),
            )) => {
                if full_response.is_empty() && !text.text.is_empty() {
                    eprintln!("{GREEN}[turn {turn}] text:{RESET}");
                }
                full_response.push_str(&text.text);
                // Print text chunks in real time to stderr
                eprint!("{}", text.text);

                // Check for content chanting loop
                let loop_check = loop_detector.check_content(&text.text);
                if loop_check.is_loop() {
                    let msg = loop_check.message();
                    eprintln!("\n{RED}{BOLD}[loop detected]{RESET} {}", msg);
                    hook.write_event("loop_detected", serde_json::json!({ "reason": msg }));
                    hook.write_session_end();
                    return Err(msg);
                }
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ToolCall { tool_call, .. },
            )) => {
                turn += 1;
                tool_call_count += 1;
                last_tool_start = Instant::now();
                let args = serde_json::to_string(&tool_call.function.arguments)
                    .unwrap_or_default();

                // Check for tool call loop BEFORE displaying
                let loop_check = loop_detector.check_tool_call(
                    &tool_call.function.name,
                    &args,
                );
                if loop_check.is_loop() {
                    let msg = loop_check.message();
                    eprintln!("{RED}{BOLD}[loop detected]{RESET} {}", msg);
                    hook.write_event("loop_detected", serde_json::json!({ "reason": msg }));
                    hook.write_session_end();
                    return Err(msg);
                }

                // Truncate long args for display
                let args_display = if args.len() > 120 {
                    format!("{}...", &args[..120])
                } else {
                    args
                };
                eprint!(
                    "{YELLOW}[turn {turn}]{RESET} tool_call: {BOLD}{}{RESET} {DIM}{}{RESET}",
                    tool_call.function.name, args_display,
                );
            }
            Ok(MultiTurnStreamItem::StreamUserItem(
                StreamedUserContent::ToolResult { tool_result, .. },
            )) => {
                let elapsed = last_tool_start.elapsed().as_secs_f64();
                let output = tool_result
                    .content
                    .iter()
                    .filter_map(|c| {
                        if let forge_agent::rig::message::ToolResultContent::Text(t) = c {
                            Some(t.text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let preview = if output.len() > 80 {
                    format!("{}...", &output[..80].replace('\n', " "))
                } else {
                    output.replace('\n', " ")
                };
                eprintln!(
                    " {DIM}=> {} chars ({:.2}s) {}{RESET}",
                    output.len(),
                    elapsed,
                    preview,
                );
            }
            Ok(MultiTurnStreamItem::FinalResponse(final_resp)) => {
                if full_response.is_empty() {
                    full_response = final_resp.response().to_string();
                }
                hook.write_session_end();
                break;
            }
            Err(e) => {
                hook.write_event("error", serde_json::json!({ "error": e.to_string() }));
                hook.write_session_end();
                return Err(e.to_string());
            }
            _ => {
                // ToolCallDelta, ReasoningDelta, etc. -- ignore
            }
        }
    }

    // Ensure trailing newline after streamed text
    if !full_response.is_empty() {
        eprintln!();
    }

    Ok((full_response, turn, tool_call_count))
}
