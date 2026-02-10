//! forge-diagram -- generate comprehensive architecture diagrams from the codebase.
//!
//! Uses tree-sitter to extract every function, struct, method, and their
//! cross-file call relationships. Outputs Graphviz DOT format.
//!
//! Usage:
//!   cargo run --bin forge-diagram -- [OPTIONS] [WORKSPACE]
//!
//!   # Full diagram (all symbols, all connections)
//!   cargo run --bin forge-diagram -- /path/to/project > architecture.dot
//!   dot -Tsvg architecture.dot -o architecture.svg
//!
//!   # Focus on a specific crate/directory
//!   cargo run --bin forge-diagram -- --focus forge-agent/src /path/to/project
//!
//!   # Mermaid output (for GitHub READMEs)
//!   cargo run --bin forge-diagram -- --format mermaid /path/to/project
//!
//!   # Only show top-N ranked symbols (less noise)
//!   cargo run --bin forge-diagram -- --top 50 /path/to/project

use std::collections::{HashMap, HashSet, BTreeMap};
use std::path::PathBuf;

use clap::Parser;
use walkdir::WalkDir;

use forge_agent::repomap;

#[derive(Parser)]
#[command(name = "forge-diagram", about = "Generate codebase architecture diagrams")]
struct Cli {
    /// Workspace root directory
    #[arg(default_value = ".")]
    workspace: PathBuf,

    /// Output format: dot (Graphviz) or mermaid
    #[arg(long, default_value = "dot")]
    format: String,

    /// Focus on a subdirectory (e.g., "forge-agent/src")
    #[arg(long)]
    focus: Option<String>,

    /// Only show top-N symbols by PageRank importance
    #[arg(long, default_value_t = 200)]
    top: usize,

    /// Show field-level detail (struct fields, method params)
    #[arg(long)]
    fields: bool,

    /// Minimum call count to show an edge (reduces noise)
    #[arg(long, default_value_t = 1)]
    min_calls: usize,
}

/// A definition with its location
#[derive(Debug, Clone)]
struct DefInfo {
    file: String,
    name: String,
    signature: String,
    line: usize,
    syntax_type: String, // "function", "method", "class", "interface", "module"
}

/// A reference (call) from one location to a symbol
#[derive(Debug, Clone)]
struct RefInfo {
    file: String,
    name: String,
    line: usize,
}

