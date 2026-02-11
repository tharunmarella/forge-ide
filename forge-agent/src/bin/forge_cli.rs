//! forge-cli -- standalone CLI for testing the Forge AI agent via forge-search.
//!
//! Usage:
//!   cargo run --release --bin forge-cli -- \
//!     --workspace /path/to/project \
//!     "what vision model are we using?"

use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;

// ── ANSI colors ──────────────────────────────────────────────────
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[derive(Parser)]
#[command(name = "forge-cli", about = "Test the Forge AI agent via forge-search")]
struct Cli {
    /// Workspace directory the agent will operate on
    #[arg(long, default_value = ".")]
    workspace: String,

    /// The prompt to send to the agent
    prompt: String,
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    let workspace_path = std::fs::canonicalize(&cli.workspace).unwrap_or_else(|e| {
        eprintln!("{RED}Error:{RESET} Bad workspace path '{}': {e}", cli.workspace);
        std::process::exit(1);
    });

    let workspace_id = workspace_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default");

    eprintln!(
        "{CYAN}[context]{RESET} Workspace: {} (id: {})",
        workspace_path.display(),
        workspace_id,
    );

    let client = forge_agent::forge_search::client();

    // Check auth
    if !client.is_signed_in().await {
        eprintln!("{YELLOW}Warning:{RESET} Not signed in to forge-search. Some features may be limited.");
        eprintln!("{DIM}Sign in at: {}{RESET}", client.login_url());
    }

    // First, trigger indexing
    eprintln!("{CYAN}[index]{RESET} Syncing workspace with forge-search...");
    let index_start = Instant::now();
    match client.scan_directory(workspace_id, &workspace_path).await {
        Ok(result) => {
            eprintln!(
                "{CYAN}[index]{RESET} Indexed {} files ({} symbols) in {:.1}s",
                result.files_indexed,
                result.nodes_created,
                index_start.elapsed().as_secs_f64(),
            );
        }
        Err(e) => {
            eprintln!("{YELLOW}[index]{RESET} Indexing failed: {e}");
        }
    }

    // Send chat request
    eprintln!("{CYAN}[chat]{RESET} Sending prompt to forge-search...");
    let chat_start = Instant::now();

    match client.chat(workspace_id, &cli.prompt, true, true).await {
        Ok(response) => {
            let answer = response
                .get("answer")
                .and_then(|v| v.as_str())
                .unwrap_or("No response");

            eprintln!(
                "{GREEN}{BOLD}[done]{RESET} Response received in {:.1}s",
                chat_start.elapsed().as_secs_f64(),
            );
            eprintln!();
            println!("{}", answer);
        }
        Err(e) => {
            eprintln!("{RED}{BOLD}[error]{RESET} {e}");
            std::process::exit(1);
        }
    }
}
