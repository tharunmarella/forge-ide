//! End-to-end agent performance benchmark.
//!
//! Runs a set of read-only "explain" queries through the full agent pipeline
//! (enriched prompt + agent loop + tool calls) and reports detailed metrics:
//! turns, tool calls, latency, which tools were used.
//!
//! # Usage
//!
//! ```bash
//! # Default: uses gemini-2.5-flash
//! GEMINI_API_KEY="..." cargo test -p forge-agent --test bench_agent -- --ignored --nocapture
//!
//! # Override provider/model:
//! BENCH_PROVIDER=anthropic BENCH_MODEL=claude-sonnet-4-20250514 ANTHROPIC_API_KEY="..." \
//!   cargo test -p forge-agent --test bench_agent -- --ignored --nocapture
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use futures_util::StreamExt;

use forge_agent::rig::agent::MultiTurnStreamItem;
use forge_agent::rig::completion::CompletionModel;
use forge_agent::rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingPrompt};
use forge_agent::{
    ForgeAgentConfig, StandaloneBridge, TracingHook,
    build_enriched_prompt,
};

/// Build a lean prompt -- just user info + the raw question, no pre-fetched context.
/// Forces the model to use tools (grep, codebase_search, read_file) to find answers.
fn build_lean_prompt(user_query: &str, workspace_path: &str) -> String {
    let os_info = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let today = chrono::Local::now().format("%A %b %d, %Y").to_string();
    let primary_lang = forge_agent::detect_primary_language(std::path::Path::new(workspace_path));

    format!(
        "<user_info>\n\
         OS: {} {}\n\
         Workspace: {}\n\
         Today: {}\n\
         Primary language: {}\n\
         </user_info>\n\n\
         <user_query>\n\
         {}\n\
         </user_query>",
        os_info, arch, workspace_path, today,
        if primary_lang.is_empty() { "unknown" } else { &primary_lang },
        user_query,
    )
}

// ── Test queries ─────────────────────────────────────────────────

/// Read-only "explain" queries. These should be answerable in 0-2 tool calls
/// according to the system prompt, but currently take 10-17 turns.
const BENCH_QUERIES: &[(&str, &str)] = &[
    (
        "nl_to_query",
        "how does the natural language to query feature work in the database manager",
    ),
    (
        "agent_loop",
        "explain how the AI agent's tool execution loop works",
    ),
    (
        "repo_map",
        "what is the RepoMap and how is it built",
    ),
];

// ── Per-query metrics ────────────────────────────────────────────

#[derive(Debug)]
struct QueryMetrics {
    name: String,
    query: String,
    /// Enriched prompt size in characters
    prompt_chars: usize,
    /// Time to build the enriched prompt (context pre-fetching)
    context_build_ms: u128,
    /// Total agent execution time (including all LLM calls + tool calls)
    agent_time_s: f64,
    /// Number of LLM round-trips (turns)
    turns: usize,
    /// Total number of tool calls
    tool_calls: usize,
    /// Map of tool_name -> call count
    tool_breakdown: HashMap<String, usize>,
    /// Length of the final response text
    response_chars: usize,
    /// Whether the agent completed successfully
    success: bool,
    /// Error message if failed
    error: Option<String>,
}