fn main() {
    let cli = Cli::parse();
    let workspace = cli.workspace.canonicalize().unwrap_or(cli.workspace.clone());

    // 1. Collect source files
    let configs = repomap::lang_configs();
    let supported_ext: HashSet<&str> = configs.keys().copied().collect();

    let source_files: Vec<PathBuf> = WalkDir::new(&workspace)
        .max_depth(12)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let path_str = e.path().to_string_lossy();
            !path_str.contains("node_modules")
                && !path_str.contains("/target/")
                && !path_str.contains("/.git/")
                && !path_str.contains("/vendor/")
                && !path_str.contains("/reference-repos/")
                && !path_str.contains("/__pycache__/")
                && !path_str.contains("/.venv/")
                && !path_str.contains("/dist/")
                && !path_str.contains("/build/")
        })
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| supported_ext.contains(ext))
                .unwrap_or(false)
        })
        .filter(|e| {
            if let Some(ref focus) = cli.focus {
                let rel = e.path().strip_prefix(&workspace).unwrap_or(e.path());
                rel.to_string_lossy().starts_with(focus.as_str())
            } else {
                true
            }
        })
        .take(1000)
        .map(|e| e.into_path())
        .collect();

    eprintln!("Scanning {} source files...", source_files.len());

    // 2. Extract all tags using tree-sitter
    let mut all_defs: Vec<DefInfo> = Vec::new();
    let mut all_refs: Vec<RefInfo> = Vec::new();

    let mut tag_ctx = tree_sitter_tags::TagsContext::new();

    for file in &source_files {
        let ext = match file.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => continue,
        };
        let lang_cfg = match configs.get(ext) {
            Some(c) => c,
            None => continue,
        };

        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if content.is_empty() {
            continue;
        }

        let source_bytes = content.as_bytes();
        let lines: Vec<&str> = content.lines().collect();
        let rel_path = file
            .strip_prefix(&workspace)
            .unwrap_or(file)
            .to_string_lossy()
            .to_string();

        match tag_ctx.generate_tags(lang_cfg.config(), source_bytes, None) {
            Ok((tag_iter, _)) => {
                for tag_result in tag_iter {
                    if let Ok(tag) = tag_result {
                        let name =
                            match std::str::from_utf8(&source_bytes[tag.name_range.clone()]) {
                                Ok(n) => n.to_string(),
                                Err(_) => continue,
                            };

                        if name.len() < 2 {
                            continue;
                        }

                        let line = tag.span.start.row + 1;

                        if tag.is_definition {
                            let signature = lines
                                .get(tag.span.start.row)
                                .map(|l| l.trim().to_string())
                                .unwrap_or_default();

                            let syntax_type = lang_cfg
                                .config()
                                .syntax_type_name(tag.syntax_type_id as u32)
                                .to_string();

                            all_defs.push(DefInfo {
                                file: rel_path.clone(),
                                name,
                                signature,
                                line,
                                syntax_type,
                            });
                        } else {
                            all_refs.push(RefInfo {
                                file: rel_path.clone(),
                                name,
                                line,
                            });
                        }
                    }
                }
            }
            Err(_) => continue,
        }
    }

    eprintln!(
        "Found {} definitions, {} references",
        all_defs.len(),
        all_refs.len()
    );

    // 3. Build lookup: symbol_name -> Vec<DefInfo>
    let mut def_map: HashMap<String, Vec<&DefInfo>> = HashMap::new();
    for d in &all_defs {
        def_map.entry(d.name.clone()).or_default().push(d);
    }

    // 4. Build edges: (source_file:source_symbol) -> (target_file:target_symbol)
    //    A reference in file A to symbol X, where X is defined in file B, creates an edge.
    let mut edges: HashMap<(String, String), usize> = HashMap::new(); // (from_key, to_key) -> count

    // For each reference, find where the symbol is defined
    // Group refs by file so we can figure out which function the ref is "inside"
    for r in &all_refs {
        if let Some(defs) = def_map.get(&r.name) {
            // Find which function in the referencing file contains this reference
            let caller = find_enclosing_def(&all_defs, &r.file, r.line);
            let from_key = if let Some(c) = &caller {
                format!("{}::{}", short_file(&c.file), c.name)
            } else {
                format!("{}::<module>", short_file(&r.file))
            };

            for d in defs {
                // Skip self-references within same function
                if d.file == r.file && d.name == r.name {
                    if let Some(ref c) = caller {
                        if c.name == d.name {
                            continue;
                        }
                    }
                }

                let to_key = format!("{}::{}", short_file(&d.file), d.name);
                if from_key != to_key {
                    *edges.entry((from_key.clone(), to_key)).or_insert(0) += 1;
                }
            }
        }
    }

    // Filter edges by min_calls
    let edges: Vec<_> = edges
        .into_iter()
        .filter(|(_, count)| *count >= cli.min_calls)
        .collect();

    eprintln!("Built {} edges (min_calls={})", edges.len(), cli.min_calls);

    // 5. Compute which symbols appear in edges (to filter to only connected symbols)
    let mut connected_symbols: HashSet<String> = HashSet::new();
    for ((from, to), _) in &edges {
        connected_symbols.insert(from.clone());
        connected_symbols.insert(to.clone());
    }

    // 6. If --top is set, only keep top-N most connected symbols
    let mut symbol_edge_count: HashMap<String, usize> = HashMap::new();
    for ((from, to), count) in &edges {
        *symbol_edge_count.entry(from.clone()).or_insert(0) += count;
        *symbol_edge_count.entry(to.clone()).or_insert(0) += count;
    }
    let mut ranked_symbols: Vec<_> = symbol_edge_count.iter().collect();
    ranked_symbols.sort_by(|a, b| b.1.cmp(a.1));
    let top_symbols: HashSet<String> = ranked_symbols
        .into_iter()
        .take(cli.top)
        .map(|(k, _)| k.clone())
        .collect();

    // 7. Output
    match cli.format.as_str() {
        "mermaid" => output_mermaid(&all_defs, &edges, &top_symbols),
        _ => output_dot(&all_defs, &edges, &top_symbols),
    }
}

/// Find the enclosing function/method definition for a line in a file
fn find_enclosing_def<'a>(defs: &'a [DefInfo], file: &str, line: usize) -> Option<&'a DefInfo> {
    let mut best: Option<&DefInfo> = None;

    for d in defs {
        if d.file != file {
            continue;
        }
        // The definition must start before our line
        if d.line <= line {
            // Pick the closest one (largest start line that's still <= our line)
            if let Some(current_best) = best {
                if d.line > current_best.line {
                    best = Some(d);
                }
            } else {
                best = Some(d);
            }
        }
    }

    best
}

/// Shorten a file path for display
fn short_file(path: &str) -> String {
    // "forge-agent/src/tools/search.rs" -> "tools/search"
    let p = path
        .trim_end_matches(".rs")
        .trim_end_matches(".py")
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".js")
        .trim_end_matches(".go");

    // Remove common prefixes
    let p = p
        .strip_prefix("forge-agent/src/")
        .or(p.strip_prefix("lapce-app/src/"))
        .or(p.strip_prefix("lapce-proxy/src/"))
        .unwrap_or(p);

    p.to_string()
}

