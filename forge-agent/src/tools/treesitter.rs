//! Symbol extraction using regex patterns.
//!
//! Provides accurate symbol extraction for tools like
//! `list_definitions`, `get_definition`, and `find_references`.
//! Uses regex patterns for common languages.

use anyhow::Result;
use regex::Regex;

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

/// Parse a file and extract symbol definitions using regex patterns.
pub fn parse_definitions(content: &str, file_ext: &str) -> Result<Vec<Symbol>> {
    let lines: Vec<&str> = content.lines().collect();
    let mut symbols = Vec::new();

    let patterns = get_patterns(file_ext);
    if patterns.is_empty() {
        return Ok(Vec::new());
    }

    for (line_idx, line) in lines.iter().enumerate() {
        for (pattern, kind) in &patterns {
            if let Some(caps) = pattern.captures(line) {
                if let Some(name_match) = caps.name("name") {
                    let name = name_match.as_str().to_string();
                    if name.len() < 2 {
                        continue;
                    }

                    // Estimate end line (simple heuristic: look for closing brace)
                    let end_line = find_block_end(&lines, line_idx);

                    symbols.push(Symbol {
                        name,
                        kind: *kind,
                        start_line: line_idx + 1, // 1-indexed
                        end_line: end_line + 1,
                        signature: line.trim().to_string(),
                    });
                }
            }
        }
    }

    symbols.sort_by_key(|s| s.start_line);
    Ok(symbols)
}