impl QueryMetrics {
    fn tool_summary(&self) -> String {
        if self.tool_breakdown.is_empty() {
            return "none".to_string();
        }
        let mut pairs: Vec<_> = self.tool_breakdown.iter().collect();
        pairs.sort_by(|a, b| b.1.cmp(a.1));
        pairs
            .iter()
            .map(|(name, count)| format!("{}x{}", name, count))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

// ── Agent runner ─────────────────────────────────────────────────

/// Run the streaming agent loop and collect metrics.
/// This mirrors `run_cli_agent` from forge_cli.rs but captures structured metrics.
async fn run_agent_with_metrics<M>(
    agent: forge_agent::rig::agent::Agent<M>,
    prompt: &str,
    hook: TracingHook,
) -> (String, usize, usize, HashMap<String, usize>, Option<String>)
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
    let mut tool_breakdown: HashMap<String, usize> = HashMap::new();
    let mut loop_detector = forge_agent::loop_detection::LoopDetector::new();

    while let Some(item) = stream.next().await {
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::Text(text),
            )) => {
                full_response.push_str(&text.text);

                // Check for content looping
                let loop_check = loop_detector.check_content(&text.text);
                if loop_check.is_loop() {
                    let msg = loop_check.message();
                    eprintln!("    [LOOP] {}", msg);
                    hook.write_event("loop_detected", serde_json::json!({ "reason": msg }));
                    hook.write_session_end();
                    return (full_response, turn, tool_call_count, tool_breakdown, Some(msg));
                }
            }
            Ok(MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ToolCall { tool_call, .. },
            )) => {
                turn += 1;
                tool_call_count += 1;
                let name = tool_call.function.name.clone();
                *tool_breakdown.entry(name.clone()).or_insert(0) += 1;

                let args = serde_json::to_string(&tool_call.function.arguments)
                    .unwrap_or_default();
                let args_short = if args.len() > 100 {
                    format!("{}...", &args[..100])
                } else {
                    args
                };
                eprintln!("    [turn {}] {} {}", turn, name, args_short);

                // Check for tool call loop
                let full_args = serde_json::to_string(&tool_call.function.arguments)
                    .unwrap_or_default();
                let loop_check = loop_detector.check_tool_call(&name, &full_args);
                if loop_check.is_loop() {
                    let msg = loop_check.message();
                    eprintln!("    [LOOP] {}", msg);
                    hook.write_event("loop_detected", serde_json::json!({ "reason": msg }));
                    hook.write_session_end();
                    return (full_response, turn, tool_call_count, tool_breakdown, Some(msg));
                }
            }
            Ok(MultiTurnStreamItem::StreamUserItem(
                StreamedUserContent::ToolResult { tool_result, .. },
            )) => {
                let output_len: usize = tool_result
                    .content
                    .iter()
                    .map(|c| {
                        if let forge_agent::rig::message::ToolResultContent::Text(t) = c {
                            t.text.len()
                        } else {
                            0
                        }
                    })
                    .sum();
                eprintln!("           => {} chars", output_len);
            }
            Ok(MultiTurnStreamItem::FinalResponse(_)) => {
                hook.write_session_end();
                break;
            }
            Err(e) => {
                let msg = e.to_string();
                eprintln!("    [ERROR] {}", msg);
                hook.write_event("error", serde_json::json!({ "error": &msg }));
                hook.write_session_end();
                return (full_response, turn, tool_call_count, tool_breakdown, Some(msg));
            }
            _ => {
                // ToolCallDelta, ReasoningDelta, etc.
            }
        }
    }

    (full_response, turn, tool_call_count, tool_breakdown, None)
}

// ── Config helpers ───────────────────────────────────────────────

fn bench_provider() -> String {
    std::env::var("BENCH_PROVIDER").unwrap_or_else(|_| "gemini".to_string())
}

fn bench_model() -> String {
    std::env::var("BENCH_MODEL").unwrap_or_else(|_| {
        match bench_provider().as_str() {
            "gemini" => "gemini-2.5-flash".to_string(),
            "anthropic" => "claude-sonnet-4-20250514".to_string(),
            "openai" => "gpt-4o".to_string(),
            _ => "gemini-2.5-flash".to_string(),
        }
    })
}

fn bench_api_key() -> String {
    let provider = bench_provider();
    let var = match provider.as_str() {
        "gemini" => "GEMINI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        _ => "LLM_API_KEY",
    };
    std::env::var(var).unwrap_or_else(|_| {
        panic!(
            "Missing API key: set {} or BENCH_PROVIDER to a different provider",
            var
        )
    })
}

fn workspace_path() -> std::path::PathBuf {
    // Use the forge-ide workspace itself as the test workspace
    std::env::var("BENCH_WORKSPACE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            // Assumes we're running from the forge-ide project root
            let manifest = std::env::var("CARGO_MANIFEST_DIR")
                .unwrap_or_else(|_| ".".to_string());
            std::path::Path::new(&manifest)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf()
        })
}

// ── Prompt mode ──────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum PromptMode {
    /// Full enriched prompt: repo_map + pre_search + project_layout + git_status
    Enriched,
    /// Lean prompt: just user_info + the raw question, model must use tools
    Lean,
}

