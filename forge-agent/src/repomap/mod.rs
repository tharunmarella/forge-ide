//! Repository Map - Aider-style codebase understanding
//!
//! Builds a semantic map of the codebase by:
//! 1. Extracting symbol definitions and references using tree-sitter
//! 2. Building a graph of file relationships
//! 3. Ranking files/symbols using PageRank
//! 4. Fitting the most important symbols into a token budget
//!
//! Uses `tree-sitter-tags` for accurate, language-aware symbol extraction
//! instead of fragile regex patterns. This gives us proper understanding of:
//! - Struct/class/enum definitions, trait/interface definitions
//! - Function/method definitions with correct scope
//! - Cross-file references (calls, implementations, type usages)
//! - Accurate line numbers for signature display

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::page_rank;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;
use tree_sitter_tags::{TagsConfiguration, TagsContext};
use walkdir::WalkDir;

// ══════════════════════════════════════════════════════════════════
//  ENHANCED TYPESCRIPT TAGS QUERY
// ══════════════════════════════════════════════════════════════════
//
// The bundled tree-sitter-typescript 0.21 TAGS_QUERY is very minimal
// (only interface, abstract class, module, type annotations).
// We provide a comprehensive query that covers all TS/TSX constructs.

const ENHANCED_TS_TAGS_QUERY: &str = r#"
; ── Class declarations ──
(class_declaration
  name: (type_identifier) @name) @definition.class

(abstract_class_declaration
  name: (type_identifier) @name) @definition.class

; ── Interface declarations ──
(interface_declaration
  name: (type_identifier) @name) @definition.interface

; ── Type alias ──
(type_alias_declaration
  name: (type_identifier) @name) @definition.interface

; ── Enum declarations ──
(enum_declaration
  name: (identifier) @name) @definition.class

; ── Function declarations ──
(function_declaration
  name: (identifier) @name) @definition.function

(function_signature
  name: (identifier) @name) @definition.function

; ── Method definitions ──
(method_definition
  name: (property_identifier) @name) @definition.method

(method_signature
  name: (property_identifier) @name) @definition.method

(abstract_method_signature
  name: (property_identifier) @name) @definition.method

; ── Arrow functions / const assignments ──
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: [(arrow_function) (function_expression)]) @definition.function)

(variable_declaration
  (variable_declarator
    name: (identifier) @name
    value: [(arrow_function) (function_expression)]) @definition.function)

; ── Module ──
(module
  name: (identifier) @name) @definition.module

; ── References ──
(type_annotation
  (type_identifier) @name) @reference.type

(new_expression
  constructor: (identifier) @name) @reference.class

(call_expression
  function: (identifier) @name) @reference.call

(call_expression
  function: (member_expression
    property: (property_identifier) @name)) @reference.call
"#;

// ══════════════════════════════════════════════════════════════════
//  LANGUAGE CONFIGURATIONS (initialized once, reused forever)
// ══════════════════════════════════════════════════════════════════

pub struct LangConfig {
    config: TagsConfiguration,
    /// Human-readable language name for diagnostics
    #[allow(dead_code)]
    name: &'static str,
}

impl LangConfig {
    /// Access the underlying TagsConfiguration for tag generation.
    pub fn config(&self) -> &TagsConfiguration {
        &self.config
    }
}

// SAFETY: TagsConfiguration contains tree-sitter Language pointers that are
// effectively immutable static data (compiled grammar tables). They are safe
// to share across threads. The TagsContext (which is NOT Sync) is created
// per-call in extract_tags(), never shared.
unsafe impl Send for LangConfig {}
unsafe impl Sync for LangConfig {}