/// Parse a file and extract references (calls, type usages, etc.).
pub fn parse_references(content: &str, file_ext: &str) -> Result<Vec<(String, usize)>> {
    let lines: Vec<&str> = content.lines().collect();
    let mut refs = Vec::new();

    // Simple pattern: identifier followed by ( for function calls
    let call_pattern = Regex::new(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(").unwrap();

    for (line_idx, line) in lines.iter().enumerate() {
        for caps in call_pattern.captures_iter(line) {
            if let Some(name_match) = caps.get(1) {
                let name = name_match.as_str().to_string();
                // Filter out common keywords
                if !is_keyword(&name, file_ext) && name.len() >= 2 {
                    refs.push((name, line_idx + 1));
                }
            }
        }
    }

    Ok(refs)
}

/// Find a symbol definition by name
pub fn find_definition(content: &str, file_ext: &str, symbol_name: &str) -> Option<Symbol> {
    let symbols = parse_definitions(content, file_ext).ok()?;
    symbols.into_iter().find(|s| s.name == symbol_name)
}

// ── Helper functions ─────────────────────────────────────────────

fn get_patterns(file_ext: &str) -> Vec<(Regex, SymbolKind)> {
    match file_ext {
        "rs" => vec![
            (Regex::new(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Function),
            (Regex::new(r"^\s*(?:pub\s+)?struct\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Struct),
            (Regex::new(r"^\s*(?:pub\s+)?enum\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Enum),
            (Regex::new(r"^\s*(?:pub\s+)?trait\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Interface),
            (Regex::new(r"^\s*(?:pub\s+)?type\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Type),
            (Regex::new(r"^\s*(?:pub\s+)?const\s+(?P<name>[A-Z_][A-Z0-9_]*)").unwrap(), SymbolKind::Constant),
            (Regex::new(r"^\s*(?:pub\s+)?mod\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Module),
            (Regex::new(r"^\s*impl(?:<[^>]*>)?\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Struct),
        ],
        "py" => vec![
            (Regex::new(r"^\s*(?:async\s+)?def\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Function),
            (Regex::new(r"^\s*class\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Class),
        ],
        "js" | "ts" | "tsx" | "jsx" => vec![
            (Regex::new(r"^\s*(?:export\s+)?(?:async\s+)?function\s+(?P<name>[a-zA-Z_$][a-zA-Z0-9_$]*)").unwrap(), SymbolKind::Function),
            (Regex::new(r"^\s*(?:export\s+)?class\s+(?P<name>[a-zA-Z_$][a-zA-Z0-9_$]*)").unwrap(), SymbolKind::Class),
            (Regex::new(r"^\s*(?:export\s+)?(?:const|let|var)\s+(?P<name>[a-zA-Z_$][a-zA-Z0-9_$]*)\s*=\s*(?:async\s+)?\(").unwrap(), SymbolKind::Function),
            (Regex::new(r"^\s*(?:export\s+)?interface\s+(?P<name>[a-zA-Z_$][a-zA-Z0-9_$]*)").unwrap(), SymbolKind::Interface),
            (Regex::new(r"^\s*(?:export\s+)?type\s+(?P<name>[a-zA-Z_$][a-zA-Z0-9_$]*)").unwrap(), SymbolKind::Type),
        ],
        "go" => vec![
            (Regex::new(r"^\s*func\s+(?:\([^)]+\)\s+)?(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Function),
            (Regex::new(r"^\s*type\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)\s+struct").unwrap(), SymbolKind::Struct),
            (Regex::new(r"^\s*type\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)\s+interface").unwrap(), SymbolKind::Interface),
        ],
        "java" | "kt" => vec![
            (Regex::new(r"^\s*(?:public\s+|private\s+|protected\s+)?(?:static\s+)?(?:final\s+)?(?:abstract\s+)?class\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Class),
            (Regex::new(r"^\s*(?:public\s+|private\s+|protected\s+)?(?:static\s+)?(?:final\s+)?(?:abstract\s+)?interface\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Interface),
            (Regex::new(r"^\s*(?:public\s+|private\s+|protected\s+)?(?:static\s+)?(?:final\s+)?(?:abstract\s+)?(?:synchronized\s+)?(?:[a-zA-Z_<>\[\],\s]+)\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)\s*\(").unwrap(), SymbolKind::Method),
        ],
        "c" | "cpp" | "h" | "hpp" => vec![
            (Regex::new(r"^\s*(?:static\s+)?(?:inline\s+)?(?:virtual\s+)?(?:[a-zA-Z_][a-zA-Z0-9_*&\s]+)\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)\s*\(").unwrap(), SymbolKind::Function),
            (Regex::new(r"^\s*(?:class|struct)\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Class),
            (Regex::new(r"^\s*enum\s+(?:class\s+)?(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)").unwrap(), SymbolKind::Enum),
        ],
        _ => vec![],
    }
}

fn find_block_end(lines: &[&str], start: usize) -> usize {
    let mut depth = 0;
    let mut found_open = false;

    for (i, line) in lines.iter().enumerate().skip(start) {
        for c in line.chars() {
            if c == '{' {
                depth += 1;
                found_open = true;
            } else if c == '}' {
                depth -= 1;
                if found_open && depth == 0 {
                    return i;
                }
            }
        }
    }

    // Fallback: return a reasonable end
    (start + 20).min(lines.len().saturating_sub(1))
}

fn is_keyword(name: &str, file_ext: &str) -> bool {
    let common = ["if", "else", "for", "while", "return", "match", "switch", "case"];
    if common.contains(&name) {
        return true;
    }

    match file_ext {
        "rs" => ["fn", "let", "mut", "pub", "use", "mod", "impl", "struct", "enum", "trait", "where", "async", "await", "loop", "break", "continue", "self", "Self", "super", "crate", "extern", "ref", "move", "dyn", "type", "const", "static", "unsafe", "as", "in"].contains(&name),
        "py" => ["def", "class", "import", "from", "as", "if", "elif", "else", "for", "while", "try", "except", "finally", "with", "lambda", "yield", "return", "pass", "break", "continue", "and", "or", "not", "in", "is", "None", "True", "False", "async", "await", "print"].contains(&name),
        "js" | "ts" | "tsx" | "jsx" => ["function", "const", "let", "var", "class", "import", "export", "from", "if", "else", "for", "while", "do", "switch", "case", "break", "continue", "return", "try", "catch", "finally", "throw", "new", "this", "super", "typeof", "instanceof", "void", "delete", "async", "await", "yield", "null", "undefined", "true", "false", "console", "require"].contains(&name),
        "go" => ["func", "package", "import", "type", "struct", "interface", "map", "chan", "go", "defer", "select", "case", "default", "if", "else", "for", "range", "switch", "break", "continue", "return", "fallthrough", "goto", "var", "const", "nil", "true", "false", "make", "new", "len", "cap", "append", "copy", "delete", "panic", "recover", "print", "println"].contains(&name),
        _ => false,
    }
}