/// Sanitize a string for use as a DOT identifier
fn dot_id(s: &str) -> String {
    s.replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
}

fn output_dot(all_defs: &[DefInfo], edges: &[((String, String), usize)], top_symbols: &HashSet<String>) {
    println!("digraph forge {{");
    println!("  rankdir=LR;");
    println!("  fontname=\"Helvetica\";");
    println!("  node [fontname=\"Helvetica\", fontsize=10, shape=box, style=\"rounded,filled\"];");
    println!("  edge [fontname=\"Helvetica\", fontsize=8, color=\"#666666\"];");
    println!();

    // Group definitions by file for subgraph clusters
    let mut file_defs: BTreeMap<String, Vec<&DefInfo>> = BTreeMap::new();
    for d in all_defs {
        let key = format!("{}::{}", short_file(&d.file), d.name);
        if top_symbols.contains(&key) {
            file_defs.entry(d.file.clone()).or_default().push(d);
        }
    }

    // Color scheme by syntax type
    let color_for = |syntax_type: &str| -> &str {
        match syntax_type {
            "function" => "#E3F2FD",  // light blue
            "method" => "#E8F5E9",    // light green
            "class" => "#FFF3E0",     // light orange
            "interface" => "#F3E5F5", // light purple
            "module" => "#FFFDE7",    // light yellow
            "macro" => "#FCE4EC",     // light pink
            _ => "#F5F5F5",           // light grey
        }
    };

    let shape_for = |syntax_type: &str| -> &str {
        match syntax_type {
            "class" | "interface" => "component",
            "module" => "folder",
            _ => "box",
        }
    };

    // Emit subgraph clusters for each file
    for (i, (file, defs)) in file_defs.iter().enumerate() {
        let short = short_file(file);
        println!("  subgraph cluster_{} {{", i);
        println!("    label=\"{}\";", short);
        println!("    style=\"rounded,dashed\";");
        println!("    color=\"#BBBBBB\";");
        println!("    fontsize=11;");
        println!("    fontcolor=\"#555555\";");
        println!();

        for d in defs {
            let node_id = dot_id(&format!("{}::{}", short_file(&d.file), d.name));
            let label = if d.signature.len() > 60 {
                format!("{}...", &d.signature[..57])
            } else if d.signature.is_empty() {
                d.name.clone()
            } else {
                d.signature.clone()
            };
            // Escape quotes for DOT
            let label = label.replace('"', "\\\"");

            println!(
                "    {} [label=\"{}\", fillcolor=\"{}\", shape=\"{}\"];",
                node_id,
                label,
                color_for(&d.syntax_type),
                shape_for(&d.syntax_type),
            );
        }
        println!("  }}");
        println!();
    }

    // Emit edges
    for ((from, to), count) in edges {
        if !top_symbols.contains(from) || !top_symbols.contains(to) {
            continue;
        }
        let from_id = dot_id(from);
        let to_id = dot_id(to);

        let penwidth = if *count >= 5 {
            "2.5"
        } else if *count >= 3 {
            "1.5"
        } else {
            "0.8"
        };

        if *count > 1 {
            println!(
                "  {} -> {} [penwidth={}, label=\"{}\"];",
                from_id, to_id, penwidth, count
            );
        } else {
            println!("  {} -> {} [penwidth={}];", from_id, to_id, penwidth);
        }
    }

    println!("}}");
}

fn output_mermaid(all_defs: &[DefInfo], edges: &[((String, String), usize)], top_symbols: &HashSet<String>) {
    println!("```mermaid");
    println!("graph LR");
    println!();

    // Group definitions by file
    let mut file_defs: BTreeMap<String, Vec<&DefInfo>> = BTreeMap::new();
    for d in all_defs {
        let key = format!("{}::{}", short_file(&d.file), d.name);
        if top_symbols.contains(&key) {
            file_defs.entry(d.file.clone()).or_default().push(d);
        }
    }

    // Emit subgraphs
    for (file, defs) in &file_defs {
        let short = short_file(file);
        let sub_id = dot_id(&short);
        println!("  subgraph {}[\"{}\"]", sub_id, short);
        for d in defs {
            let node_id = dot_id(&format!("{}::{}", short_file(&d.file), d.name));
            let label = if d.name.len() > 30 {
                format!("{}...", &d.name[..27])
            } else {
                d.name.clone()
            };
            println!("    {}[\"{}\" ]", node_id, label);
        }
        println!("  end");
    }
    println!();

    // Emit edges
    for ((from, to), count) in edges {
        if !top_symbols.contains(from) || !top_symbols.contains(to) {
            continue;
        }
        let from_id = dot_id(from);
        let to_id = dot_id(to);

        if *count > 1 {
            println!("  {} -->|\"{}\"| {}", from_id, count, to_id);
        } else {
            println!("  {} --> {}", from_id, to_id);
        }
    }

    println!("```");
}