/// All supported language configs, lazily initialized once.
pub fn lang_configs() -> &'static HashMap<&'static str, LangConfig> {
    static CONFIGS: OnceLock<HashMap<&'static str, LangConfig>> = OnceLock::new();
    CONFIGS.get_or_init(|| {
        let mut map = HashMap::new();

        // Rust
        if let Ok(cfg) = TagsConfiguration::new(
            tree_sitter_rust::language(),
            tree_sitter_rust::TAGS_QUERY,
            "",
        ) {
            map.insert("rs", LangConfig { config: cfg, name: "Rust" });
        }

        // Python
        if let Ok(cfg) = TagsConfiguration::new(
            tree_sitter_python::language(),
            tree_sitter_python::TAGS_QUERY,
            "",
        ) {
            map.insert("py", LangConfig { config: cfg, name: "Python" });
        }

        // JavaScript / JSX -- use TypeScript TSX parser (TS is a JS superset)
        // We skip tree-sitter-javascript crate due to its `cc ~1.0.90` constraint
        // conflicting with the workspace's cc 1.2.x.
        if let Ok(cfg) = TagsConfiguration::new(
            tree_sitter_typescript::language_tsx(),
            ENHANCED_TS_TAGS_QUERY,
            "",
        ) {
            map.insert("js", LangConfig { config: cfg, name: "JavaScript" });
        }
        if let Ok(cfg) = TagsConfiguration::new(
            tree_sitter_typescript::language_tsx(),
            ENHANCED_TS_TAGS_QUERY,
            "",
        ) {
            map.insert("jsx", LangConfig { config: cfg, name: "JSX" });
        }

        // TypeScript (using our enhanced query)
        if let Ok(cfg) = TagsConfiguration::new(
            tree_sitter_typescript::language_typescript(),
            ENHANCED_TS_TAGS_QUERY,
            "",
        ) {
            map.insert("ts", LangConfig { config: cfg, name: "TypeScript" });
        }

        // TSX (using our enhanced query with TSX grammar)
        if let Ok(cfg) = TagsConfiguration::new(
            tree_sitter_typescript::language_tsx(),
            ENHANCED_TS_TAGS_QUERY,
            "",
        ) {
            map.insert("tsx", LangConfig { config: cfg, name: "TSX" });
        }

        // Go
        if let Ok(cfg) = TagsConfiguration::new(
            tree_sitter_go::language(),
            tree_sitter_go::TAGS_QUERY,
            "",
        ) {
            map.insert("go", LangConfig { config: cfg, name: "Go" });
        }

        map
    })
}

// ══════════════════════════════════════════════════════════════════
//  TAG MODEL
// ══════════════════════════════════════════════════════════════════

/// A symbol tag extracted from source code
#[derive(Debug, Clone)]
pub struct Tag {
    pub rel_fname: String,
    pub abs_fname: PathBuf,
    pub line: usize,
    pub name: String,
    pub kind: TagKind,
    /// The source line text at the definition site (used for signature display)
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TagKind {
    Definition,
    Reference,
}

// ══════════════════════════════════════════════════════════════════
//  REPO MAP BUILDER
// ══════════════════════════════════════════════════════════════════

/// The main RepoMap builder
pub struct RepoMap {
    root: PathBuf,
    max_tokens: usize,
    tags_cache: HashMap<PathBuf, (std::time::SystemTime, Vec<Tag>)>,
}

impl RepoMap {
    pub fn new(root: PathBuf, max_tokens: usize) -> Self {
        Self {
            root,
            max_tokens,
            tags_cache: HashMap::new(),
        }
    }