impl PromptMode {
    fn label(&self) -> &'static str {
        match self {
            PromptMode::Enriched => "ENRICHED",
            PromptMode::Lean => "LEAN",
        }
    }
}

// ── Core benchmark runner ────────────────────────────────────────

async fn run_benchmark(mode: PromptMode) {
    let provider = bench_provider();
    let model = bench_model();
    let api_key = bench_api_key();
    let workspace = workspace_path();
    let workspace_str = workspace.display().to_string();

    eprintln!();
    eprintln!("╔══════════════════════════════════════════════════════════════╗");
    eprintln!("║         FORGE AGENT PERFORMANCE BENCHMARK                   ║");
    eprintln!("╚══════════════════════════════════════════════════════════════╝");
    eprintln!();
    eprintln!("  Mode:      {}", mode.label());
    eprintln!("  Provider:  {}", provider);
    eprintln!("  Model:     {}", model);
    eprintln!("  Workspace: {}", workspace_str);
    eprintln!("  Queries:   {}", BENCH_QUERIES.len());
    eprintln!();

    let config = ForgeAgentConfig {
        provider: provider.clone(),
        model: model.clone(),
        api_key,
        max_turns: 15,
        temperature: Some(0.0),
        ..Default::default()
    };

    let bridge: Arc<dyn forge_agent::bridge::ProxyBridge> =
        Arc::new(StandaloneBridge::new(workspace.clone()));

    let mut all_metrics: Vec<QueryMetrics> = Vec::new();

    for (name, query) in BENCH_QUERIES {
        eprintln!("────────────────────────────────────────────────────────────");
        eprintln!("  [{}] Query: \"{}\"", mode.label(), query);
        eprintln!("  Name:  {}", name);
        eprintln!();

        // 1. Build prompt based on mode
        let ctx_start = Instant::now();
        let prompt = match mode {
            PromptMode::Enriched => build_enriched_prompt(query, &workspace_str),
            PromptMode::Lean => build_lean_prompt(query, &workspace_str),
        };
        let context_build_ms = ctx_start.elapsed().as_millis();
        let prompt_chars = prompt.len();

        eprintln!("  Prompt: {} chars, built in {}ms", prompt_chars, context_build_ms);

        // 2. Create trace file
        let trace_dir = std::env::temp_dir().join("forge-bench-traces");
        let mode_tag = match mode {
            PromptMode::Enriched => "enriched",
            PromptMode::Lean => "lean",
        };
        let trace_file = trace_dir.join(format!(
            "bench-{}-{}-{}.jsonl",
            mode_tag,
            name,
            chrono::Utc::now().format("%Y%m%d-%H%M%S"),
        ));
        let hook = TracingHook::new(&trace_file).expect("create trace file");
        eprintln!("  Trace:   {}", trace_file.display());

        hook.write_event(
            "bench_start",
            serde_json::json!({
                "query": query,
                "name": name,
                "mode": mode.label(),
                "provider": &provider,
                "model": &model,
                "prompt_chars": prompt_chars,
                "context_build_ms": context_build_ms,
            }),
        );

        // 3. Create agent and run
        let agent_start = Instant::now();

        let (response, turns, tool_calls, tool_breakdown, error) = match provider.as_str() {
            "gemini" => {
                let agent = forge_agent::create_agent_gemini(&config, bridge.clone())
                    .expect("create gemini agent");
                run_agent_with_metrics(agent, &prompt, hook.clone()).await
            }
            "anthropic" => {
                let agent = forge_agent::create_agent_anthropic(&config, bridge.clone())
                    .expect("create anthropic agent");
                run_agent_with_metrics(agent, &prompt, hook.clone()).await
            }
            "openai" => {
                let agent = forge_agent::create_agent_openai(&config, bridge.clone())
                    .expect("create openai agent");
                run_agent_with_metrics(agent, &prompt, hook.clone()).await
            }
            other => panic!("Unknown provider: {}", other),
        };

        let agent_time_s = agent_start.elapsed().as_secs_f64();

        let metrics = QueryMetrics {
            name: name.to_string(),
            query: query.to_string(),
            prompt_chars,
            context_build_ms,
            agent_time_s,
            turns,
            tool_calls,
            tool_breakdown,
            response_chars: response.len(),
            success: error.is_none(),
            error,
        };

        eprintln!();
        eprintln!("  Result: {} turns, {} tool calls, {:.1}s",
            metrics.turns, metrics.tool_calls, metrics.agent_time_s);
        eprintln!("  Tools:  {}", metrics.tool_summary());
        eprintln!("  Response: {} chars", metrics.response_chars);
        if let Some(ref err) = metrics.error {
            eprintln!("  Error: {}", err);
        }
        eprintln!();

        all_metrics.push(metrics);
    }

    // ── Summary table ────────────────────────────────────────────

    print_summary(&all_metrics, mode.label());
}

