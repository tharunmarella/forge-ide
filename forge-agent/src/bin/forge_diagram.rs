//! forge-diagram -- generate codebase diagrams.
//!
//! This tool is temporarily disabled while the codebase is being refactored.
//! The diagram generation will be moved to forge-search which has the graph data.

fn main() {
    eprintln!("forge-diagram is being migrated to forge-search.");
    eprintln!("Use the forge-search API for graph visualization:");
    eprintln!("  POST /trace - for call chain visualization");
    eprintln!("  POST /impact - for impact analysis");
    std::process::exit(0);
}
