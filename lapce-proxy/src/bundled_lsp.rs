//! Resolve bundled language-server binaries shipped inside the app bundle.

use std::path::PathBuf;

/// Look for a language-server binary bundled next to the proxy executable
/// (e.g. `Forge-IDE.app/Contents/MacOS/pyright-langserver`).
pub fn bundled_lsp_binary(binary_name: &str) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let bundled = exe_dir.join(binary_name);
    if bundled.is_file() {
        return Some(bundled);
    }
    None
}

/// Search PATH for an executable with the given name.
pub fn path_lookup(binary_name: &str) -> Option<PathBuf> {
    let path_env = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolve the best available path for a native LSP binary.
/// Prefers bundled binaries, then PATH lookup.
pub fn resolve_lsp_binary(binary_name: &str) -> Option<PathBuf> {
    bundled_lsp_binary(binary_name).or_else(|| path_lookup(binary_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_path_joins_exe_dir() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let expected = dir.join("pyright-langserver");
                if expected.is_file() {
                    assert_eq!(
                        bundled_lsp_binary("pyright-langserver"),
                        Some(expected)
                    );
                }
            }
        }
    }
}