fn print_summary(all_metrics: &[QueryMetrics], label: &str) {
    eprintln!();
    eprintln!("╔══════════════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║  RESULTS: {:<70}  ║", label);
    eprintln!("╠══════════════════════════════════════════════════════════════════════════════════════╣");
    eprintln!("║ {:<14} │ {:>5} │ {:>5} │ {:>7} │ {:>7} │ {:>8} │ {:<20} ║",
        "Query", "Turns", "Tools", "Time(s)", "Ctx(ms)", "Resp(ch)", "Tool breakdown");
    eprintln!("╠══════════════════════════════════════════════════════════════════════════════════════╣");

    let mut total_turns = 0;
    let mut total_tools = 0;
    let mut total_time = 0.0;

    for m in all_metrics {
        let status = if m.success { " " } else { "!" };
        let tool_summary = m.tool_summary();
        let tool_display = if tool_summary.len() > 20 {
            format!("{}...", &tool_summary[..17])
        } else {
            tool_summary
        };
        eprintln!("║{}{:<14} │ {:>5} │ {:>5} │ {:>7.1} │ {:>7} │ {:>8} │ {:<20} ║",
            status, m.name, m.turns, m.tool_calls, m.agent_time_s,
            m.context_build_ms, m.response_chars, tool_display);
        total_turns += m.turns;
        total_tools += m.tool_calls;
        total_time += m.agent_time_s;
    }

    eprintln!("╠══════════════════════════════════════════════════════════════════════════════════════╣");
    eprintln!("║ {:<14} │ {:>5} │ {:>5} │ {:>7.1} │         │          │                      ║",
        "TOTAL", total_turns, total_tools, total_time);
    eprintln!("║ {:<14} │ {:>5.1} │ {:>5.1} │ {:>7.1} │         │          │                      ║",
        "AVG/query",
        total_turns as f64 / all_metrics.len() as f64,
        total_tools as f64 / all_metrics.len() as f64,
        total_time / all_metrics.len() as f64);
    eprintln!("╚══════════════════════════════════════════════════════════════════════════════════════╝");
    eprintln!();

    // ── Detailed tool breakdown ──────────────────────────────────

    let mut global_tools: HashMap<String, usize> = HashMap::new();
    for m in all_metrics {
        for (tool, count) in &m.tool_breakdown {
            *global_tools.entry(tool.clone()).or_insert(0) += count;
        }
    }

    if !global_tools.is_empty() {
        let mut sorted: Vec<_> = global_tools.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));

        eprintln!("  Tool usage across all queries:");
        for (tool, count) in &sorted {
            let bar = "█".repeat(*(*count).min(&30));
            eprintln!("    {:<20} {:>3}  {}", tool, count, bar);
        }
        eprintln!();
    }

    let failed = all_metrics.iter().filter(|m| !m.success).count();
    if failed > 0 {
        eprintln!("  WARNING: {} of {} queries failed", failed, all_metrics.len());
    }
}

// ── The benchmark tests ──────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
#[ignore] // Requires API keys and network -- run with: cargo test --test bench_agent bench_enriched -- --ignored --nocapture
async fn bench_enriched() {
    run_benchmark(PromptMode::Enriched).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore] // Requires API keys and network -- run with: cargo test --test bench_agent bench_lean -- --ignored --nocapture
async fn bench_lean() {
    run_benchmark(PromptMode::Lean).await;
}
