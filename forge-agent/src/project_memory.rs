//! Persistent project memory via FORGE.md files.
//!
//! Ported from gemini-cli's hierarchical GEMINI.md system. The agent "learns"
//! about projects by reading and writing FORGE.md files at 3 tiers:
//!
//! - **Global:** `~/.config/forge-ide/FORGE.md` -- user-wide preferences
//! - **Workspace:** `./FORGE.md` at the project root -- project conventions
//! - **Subdirectory (JIT):** `./subdir/FORGE.md` -- scope-specific overrides
//!
//! Precedence: Subdirectory > Workspace > Global (most specific wins).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The context filename used by forge-ide.
pub const CONTEXT_FILENAME: &str = "FORGE.md";

/// Section header for agent-saved memories in the global FORGE.md.
pub const MEMORY_SECTION_HEADER: &str = "## Forge Learned Memories";

/// Maximum characters to inject from a single FORGE.md file.
const MAX_MEMORY_CHARS: usize = 4000;

// ══════════════════════════════════════════════════════════════════
//  DISCOVERY & LOADING
// ══════════════════════════════════════════════════════════════════

/// Get the path to the global FORGE.md.
/// Uses `dirs::config_dir()` -> `~/.config/forge-ide/FORGE.md` on macOS/Linux,
/// `%APPDATA%/forge-ide/FORGE.md` on Windows.
pub fn global_memory_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("forge-ide").join(CONTEXT_FILENAME))
}

/// Load the global FORGE.md content (Tier 1).
///
/// Returns an empty string if the file doesn't exist.
pub fn load_global() -> String {
    let Some(path) = global_memory_path() else {
        return String::new();
    };
    read_and_cap(&path)
}

/// Load the workspace-root FORGE.md content (Tier 2).
///
/// Looks for `FORGE.md` in the workspace root directory.
pub fn load_workspace(workspace_root: &Path) -> String {
    let path = workspace_root.join(CONTEXT_FILENAME);
    read_and_cap(&path)
}

/// Discover and load a subdirectory FORGE.md (Tier 3 -- JIT).
///
/// Given a file path the agent is accessing, traverse upward from that file's
/// directory to the workspace root, looking for FORGE.md files that haven't
/// been loaded yet. Returns newly discovered (path, content) pairs.
///
/// This implements gemini-cli's `loadJitSubdirectoryMemory`: when the agent
/// reads/edits a file deep in the tree, we check if there's a FORGE.md in
/// any parent directory between the file and the workspace root.
pub fn discover_subdir_memory(
    accessed_path: &Path,
    workspace_root: &Path,
    already_loaded: &HashSet<PathBuf>,
) -> Vec<(PathBuf, String)> {
    let mut results = Vec::new();

    // Start from the directory containing the accessed file
    let start_dir = if accessed_path.is_file() {
        accessed_path.parent().unwrap_or(accessed_path)
    } else {
        accessed_path
    };

    // Don't search above the workspace root
    let root = match workspace_root.canonicalize() {
        Ok(r) => r,
        Err(_) => workspace_root.to_path_buf(),
    };

    let mut current = match start_dir.canonicalize() {
        Ok(c) => c,
        Err(_) => start_dir.to_path_buf(),
    };

    // Walk upward from the accessed path to the workspace root
    loop {
        // Don't include the workspace root itself (that's Tier 2)
        if current == root {
            break;
        }

        // Check if this directory is still within the workspace
        if !current.starts_with(&root) {
            break;
        }

        let forge_md = current.join(CONTEXT_FILENAME);
        if forge_md.exists() && !already_loaded.contains(&forge_md) {
            let content = read_and_cap(&forge_md);
            if !content.is_empty() {
                results.push((forge_md, content));
            }
        }

        // Move to parent
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }

    results
}

// ══════════════════════════════════════════════════════════════════
//  RENDERING (for prompt injection)
// ══════════════════════════════════════════════════════════════════

