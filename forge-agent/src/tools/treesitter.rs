//! Symbol extraction using tree-sitter-tags.
//!
//! Provides accurate AST-based symbol extraction for tools like
//! `list_definitions`, `get_definition`, and `find_references`.
//! Uses the same tree-sitter-tags infrastructure as the RepoMap.

use anyhow::Result;
use crate::repomap;

/// Code symbol definition
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SymbolKind {
    Function,
    Class,
    Struct,
    Enum,
    Interface,
    Type,
    Constant,
    #[allow(dead_code)]
    Variable,
    Method,
    Module,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Function => write!(f, "fn"),
            Self::Class => write!(f, "class"),
            Self::Struct => write!(f, "struct"),
            Self::Enum => write!(f, "enum"),
            Self::Interface => write!(f, "interface"),
            Self::Type => write!(f, "type"),
            Self::Constant => write!(f, "const"),
            Self::Variable => write!(f, "var"),
            Self::Method => write!(f, "method"),
            Self::Module => write!(f, "mod"),
        }
    }
}

/// Parse a file and extract symbol definitions using tree-sitter-tags.
///
/// This uses the same tree-sitter infrastructure as the RepoMap for
/// accurate, scope-aware symbol extraction.
pub fn parse_definitions(content: &str, file_ext: &str) -> Result<Vec<Symbol>> {
    let configs = repomap::lang_configs();
    let lang_cfg = match configs.get(file_ext) {
        Some(cfg) => cfg,
        None => return Ok(Vec::new()), // unsupported language
    };

    if content.is_empty() {
        return Ok(Vec::new());
    }

    let source_bytes = content.as_bytes();
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let mut context = tree_sitter_tags::TagsContext::new();

    let mut symbols = Vec::new();

    match context.generate_tags(lang_cfg.config(), source_bytes, None) {
        Ok((tag_iter, _)) => {
            for tag_result in tag_iter {
                if let Ok(tag) = tag_result {
                    if !tag.is_definition {
                        continue;
                    }

                    let name = match std::str::from_utf8(&source_bytes[tag.name_range.clone()]) {
                        Ok(n) => n.to_string(),
                        Err(_) => continue,
                    };

                    if name.len() < 2 {
                        continue;
                    }

                    let start_line = tag.span.start.row + 1; // 1-indexed
                    // Use the tag's span end row for end_line (accurate block boundary)
                    let end_line = (tag.span.end.row + 1).min(total_lines);

                    let signature = lines
                        .get(tag.span.start.row)
                        .map(|l| l.trim().to_string())
                        .unwrap_or_default();

                    // Map tree-sitter syntax type to our SymbolKind
                    let type_name = lang_cfg.config().syntax_type_name(tag.syntax_type_id as u32);
                    let kind = match type_name {
                        "function" => SymbolKind::Function,
                        "method" => SymbolKind::Method,
                        "class" => SymbolKind::Class,
                        "interface" => SymbolKind::Interface,
                        "module" => SymbolKind::Module,
                        "macro" => SymbolKind::Function,
                        _ => SymbolKind::Function,
                    };

                    symbols.push(Symbol {
                        name,
                        kind,
                        start_line,
                        end_line,
                        signature,
                    });
                }
            }
        }
        Err(_) => return Ok(Vec::new()),
    }

    symbols.sort_by_key(|s| s.start_line);
    Ok(symbols)
}

/// Parse a file and extract references (calls, type usages, etc.) using tree-sitter-tags.
pub fn parse_references(content: &str, file_ext: &str) -> Result<Vec<(String, usize)>> {
    let configs = repomap::lang_configs();
    let lang_cfg = match configs.get(file_ext) {
        Some(cfg) => cfg,
        None => return Ok(Vec::new()),
    };

    if content.is_empty() {
        return Ok(Vec::new());
    }

    let source_bytes = content.as_bytes();
    let mut context = tree_sitter_tags::TagsContext::new();
    let mut refs = Vec::new();

    match context.generate_tags(lang_cfg.config(), source_bytes, None) {
        Ok((tag_iter, _)) => {
            for tag_result in tag_iter {
                if let Ok(tag) = tag_result {
                    if tag.is_definition {
                        continue;
                    }
                    let name = match std::str::from_utf8(&source_bytes[tag.name_range.clone()]) {
                        Ok(n) => n.to_string(),
                        Err(_) => continue,
                    };
                    if name.len() < 2 {
                        continue;
                    }
                    refs.push((name, tag.span.start.row + 1));
                }
            }
        }
        Err(_) => {}
    }

    Ok(refs)
}

/// Find a symbol definition by name
pub fn find_definition(content: &str, file_ext: &str, symbol_name: &str) -> Option<Symbol> {
    let symbols = parse_definitions(content, file_ext).ok()?;
    symbols.into_iter().find(|s| s.name == symbol_name)
}
