use mpatch;
use serde_json::json;
use std::path::PathBuf;
use tempfile::tempdir;
use forge_agent::tools::files::{replace, apply_patch}; // Make sure these are accessible, or I can test mpatch directly.

#[test]
fn test_mpatch_fuzzy_replace() {
    let original = "fn main() {\n    println!(\"Hello\");\n    let x = 5;\n}\n";
    
    // Simulating LLM fuzzy output with wrong indentation or slight mismatch
    let old_str = "fn main() {\n  println!(\"Hello\");\n  let x = 5;\n}";
    let new_str = "fn main() {\n  println!(\"Hello World\");\n  let x = 5;\n  let y = 10;\n}";
    
    let fuzzy_patch = format!(
        "```diff\n--- a/file\n+++ b/file\n@@ -1,1 +1,1 @@\n{}\n{}\n```",
        old_str.lines().map(|l| format!("-{}", l)).collect::<Vec<_>>().join("\n"),
        new_str.lines().map(|l| format!("+{}", l)).collect::<Vec<_>>().join("\n")
    );
    
    let options = mpatch::ApplyOptions::new().with_fuzz_factor(0.6);
    let result = mpatch::patch_content_str(&fuzzy_patch, Some(original), &options);
    
    assert!(result.is_ok(), "Fuzzy patch should apply successfully");
    let new_content = result.unwrap();
    assert!(new_content.contains("Hello World"));
    assert!(new_content.contains("let y = 10;"));
}

#[tokio::test]
async fn test_mpatch_unified_diff() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    
    let original = "fn main() {\n    println!(\"Hello\");\n    let x = 5;\n}\n";
    std::fs::write(&file_path, original).unwrap();
    
    // A slightly mangled unified diff
    let patch = "--- a/test.rs\n+++ b/test.rs\n@@ -1,4 +1,5 @@\n fn main() {\n-    println!(\"Hello\");\n+    println!(\"Hello from patch\");\n     let x = 5;\n+    let z = 100;\n }\n";
    
    let args = json!({
        "path": "test.rs",
        "patch": patch
    });
    
    let result = apply_patch(&args, dir.path()).await;
    assert!(result.success, "Apply patch failed: {}", result.output);
    
    let new_content = std::fs::read_to_string(&file_path).unwrap();
    assert!(new_content.contains("Hello from patch"));
    assert!(new_content.contains("let z = 100;"));
}