/// Render all loaded memory tiers into a single `<project_memory>` block.
///
/// Adapted from gemini-cli's `renderUserMemory()` in `snippets.ts`.
pub fn render_memory(
    global: &str,
    workspace: &str,
    subdirs: &[(PathBuf, String)],
    workspace_root: &Path,
) -> String {
    let has_any = !global.is_empty() || !workspace.is_empty() || !subdirs.is_empty();
    if !has_any {
        return String::new();
    }

    let mut out = String::with_capacity(2048);
    out.push_str("Contextual instructions loaded from FORGE.md files.\n");
    out.push_str("Precedence: Sub-directories > Workspace Root > Global.\n");
    out.push_str("These override default behaviors but NOT safety rules.\n\n");

    if !global.is_empty() {
        let label = global_memory_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "~/.config/forge-ide/FORGE.md".to_string());
        out.push_str(&format!("--- {} ---\n", label));
        out.push_str(global);
        if !global.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }

    if !workspace.is_empty() {
        out.push_str(&format!("--- {} ---\n", CONTEXT_FILENAME));
        out.push_str(workspace);
        if !workspace.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }

    for (path, content) in subdirs {
        let rel = path
            .parent()
            .and_then(|p| p.strip_prefix(workspace_root).ok())
            .map(|p| p.join(CONTEXT_FILENAME).display().to_string())
            .unwrap_or_else(|| path.display().to_string());
        out.push_str(&format!("--- {} ---\n", rel));
        out.push_str(content);
        if !content.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }

    out
}

// ══════════════════════════════════════════════════════════════════
//  SAVE MEMORY (for the SaveMemory tool)
// ══════════════════════════════════════════════════════════════════

/// Save a fact to the global FORGE.md under the "Forge Learned Memories" section.
///
/// Adapted from gemini-cli's `memoryTool.ts`:
/// - Creates the file and parent dirs if missing
/// - Finds or creates the `## Forge Learned Memories` section
/// - Appends the fact as a `- ` bullet point
/// - Sanitizes input (strips newlines, markdown injection)
pub fn save_memory(fact: &str) -> Result<String, String> {
    let path = global_memory_path()
        .ok_or_else(|| "Could not determine config directory".to_string())?;

    // Sanitize: collapse to single line, strip leading dashes
    let sanitized = fact
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string();
    let sanitized = sanitized
        .trim_start_matches(|c: char| c == '-' || c.is_whitespace())
        .trim()
        .to_string();

    if sanitized.is_empty() {
        return Err("Fact cannot be empty".to_string());
    }

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {e}"))?;
    }

    // Read existing content
    let current = std::fs::read_to_string(&path).unwrap_or_default();

    let new_item = format!("- {}", sanitized);

    let new_content = if let Some(header_pos) = current.find(MEMORY_SECTION_HEADER) {
        // Header exists -- append after the section
        let after_header = header_pos + MEMORY_SECTION_HEADER.len();

        // Find the end of this section (next ## heading or EOF)
        let section_end = current[after_header..]
            .find("\n## ")
            .map(|pos| after_header + pos)
            .unwrap_or(current.len());

        let mut result = current[..section_end].trim_end().to_string();
        result.push('\n');
        result.push_str(&new_item);
        result.push('\n');
        if section_end < current.len() {
            result.push_str(&current[section_end..]);
        }
        result
    } else {
        // No header -- append section at end
        let mut result = current.clone();
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        if !result.is_empty() && !result.ends_with("\n\n") {
            result.push('\n');
        }
        result.push_str(MEMORY_SECTION_HEADER);
        result.push('\n');
        result.push_str(&new_item);
        result.push('\n');
        result
    };

    std::fs::write(&path, &new_content)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;

    Ok(format!(
        "Remembered: \"{}\". Saved to {}",
        sanitized,
        path.display()
    ))
}

// ══════════════════════════════════════════════════════════════════
//  INIT PROMPT (for forge-cli --init)
// ══════════════════════════════════════════════════════════════════

/// The prompt sent to the agent when running `forge-cli --init`.
/// Instructs the agent to explore the project and generate a FORGE.md.
/// Adapted from gemini-cli's `init.ts`.
pub const INIT_PROMPT: &str = r#"You are an AI agent. Your task is to analyze the current project directory and generate a comprehensive FORGE.md file to be used as instructional context for future interactions.

**Analysis Process:**

1. **Initial Exploration:**
   - Start by listing the files and directories to get a high-level overview of the structure.
   - Read the README file (e.g., `README.md`, `README.txt`) if it exists. This is often the best place to start.

2. **Iterative Deep Dive (up to 10 files):**
   - Based on your initial findings, select a few files that seem most important (e.g., configuration files, main source files, documentation).
   - Read them. As you learn more, refine your understanding and decide which files to read next.