    /// Build the repo map for the given files
    pub fn build(&mut self, chat_files: &[PathBuf], other_files: &[PathBuf]) -> String {
        let start = Instant::now();
        
        // Collect all tags from files
        let mut all_tags: Vec<Tag> = Vec::new();
        let mut defines: HashMap<String, HashSet<String>> = HashMap::new();
        let mut references: HashMap<String, Vec<String>> = HashMap::new();
        
        let all_files: HashSet<_> = chat_files.iter().chain(other_files.iter()).collect();
        
        for file in &all_files {
            if let Some(tags) = self.get_tags(file) {
                for tag in &tags {
                    match tag.kind {
                        TagKind::Definition => {
                            defines
                                .entry(tag.name.clone())
                                .or_default()
                                .insert(tag.rel_fname.clone());
                        }
                        TagKind::Reference => {
                            references
                                .entry(tag.name.clone())
                                .or_default()
                                .push(tag.rel_fname.clone());
                        }
                    }
                }
                all_tags.extend(tags);
            }
        }

        // Build graph: nodes = files, edges = references
        let mut graph: DiGraph<String, f64> = DiGraph::new();
        let mut node_indices: HashMap<String, NodeIndex> = HashMap::new();
        
        for file in &all_files {
            let rel = self.get_rel_fname(file);
            let idx = graph.add_node(rel.clone());
            node_indices.insert(rel, idx);
        }

        // Add edges based on cross-file references
        let symbols: HashSet<_> = defines.keys().collect();
        for symbol in symbols {
            if let (Some(definers), Some(referencers)) = (defines.get(symbol), references.get(symbol)) {
                for referencer in referencers {
                    if let Some(&ref_idx) = node_indices.get(referencer) {
                        for definer in definers {
                            if let Some(&def_idx) = node_indices.get(definer) {
                                if ref_idx != def_idx {
                                    let weight = self.symbol_weight(symbol);
                                    graph.add_edge(ref_idx, def_idx, weight);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Run PageRank
        let ranks = if graph.node_count() > 0 {
            page_rank(&graph, 0.85, 20)
        } else {
            vec![]
        };

        // Boost chat files in ranking
        let chat_rel_fnames: HashSet<_> = chat_files.iter()
            .map(|f| self.get_rel_fname(f))
            .collect();

        // Create ranked list of (file, rank, definition_tags)
        let mut file_ranks: Vec<(String, f64, Vec<&Tag>)> = Vec::new();
        for (node_idx, &rank) in ranks.iter().enumerate() {
            let node_idx = NodeIndex::new(node_idx);
            if let Some(fname) = graph.node_weight(node_idx) {
                if chat_rel_fnames.contains(fname) {
                    continue;
                }
                
                let file_tags: Vec<_> = all_tags.iter()
                    .filter(|t| &t.rel_fname == fname && t.kind == TagKind::Definition)
                    .collect();
                
                file_ranks.push((fname.clone(), rank, file_tags));
            }
        }

        file_ranks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let output = self.render_map(&file_ranks);
        
        let elapsed = start.elapsed();
        let def_count = all_tags.iter().filter(|t| t.kind == TagKind::Definition).count();
        let ref_count = all_tags.iter().filter(|t| t.kind == TagKind::Reference).count();
        tracing::info!(
            "[RepoMap] Built in {:?}: {} files, {} defs, {} refs, {} chars output",
            elapsed, all_files.len(), def_count, ref_count, output.len()
        );

        output
    }

    /// Get tags for a file, using cache if available
    fn get_tags(&mut self, path: &Path) -> Option<Vec<Tag>> {
        let mtime = std::fs::metadata(path).ok()?.modified().ok()?;
        
        if let Some((cached_mtime, cached_tags)) = self.tags_cache.get(path) {
            if *cached_mtime == mtime {
                return Some(cached_tags.clone());
            }
        }

        let tags = self.extract_tags(path)?;
        self.tags_cache.insert(path.to_path_buf(), (mtime, tags.clone()));
        Some(tags)
    }

    /// Extract tags using tree-sitter-tags for accurate AST-based symbol extraction.
    ///
    /// This replaces the old regex-based extraction with proper tree-sitter parsing.
    /// Benefits:
    /// - Accurate scope-aware definitions (methods vs functions vs classes)
    /// - Proper reference detection (calls, type usages, implementations)
    /// - Correct line numbers even with complex nesting
    /// - Language-specific understanding (e.g., Rust traits, Go interfaces, TS generics)
    fn extract_tags(&self, path: &Path) -> Option<Vec<Tag>> {
        let ext = path.extension()?.to_str()?;
        let configs = lang_configs();
        let lang_cfg = configs.get(ext)?;
        
        let content = std::fs::read_to_string(path).ok()?;
        if content.is_empty() {
            return Some(Vec::new());
        }
        let source_bytes = content.as_bytes();
        let lines: Vec<&str> = content.lines().collect();
        let rel_fname = self.get_rel_fname(path);
        
        let mut tags = Vec::new();
        let mut context = TagsContext::new();

        match context.generate_tags(&lang_cfg.config, source_bytes, None) {
            Ok((tag_iter, _has_error)) => {
                for tag_result in tag_iter {
                    match tag_result {
                        Ok(tag) => {
                            let name = match std::str::from_utf8(&source_bytes[tag.name_range.clone()]) {
                                Ok(n) => n.to_string(),
                                Err(_) => continue,
                            };

                            // Skip trivially short names (e.g., "a", "x")
                            if name.len() < 2 {
                                continue;
                            }

                            let line = tag.span.start.row + 1; // 1-indexed
                            let kind = if tag.is_definition {
                                TagKind::Definition
                            } else {
                                TagKind::Reference
                            };

                            // For definitions, capture the signature line
                            let signature = if tag.is_definition {
                                lines.get(tag.span.start.row).map(|l| l.trim().to_string())
                            } else {
                                None
                            };

                            tags.push(Tag {
                                rel_fname: rel_fname.clone(),
                                abs_fname: path.to_path_buf(),
                                line,
                                name,
                                kind,
                                signature,
                            });
                        }
                        Err(_) => continue,
                    }
                }
            }
            Err(e) => {
                tracing::debug!("[RepoMap] tree-sitter failed for {}: {}", rel_fname, e);
                return None;
            }
        }

        Some(tags)
    }

    /// Calculate weight for a symbol based on naming conventions
    fn symbol_weight(&self, symbol: &str) -> f64 {
        let mut weight = 1.0;
        
        if symbol.len() >= 8 {
            weight *= 2.0;
        }
        
        let is_camel = symbol.chars().any(|c| c.is_uppercase()) && symbol.chars().any(|c| c.is_lowercase());
        let is_snake = symbol.contains('_');
        if is_camel || is_snake {
            weight *= 2.0;
        }
        
        if symbol.starts_with('_') {
            weight *= 0.5;
        }
        
        weight
    }

    fn get_rel_fname(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    }

    /// Render the map to a string with signatures, fitting within token budget.
    ///
    /// Output format shows actual code signatures (much more useful than bare names):
    /// ```text
    /// src/auth/service.rs:
    /// │ pub struct AuthService {             (line 15)
    /// │ pub fn authenticate(&self, ...)      (line 42)
    /// │ pub fn refresh_token(...)            (line 78)
    ///
    /// src/models/user.rs:
    /// │ pub struct User {                    (line 5)
    /// │ pub fn new(name: &str) -> Self       (line 12)
    /// ```
    fn render_map(&self, file_ranks: &[(String, f64, Vec<&Tag>)]) -> String {
        let mut output = String::new();
        let mut estimated_tokens = 0;
        
        for (fname, _rank, tags) in file_ranks {
            if estimated_tokens >= self.max_tokens {
                break;
            }

            if tags.is_empty() {
                let line = format!("{}\n", fname);
                estimated_tokens += line.len() / 4;
                output.push_str(&line);
            } else {
                let header = format!("\n{}:\n", fname);
                estimated_tokens += header.len() / 4;
                output.push_str(&header);

                // Sort tags by line number, dedup by (name, line)
                let mut sorted_tags: Vec<_> = tags.iter().collect();
                sorted_tags.sort_by_key(|t| t.line);
                sorted_tags.dedup_by_key(|t| (&t.name, t.line));

                for tag in sorted_tags.iter().take(15) { // up to 15 symbols per file
                    let line = if let Some(sig) = &tag.signature {
                        // Show the actual signature (much more useful than just the name)
                        let sig_display = if sig.len() > 80 {
                            format!("{}...", &sig[..77])
                        } else {
                            sig.clone()
                        };
                        format!("│ {:<60} (line {})\n", sig_display, tag.line)
                    } else {
                        format!("│ {} (line {})\n", tag.name, tag.line)
                    };
                    estimated_tokens += line.len() / 4;
                    output.push_str(&line);
                    
                    if estimated_tokens >= self.max_tokens {
                        break;
                    }
                }
            }
        }

        output
    }

    /// Scan directory and build map for all source files
    pub fn build_from_directory(&mut self) -> String {
        let start = Instant::now();
        let mut source_files: Vec<PathBuf> = Vec::new();
        
        // Only include extensions we have tree-sitter grammars for
        let supported_extensions: HashSet<&str> = lang_configs().keys().copied().collect();
        const MAX_FILES: usize = 1000; // tree-sitter is fast enough for larger repos
        
        for entry in WalkDir::new(&self.root)
            .max_depth(12) // increased from 10 for deeper TS/JS project structures
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            if source_files.len() >= MAX_FILES {
                break;
            }
            
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if supported_extensions.contains(ext) {
                    let path_str = path.to_string_lossy();
                    if !path_str.contains("node_modules") 
                        && !path_str.contains("/target/")
                        && !path_str.contains("/.git/")
                        && !path_str.contains("/vendor/")
                        && !path_str.contains("/dist/")
                        && !path_str.contains("/build/")
                        && !path_str.contains("/__pycache__/")
                        && !path_str.contains("/.venv/")
                        && !path_str.contains("/venv/")
                        && !path_str.contains("/reference-repos/")
                        && !path_str.contains("/.cargo/")
                        && !path_str.contains("/pkg/mod/")
                        && !path_str.contains("/test_data/")
                        && !path_str.contains("/fixtures/")
                    {
                        source_files.push(path.to_path_buf());
                    }
                }
            }
        }
        
        let scan_time = start.elapsed();
        tracing::info!("[RepoMap] Scanned {} source files in {:?}", source_files.len(), scan_time);

        self.build(&[], &source_files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_weight() {
        let rm = RepoMap::new(PathBuf::from("."), 1024);
        
        assert!(rm.symbol_weight("MyClassName") > rm.symbol_weight("x"));
        assert!(rm.symbol_weight("authenticate_user") > rm.symbol_weight("a"));
        assert!(rm.symbol_weight("_private") < rm.symbol_weight("public_func"));
    }

    #[test]
    fn test_lang_configs_initialized() {
        let configs = lang_configs();
        assert!(configs.contains_key("rs"), "Rust config missing");
        assert!(configs.contains_key("py"), "Python config missing");
        assert!(configs.contains_key("js"), "JavaScript config missing");
        assert!(configs.contains_key("ts"), "TypeScript config missing");
        assert!(configs.contains_key("tsx"), "TSX config missing");
        assert!(configs.contains_key("go"), "Go config missing");
    }

    #[test]
    fn test_extract_rust_tags() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(f, "{}", r#"
pub struct User {
    pub name: String,
}

impl User {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }
}

pub fn create_user() -> User {
    User::new("test")
}
"#).unwrap();

        let mut rm = RepoMap::new(dir.path().to_path_buf(), 4096);
        let tags = rm.extract_tags(&file_path).unwrap();
        
        let def_names: Vec<&str> = tags.iter()
            .filter(|t| t.kind == TagKind::Definition)
            .map(|t| t.name.as_str())
            .collect();
        
        assert!(def_names.contains(&"User"), "Should find struct User, got: {:?}", def_names);
        assert!(def_names.contains(&"new"), "Should find fn new, got: {:?}", def_names);
        assert!(def_names.contains(&"create_user"), "Should find fn create_user, got: {:?}", def_names);
        
        // Check that references are also extracted
        let ref_names: Vec<&str> = tags.iter()
            .filter(|t| t.kind == TagKind::Reference)
            .map(|t| t.name.as_str())
            .collect();
        assert!(!ref_names.is_empty(), "Should find at least some references");
    }

    #[test]
    fn test_extract_python_tags() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.py");
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(f, "{}", r#"
class UserService:
    def __init__(self, db):
        self.db = db

    def get_user(self, user_id):
        return self.db.find(user_id)

def create_service():
    return UserService(None)
"#).unwrap();

        let mut rm = RepoMap::new(dir.path().to_path_buf(), 4096);
        let tags = rm.extract_tags(&file_path).unwrap();
        
        let def_names: Vec<&str> = tags.iter()
            .filter(|t| t.kind == TagKind::Definition)
            .map(|t| t.name.as_str())
            .collect();
        
        assert!(def_names.contains(&"UserService"), "Should find class UserService, got: {:?}", def_names);
        assert!(def_names.contains(&"get_user"), "Should find method get_user, got: {:?}", def_names);
        assert!(def_names.contains(&"create_service"), "Should find fn create_service, got: {:?}", def_names);
    }

    #[test]
    fn test_extract_typescript_tags() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.ts");
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(f, "{}", r#"
export interface AuthConfig {
    secret: string;
}

export class AuthService {
    constructor(private config: AuthConfig) {}

    authenticate(token: string): boolean {
        return token === this.config.secret;
    }
}

export function createAuth(config: AuthConfig): AuthService {
    return new AuthService(config);
}
"#).unwrap();

        let mut rm = RepoMap::new(dir.path().to_path_buf(), 4096);
        let tags = rm.extract_tags(&file_path).unwrap();
        
        let def_names: Vec<&str> = tags.iter()
            .filter(|t| t.kind == TagKind::Definition)
            .map(|t| t.name.as_str())
            .collect();
        
        assert!(def_names.contains(&"AuthConfig"), "Should find interface AuthConfig, got: {:?}", def_names);
        assert!(def_names.contains(&"AuthService"), "Should find class AuthService, got: {:?}", def_names);
        assert!(def_names.contains(&"authenticate"), "Should find method authenticate, got: {:?}", def_names);
        assert!(def_names.contains(&"createAuth"), "Should find fn createAuth, got: {:?}", def_names);
    }

    #[test]
    fn test_signatures_captured() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("sig.rs");
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(f, "pub fn process_data(input: &[u8], config: &Config) -> Result<Output> {{\n    todo!()\n}}\n").unwrap();

        let mut rm = RepoMap::new(dir.path().to_path_buf(), 4096);
        let tags = rm.extract_tags(&file_path).unwrap();
        
        let def_tags: Vec<_> = tags.iter()
            .filter(|t| t.kind == TagKind::Definition && t.name == "process_data")
            .collect();
        
        assert_eq!(def_tags.len(), 1);
        let sig = def_tags[0].signature.as_ref().unwrap();
        assert!(sig.contains("process_data"), "Signature should contain function name: {}", sig);
        assert!(sig.contains("Result<Output>"), "Signature should contain return type: {}", sig);
    }

    #[test]
    fn test_extract_go_tags() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.go");
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(f, "{}", r#"
package main

type User struct {
    Name  string
    Email string
}

func NewUser(name, email string) *User {
    return &User{Name: name, Email: email}
}

func (u *User) DisplayName() string {
    return u.Name
}
"#).unwrap();

        let mut rm = RepoMap::new(dir.path().to_path_buf(), 4096);
        let tags = rm.extract_tags(&file_path).unwrap();
        
        let def_names: Vec<&str> = tags.iter()
            .filter(|t| t.kind == TagKind::Definition)
            .map(|t| t.name.as_str())
            .collect();
        
        assert!(def_names.contains(&"User"), "Should find type User, got: {:?}", def_names);
        assert!(def_names.contains(&"NewUser"), "Should find func NewUser, got: {:?}", def_names);
        assert!(def_names.contains(&"DisplayName"), "Should find method DisplayName, got: {:?}", def_names);
    }

    #[test]
    fn test_extract_javascript_tags() {
        // JS files are parsed with the TSX grammar (TypeScript is a JS superset)
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.js");
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(f, "{}", r#"
class EventEmitter {
  on(event, callback) {
    this.listeners[event].push(callback);
  }

  emit(event, data) {
    this.listeners[event].forEach(cb => cb(data));
  }
}

function createEmitter() {
  return new EventEmitter();
}

const processEvent = (event) => {
  console.log(event);
};
"#).unwrap();

        let mut rm = RepoMap::new(dir.path().to_path_buf(), 4096);
        let tags = rm.extract_tags(&file_path).unwrap();
        
        let def_names: Vec<&str> = tags.iter()
            .filter(|t| t.kind == TagKind::Definition)
            .map(|t| t.name.as_str())
            .collect();
        
        assert!(def_names.contains(&"EventEmitter"), "Should find class EventEmitter, got: {:?}", def_names);
        assert!(def_names.contains(&"on"), "Should find method on, got: {:?}", def_names);
        assert!(def_names.contains(&"emit"), "Should find method emit, got: {:?}", def_names);
        assert!(def_names.contains(&"createEmitter"), "Should find fn createEmitter, got: {:?}", def_names);
        assert!(def_names.contains(&"processEvent"), "Should find const processEvent, got: {:?}", def_names);
    }
}
