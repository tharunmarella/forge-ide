//! Symbol extraction using regex patterns.
//!
//! This is a fallback implementation that uses regex instead of tree-sitter.
//! TODO: Wire to IDE's tree-sitter infrastructure via the proxy bridge
//! for more accurate symbol extraction.

use anyhow::Result;

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

/// Parse a file and extract symbol definitions (regex-based)
pub fn parse_definitions(content: &str, file_ext: &str) -> Result<Vec<Symbol>> {
    let patterns: Vec<(&str, SymbolKind)> = match file_ext {
        "rs" => vec![
            (r"(?m)^[[:space:]]*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)", SymbolKind::Function),
            (r"(?m)^[[:space:]]*(?:pub\s+)?struct\s+(\w+)", SymbolKind::Struct),
            (r"(?m)^[[:space:]]*(?:pub\s+)?enum\s+(\w+)", SymbolKind::Enum),
            (r"(?m)^[[:space:]]*(?:pub\s+)?trait\s+(\w+)", SymbolKind::Interface),
            (r"(?m)^[[:space:]]*(?:pub\s+)?type\s+(\w+)", SymbolKind::Type),
            (r"(?m)^[[:space:]]*(?:pub\s+)?const\s+(\w+)", SymbolKind::Constant),
            (r"(?m)^[[:space:]]*(?:pub\s+)?mod\s+(\w+)", SymbolKind::Module),
            (r"(?m)^[[:space:]]*impl(?:<[^>]*>)?\s+(?:\w+\s+for\s+)?(\w+)", SymbolKind::Method),
        ],
        "py" => vec![
            (r"(?m)^[[:space:]]*(?:async\s+)?def\s+(\w+)", SymbolKind::Function),
            (r"(?m)^[[:space:]]*class\s+(\w+)", SymbolKind::Class),
        ],
        "js" | "jsx" => vec![
            (r"(?m)^[[:space:]]*(?:export\s+)?(?:async\s+)?function\s+(\w+)", SymbolKind::Function),
            (r"(?m)^[[:space:]]*(?:export\s+)?class\s+(\w+)", SymbolKind::Class),
            (r"(?m)^[[:space:]]*(?:export\s+)?const\s+(\w+)\s*=\s*(?:async\s*)?\(", SymbolKind::Function),
        ],
        "ts" | "tsx" => vec![
            (r"(?m)^[[:space:]]*(?:export\s+)?(?:async\s+)?function\s+(\w+)", SymbolKind::Function),
            (r"(?m)^[[:space:]]*(?:export\s+)?class\s+(\w+)", SymbolKind::Class),
            (r"(?m)^[[:space:]]*(?:export\s+)?interface\s+(\w+)", SymbolKind::Interface),
            (r"(?m)^[[:space:]]*(?:export\s+)?type\s+(\w+)", SymbolKind::Type),
            (r"(?m)^[[:space:]]*(?:export\s+)?const\s+(\w+)\s*=\s*(?:async\s*)?\(", SymbolKind::Function),
        ],
        "go" => vec![
            (r"(?m)^func\s+(?:\([^)]+\)\s+)?(\w+)\s*\(", SymbolKind::Function),
            (r"(?m)^type\s+(\w+)\s+struct", SymbolKind::Struct),
            (r"(?m)^type\s+(\w+)\s+interface", SymbolKind::Interface),
        ],
        _ => return Ok(Vec::new()),
    };

    let mut symbols = Vec::new();

    for (pattern, kind) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            for cap in re.captures_iter(content) {
                if let Some(name_match) = cap.get(1) {
                    let name = name_match.as_str().to_string();
                    let start_line = content[..name_match.start()].matches('\n').count() + 1;
                    // Estimate end line (look for next blank line or definition)
                    let end_line = start_line + 10; // rough estimate

                    // Get the signature (first line of the match)
                    let line_start = content[..cap.get(0).unwrap().start()]
                        .rfind('\n')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let line_end = content[cap.get(0).unwrap().start()..]
                        .find('\n')
                        .map(|p| cap.get(0).unwrap().start() + p)
                        .unwrap_or(content.len());
                    let signature = content[line_start..line_end].trim().to_string();

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
    }

    symbols.sort_by_key(|s| s.start_line);
    Ok(symbols)
}

/// Find a symbol definition by name
pub fn find_definition(content: &str, file_ext: &str, symbol_name: &str) -> Option<Symbol> {
    let symbols = parse_definitions(content, file_ext).ok()?;
    symbols.into_iter().find(|s| s.name == symbol_name)
}