3. **Identify Project Type:**
   - **Code Project:** Look for `package.json`, `requirements.txt`, `pom.xml`, `go.mod`, `Cargo.toml`, `build.gradle`, or a `src` directory.
   - **Non-Code Project:** Documentation, research papers, notes, or other content.

**FORGE.md Content Generation:**

**For a Code Project:**

- **Project Overview:** Write a clear and concise summary of the project's purpose, main technologies, and architecture.
- **Building and Running:** Document the key commands for building, running, and testing the project. Infer these from the files you've read (e.g., `scripts` in `package.json`, `Makefile`, etc.).
- **Development Conventions:** Describe any coding styles, testing practices, or contribution guidelines you can infer from the codebase.
- **Architecture Notes:** Describe the high-level architecture, key modules, and how they interact.

**For a Non-Code Project:**

- **Directory Overview:** Describe the purpose and contents of the directory.
- **Key Files:** List the most important files and briefly explain what they contain.
- **Usage:** Explain how the contents of this directory are intended to be used.

**Final Output:**

Write the complete content to the `FORGE.md` file using write_to_file. The output must be well-formatted Markdown."#;

// ══════════════════════════════════════════════════════════════════
//  HELPERS
// ══════════════════════════════════════════════════════════════════

/// Read a file and cap its content at MAX_MEMORY_CHARS.
fn read_and_cap(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            if content.len() > MAX_MEMORY_CHARS {
                let truncated = &content[..MAX_MEMORY_CHARS];
                format!(
                    "{}\n\n[... truncated ({} total chars). Edit {} to reduce size.]",
                    truncated,
                    content.len(),
                    path.display(),
                )
            } else {
                content
            }
        }
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_workspace_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let content = load_workspace(tmp.path());
        assert!(content.is_empty());
    }

    #[test]
    fn test_load_workspace_present() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("FORGE.md"), "# My Project\nUse cargo build.").unwrap();
        let content = load_workspace(tmp.path());
        assert!(content.contains("My Project"));
        assert!(content.contains("cargo build"));
    }

    #[test]
    fn test_save_memory_creates_file() {
        // Use a temp dir to avoid writing to real config
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("FORGE.md");

        // Temporarily override by writing directly
        let sanitized = "User prefers tabs over spaces";
        let content = format!("{}\n- {}\n", MEMORY_SECTION_HEADER, sanitized);
        fs::write(&path, &content).unwrap();

        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains(MEMORY_SECTION_HEADER));
        assert!(result.contains("User prefers tabs over spaces"));
    }

    #[test]
    fn test_save_memory_sanitizes_newlines() {
        let fact = "line1\nline2\rline3";
        let sanitized = fact.replace(['\r', '\n'], " ").trim().to_string();
        assert_eq!(sanitized, "line1 line2 line3");
    }

    #[test]
    fn test_render_memory_empty() {
        let rendered = render_memory("", "", &[], Path::new("/tmp"));
        assert!(rendered.is_empty());
    }

    #[test]
    fn test_render_memory_global_only() {
        let rendered = render_memory("Use tabs.", "", &[], Path::new("/tmp"));
        assert!(rendered.contains("Use tabs."));
        assert!(rendered.contains("Precedence"));
    }

    #[test]
    fn test_discover_subdir_no_forge_md() {
        let tmp = tempfile::tempdir().unwrap();
        let subdir = tmp.path().join("src").join("lib");
        fs::create_dir_all(&subdir).unwrap();
        let file = subdir.join("main.rs");
        fs::write(&file, "fn main() {}").unwrap();

        let results = discover_subdir_memory(&file, tmp.path(), &HashSet::new());
        assert!(results.is_empty());
    }

    #[test]
    fn test_discover_subdir_finds_forge_md() {
        let tmp = tempfile::tempdir().unwrap();
        let subdir = tmp.path().join("packages").join("core");
        fs::create_dir_all(&subdir).unwrap();

        // Create a FORGE.md in the subdir
        fs::write(subdir.join("FORGE.md"), "# Core Package\nUse strict types.").unwrap();

        // Access a file deeper in
        let file = subdir.join("src").join("index.ts");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "export {}").unwrap();

        let results = discover_subdir_memory(&file, tmp.path(), &HashSet::new());
        assert_eq!(results.len(), 1);
        assert!(results[0].1.contains("Core Package"));
    }
}
