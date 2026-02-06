//! Run configuration auto-detection using multiple strategies
//!
//! Detection strategies (in order of priority):
//! 1. .vscode/tasks.json - VS Code tasks (widely used standard)
//! 2. .vscode/launch.json - VS Code debug configs
//! 3. Taskfile.yml / Taskfile.yaml - Task runner
//! 4. Justfile - Just command runner
//! 5. package.json - npm/yarn/pnpm/bun scripts
//! 6. Cargo.toml - Rust cargo commands
//! 7. Makefile - Make targets
//! 8. pyproject.toml - Python projects
//! 9. go.mod - Go projects

use std::path::Path;
use std::fs;
use std::process::Command;
use std::collections::HashMap;

use lapce_rpc::proxy::DetectedRunConfig;
use serde::Deserialize;

/// Detect all run configurations from a workspace using multiple strategies
pub fn detect_run_configs(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    
    // Priority 1: VS Code tasks (most reliable, user-defined)
    configs.extend(detect_vscode_tasks(workspace));
    
    // Priority 2: VS Code launch configs
    configs.extend(detect_vscode_launch(workspace));
    
    // Priority 3: Taskfile (modern task runner)
    configs.extend(detect_taskfile(workspace));
    
    // Priority 4: Justfile
    configs.extend(detect_justfile(workspace));
    
    // Priority 5: Package.json scripts
    configs.extend(detect_npm_scripts(workspace));
    
    // Priority 6: Cargo commands
    configs.extend(detect_cargo_commands(workspace));
    
    // Priority 7: Makefile targets
    configs.extend(detect_makefile_targets(workspace));
    
    // Priority 8: Python projects
    configs.extend(detect_python_commands(workspace));
    
    // Priority 9: Go projects
    configs.extend(detect_go_commands(workspace));
    
    configs
}

// ============================================================================
// VS Code Tasks (.vscode/tasks.json)
// ============================================================================

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct VsCodeTasks {
    version: Option<String>,
    tasks: Option<Vec<VsCodeTask>>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct VsCodeTask {
    label: String,
    #[serde(rename = "type")]
    task_type: Option<String>,
    command: Option<String>,
    args: Option<Vec<serde_json::Value>>,
    #[serde(rename = "problemMatcher")]
    problem_matcher: Option<serde_json::Value>,
    group: Option<serde_json::Value>,
    options: Option<VsCodeTaskOptions>,
}

#[derive(Deserialize, Debug)]
struct VsCodeTaskOptions {
    cwd: Option<String>,
}

fn detect_vscode_tasks(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    let tasks_json = workspace.join(".vscode").join("tasks.json");
    
    if !tasks_json.exists() {
        return configs;
    }
    
    tracing::info!("Detecting VS Code tasks from {:?}", tasks_json);
    
    if let Ok(content) = fs::read_to_string(&tasks_json) {
        // Strip comments from JSON (VS Code allows comments)
        let content = strip_json_comments(&content);
        
        if let Ok(tasks) = serde_json::from_str::<VsCodeTasks>(&content) {
            if let Some(task_list) = tasks.tasks {
                for task in task_list {
                    if let Some(command) = &task.command {
                        let args: Vec<String> = task.args
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|v| match v {
                                serde_json::Value::String(s) => Some(s.clone()),
                                _ => Some(v.to_string()),
                            })
                            .collect();
                        
                        let cwd = task.options
                            .and_then(|o| o.cwd)
                            .or_else(|| Some(workspace.to_string_lossy().to_string()));
                        
                        configs.push(DetectedRunConfig {
                            name: task.label,
                            config_type: task.task_type.unwrap_or_else(|| "shell".to_string()),
                            command: command.clone(),
                            args,
                            cwd,
                            source: ".vscode/tasks.json".to_string(),
                        });
                    }
                }
            }
        } else {
            tracing::warn!("Failed to parse tasks.json");
        }
    }
    
    configs
}

// ============================================================================
// VS Code Launch (.vscode/launch.json)
// ============================================================================

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct VsCodeLaunch {
    version: Option<String>,
    configurations: Option<Vec<VsCodeLaunchConfig>>,
}

#[derive(Deserialize, Debug)]
struct VsCodeLaunchConfig {
    name: String,
    #[serde(rename = "type")]
    config_type: Option<String>,
    request: Option<String>,
    program: Option<String>,
    args: Option<Vec<String>>,
    cwd: Option<String>,
}

fn detect_vscode_launch(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    let launch_json = workspace.join(".vscode").join("launch.json");
    
    if !launch_json.exists() {
        return configs;
    }
    
    tracing::info!("Detecting VS Code launch configs from {:?}", launch_json);
    
    if let Ok(content) = fs::read_to_string(&launch_json) {
        let content = strip_json_comments(&content);
        
        if let Ok(launch) = serde_json::from_str::<VsCodeLaunch>(&content) {
            if let Some(config_list) = launch.configurations {
                for config in config_list {
                    if let Some(program) = config.program {
                        configs.push(DetectedRunConfig {
                            name: format!("[Debug] {}", config.name),
                            config_type: config.config_type.unwrap_or_else(|| "debug".to_string()),
                            command: program,
                            args: config.args.unwrap_or_default(),
                            cwd: config.cwd.or_else(|| Some(workspace.to_string_lossy().to_string())),
                            source: ".vscode/launch.json".to_string(),
                        });
                    }
                }
            }
        }
    }
    
    configs
}

// ============================================================================
// Taskfile (https://taskfile.dev)
// ============================================================================

fn detect_taskfile(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    
    // Check for Taskfile.yml or Taskfile.yaml
    let taskfile = if workspace.join("Taskfile.yml").exists() {
        workspace.join("Taskfile.yml")
    } else if workspace.join("Taskfile.yaml").exists() {
        workspace.join("Taskfile.yaml")
    } else {
        return configs;
    };
    
    tracing::info!("Detecting Taskfile tasks from {:?}", taskfile);
    
    // Try using `task --list` command first (more reliable)
    if let Ok(output) = Command::new("task")
        .args(["--list-all"])
        .current_dir(workspace)
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                // Parse lines like "* task-name: description"
                if let Some(line) = line.strip_prefix("* ") {
                    let parts: Vec<&str> = line.splitn(2, ':').collect();
                    let name = parts[0].trim();
                    if !name.is_empty() {
                        configs.push(DetectedRunConfig {
                            name: format!("task {}", name),
                            config_type: "task".to_string(),
                            command: "task".to_string(),
                            args: vec![name.to_string()],
                            cwd: Some(workspace.to_string_lossy().to_string()),
                            source: "Taskfile.yml".to_string(),
                        });
                    }
                }
            }
            return configs;
        }
    }
    
    // Fallback: Parse YAML directly (requires serde_yaml, skip if not available)
    configs
}

// ============================================================================
// Justfile (https://github.com/casey/just)
// ============================================================================

fn detect_justfile(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    let justfile = workspace.join("Justfile");
    
    if !justfile.exists() && !workspace.join("justfile").exists() {
        return configs;
    }
    
    tracing::info!("Detecting Just recipes from {:?}", justfile);
    
    // Try using `just --list` command
    if let Ok(output) = Command::new("just")
        .args(["--list", "--unsorted"])
        .current_dir(workspace)
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(1) { // Skip header
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    // Parse lines like "recipe-name # description"
                    let parts: Vec<&str> = line.splitn(2, '#').collect();
                    let name = parts[0].trim();
                    if !name.is_empty() {
                        configs.push(DetectedRunConfig {
                            name: format!("just {}", name),
                            config_type: "just".to_string(),
                            command: "just".to_string(),
                            args: vec![name.to_string()],
                            cwd: Some(workspace.to_string_lossy().to_string()),
                            source: "Justfile".to_string(),
                        });
                    }
                }
            }
            return configs;
        }
    }
    
    // Fallback: Parse Justfile manually
    let justfile_path = if justfile.exists() { justfile } else { workspace.join("justfile") };
    if let Ok(content) = fs::read_to_string(&justfile_path) {
        for line in content.lines() {
            let line = line.trim();
            // Match recipe definitions (lines ending with :)
            if !line.is_empty() 
                && !line.starts_with('#') 
                && !line.starts_with(' ') 
                && !line.starts_with('\t')
                && line.contains(':')
            {
                let name = line.split(':').next().unwrap_or("").split_whitespace().next().unwrap_or("");
                if !name.is_empty() && !name.starts_with('@') && !name.starts_with('[') {
                    configs.push(DetectedRunConfig {
                        name: format!("just {}", name),
                        config_type: "just".to_string(),
                        command: "just".to_string(),
                        args: vec![name.to_string()],
                        cwd: Some(workspace.to_string_lossy().to_string()),
                        source: "Justfile".to_string(),
                    });
                }
            }
        }
    }
    
    configs
}

// ============================================================================
// NPM/Yarn/PNPM Scripts (package.json)
// ============================================================================

fn detect_npm_scripts(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    let package_json = workspace.join("package.json");
    
    if !package_json.exists() {
        return configs;
    }
    
    tracing::info!("Detecting npm scripts from {:?}", package_json);
    
    // Try using npm/yarn/pnpm CLI first (handles workspaces correctly)
    let pm = detect_package_manager(workspace);
    
    // Note: Could use `npm run --json` for more reliable parsing
    // but direct package.json parsing works well enough
    
    // Fallback: Parse package.json directly
    #[derive(Deserialize)]
    struct PackageJson {
        scripts: Option<HashMap<String, String>>,
    }
    
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(pkg) = serde_json::from_str::<PackageJson>(&content) {
            if let Some(scripts) = pkg.scripts {
                for (name, _command) in scripts {
                    // Skip lifecycle scripts
                    if name.starts_with("pre") || name.starts_with("post") {
                        continue;
                    }
                    
                    configs.push(DetectedRunConfig {
                        name: format!("{} run {}", pm, name),
                        config_type: "npm".to_string(),
                        command: pm.to_string(),
                        args: vec!["run".to_string(), name],
                        cwd: Some(workspace.to_string_lossy().to_string()),
                        source: "package.json".to_string(),
                    });
                }
            }
        }
    }
    
    configs
}

fn detect_package_manager(workspace: &Path) -> &'static str {
    if workspace.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if workspace.join("yarn.lock").exists() {
        "yarn"
    } else if workspace.join("bun.lockb").exists() {
        "bun"
    } else {
        "npm"
    }
}

// ============================================================================
// Cargo Commands (Cargo.toml)
// ============================================================================

fn detect_cargo_commands(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    let cargo_toml = workspace.join("Cargo.toml");
    
    if !cargo_toml.exists() {
        return configs;
    }
    
    tracing::info!("Detecting cargo commands from {:?}", cargo_toml);
    
    // Standard cargo commands
    let standard_commands = [
        ("cargo run", vec!["run"], "Run the project"),
        ("cargo build", vec!["build"], "Build the project"),
        ("cargo test", vec!["test"], "Run tests"),
        ("cargo check", vec!["check"], "Check for errors"),
        ("cargo build --release", vec!["build", "--release"], "Build release"),
        ("cargo run --release", vec!["run", "--release"], "Run release"),
    ];
    
    for (name, args, _desc) in standard_commands {
        configs.push(DetectedRunConfig {
            name: name.to_string(),
            config_type: "cargo".to_string(),
            command: "cargo".to_string(),
            args: args.into_iter().map(String::from).collect(),
            cwd: Some(workspace.to_string_lossy().to_string()),
            source: "Cargo.toml".to_string(),
        });
    }
    
    // Try to detect binary targets using cargo metadata
    if let Ok(output) = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(workspace)
        .output()
    {
        if output.status.success() {
            #[derive(Deserialize)]
            struct CargoMetadata {
                packages: Vec<CargoPackage>,
            }
            
            #[derive(Deserialize)]
            struct CargoPackage {
                targets: Vec<CargoTarget>,
            }
            
            #[derive(Deserialize)]
            struct CargoTarget {
                name: String,
                kind: Vec<String>,
            }
            
            if let Ok(metadata) = serde_json::from_slice::<CargoMetadata>(&output.stdout) {
                for package in metadata.packages {
                    for target in package.targets {
                        if target.kind.contains(&"bin".to_string()) && target.name != "lapce" {
                            configs.push(DetectedRunConfig {
                                name: format!("cargo run --bin {}", target.name),
                                config_type: "cargo".to_string(),
                                command: "cargo".to_string(),
                                args: vec!["run".to_string(), "--bin".to_string(), target.name],
                                cwd: Some(workspace.to_string_lossy().to_string()),
                                source: "Cargo.toml".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    
    configs
}

// ============================================================================
// Makefile Targets
// ============================================================================

fn detect_makefile_targets(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    
    let makefile = if workspace.join("Makefile").exists() {
        workspace.join("Makefile")
    } else if workspace.join("makefile").exists() {
        workspace.join("makefile")
    } else if workspace.join("GNUmakefile").exists() {
        workspace.join("GNUmakefile")
    } else {
        return configs;
    };
    
    tracing::info!("Detecting make targets from {:?}", makefile);
    
    // Note: Could use `make -pRrq :` for more reliable target listing
    // but parsing the output is complex, so we use direct file parsing
    
    // Fallback: Parse Makefile directly
    if let Ok(content) = fs::read_to_string(&makefile) {
        let mut seen = std::collections::HashSet::new();
        
        for line in content.lines() {
            let line = line.trim();
            
            // Skip comments, empty lines, and variable assignments
            if line.is_empty() || line.starts_with('#') || line.contains('=') {
                continue;
            }
            
            // Match target definitions
            if let Some(target) = line.strip_suffix(':') {
                let target = target.split_whitespace().next().unwrap_or("");
                
                // Skip internal targets and patterns
                if !target.is_empty() 
                    && !target.starts_with('.') 
                    && !target.starts_with('%')
                    && !target.contains('%')
                    && !seen.contains(target)
                {
                    seen.insert(target.to_string());
                    configs.push(DetectedRunConfig {
                        name: format!("make {}", target),
                        config_type: "make".to_string(),
                        command: "make".to_string(),
                        args: vec![target.to_string()],
                        cwd: Some(workspace.to_string_lossy().to_string()),
                        source: "Makefile".to_string(),
                    });
                }
            } else if line.contains(':') && !line.contains('\t') {
                // Handle "target: dependencies" format
                let target = line.split(':').next().unwrap_or("");
                let target = target.split_whitespace().next().unwrap_or("");
                
                if !target.is_empty() 
                    && !target.starts_with('.') 
                    && !target.starts_with('%')
                    && !target.contains('%')
                    && !seen.contains(target)
                {
                    seen.insert(target.to_string());
                    configs.push(DetectedRunConfig {
                        name: format!("make {}", target),
                        config_type: "make".to_string(),
                        command: "make".to_string(),
                        args: vec![target.to_string()],
                        cwd: Some(workspace.to_string_lossy().to_string()),
                        source: "Makefile".to_string(),
                    });
                }
            }
        }
    }
    
    configs
}

// ============================================================================
// Python Projects (pyproject.toml, setup.py)
// ============================================================================

fn detect_python_commands(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    
    let has_pyproject = workspace.join("pyproject.toml").exists();
    let has_setup_py = workspace.join("setup.py").exists();
    let has_requirements = workspace.join("requirements.txt").exists();
    
    if !has_pyproject && !has_setup_py && !has_requirements {
        return configs;
    }
    
    tracing::info!("Detecting Python commands");
    
    // Detect Python/pip commands
    let python = if workspace.join(".venv").exists() || workspace.join("venv").exists() {
        "python"
    } else {
        "python3"
    };
    
    // Common Python commands
    if has_pyproject || has_setup_py {
        configs.push(DetectedRunConfig {
            name: format!("{} -m pytest", python),
            config_type: "python".to_string(),
            command: python.to_string(),
            args: vec!["-m".to_string(), "pytest".to_string()],
            cwd: Some(workspace.to_string_lossy().to_string()),
            source: if has_pyproject { "pyproject.toml" } else { "setup.py" }.to_string(),
        });
    }
    
    // Check for scripts in pyproject.toml
    if has_pyproject {
        #[derive(Deserialize)]
        struct PyProject {
            tool: Option<PyProjectTool>,
            project: Option<PyProjectProject>,
        }
        
        #[derive(Deserialize)]
        struct PyProjectTool {
            poetry: Option<PoetryConfig>,
        }
        
        #[derive(Deserialize)]
        struct PoetryConfig {
            scripts: Option<HashMap<String, String>>,
        }
        
        #[derive(Deserialize)]
        struct PyProjectProject {
            scripts: Option<HashMap<String, String>>,
        }
        
        if let Ok(content) = fs::read_to_string(workspace.join("pyproject.toml")) {
            if let Ok(pyproject) = toml::from_str::<PyProject>(&content) {
                // Poetry scripts
                if let Some(tool) = pyproject.tool {
                    if let Some(poetry) = tool.poetry {
                        if let Some(scripts) = poetry.scripts {
                            for (name, _) in scripts {
                                configs.push(DetectedRunConfig {
                                    name: format!("poetry run {}", name),
                                    config_type: "python".to_string(),
                                    command: "poetry".to_string(),
                                    args: vec!["run".to_string(), name],
                                    cwd: Some(workspace.to_string_lossy().to_string()),
                                    source: "pyproject.toml".to_string(),
                                });
                            }
                        }
                    }
                }
                
                // PEP 621 scripts
                if let Some(project) = pyproject.project {
                    if let Some(scripts) = project.scripts {
                        for (name, _) in scripts {
                            configs.push(DetectedRunConfig {
                                name: name.clone(),
                                config_type: "python".to_string(),
                                command: name,
                                args: vec![],
                                cwd: Some(workspace.to_string_lossy().to_string()),
                                source: "pyproject.toml".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    
    configs
}

// ============================================================================
// Go Projects (go.mod)
// ============================================================================

fn detect_go_commands(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    let go_mod = workspace.join("go.mod");
    
    if !go_mod.exists() {
        return configs;
    }
    
    tracing::info!("Detecting Go commands from {:?}", go_mod);
    
    // Standard Go commands
    let go_commands = [
        ("go run .", vec!["run", "."]),
        ("go build", vec!["build"]),
        ("go test ./...", vec!["test", "./..."]),
        ("go test -v ./...", vec!["test", "-v", "./..."]),
    ];
    
    for (name, args) in go_commands {
        configs.push(DetectedRunConfig {
            name: name.to_string(),
            config_type: "go".to_string(),
            command: "go".to_string(),
            args: args.into_iter().map(String::from).collect(),
            cwd: Some(workspace.to_string_lossy().to_string()),
            source: "go.mod".to_string(),
        });
    }
    
    // Detect main packages
    if let Ok(output) = Command::new("go")
        .args(["list", "-f", "{{.Name}} {{.Dir}}", "./..."])
        .current_dir(workspace)
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && parts[0] == "main" {
                    let dir = parts[1];
                    let rel_dir = dir.strip_prefix(&workspace.to_string_lossy().to_string())
                        .unwrap_or(dir)
                        .trim_start_matches('/');
                    
                    if !rel_dir.is_empty() && rel_dir != "." {
                        configs.push(DetectedRunConfig {
                            name: format!("go run ./{}", rel_dir),
                            config_type: "go".to_string(),
                            command: "go".to_string(),
                            args: vec!["run".to_string(), format!("./{}", rel_dir)],
                            cwd: Some(workspace.to_string_lossy().to_string()),
                            source: "go.mod".to_string(),
                        });
                    }
                }
            }
        }
    }
    
    configs
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Strip comments from JSON (VS Code allows // and /* */ comments)
fn strip_json_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escape_next = false;
    
    while let Some(c) = chars.next() {
        if escape_next {
            result.push(c);
            escape_next = false;
            continue;
        }
        
        if c == '\\' && in_string {
            result.push(c);
            escape_next = true;
            continue;
        }
        
        if c == '"' {
            in_string = !in_string;
            result.push(c);
            continue;
        }
        
        if !in_string && c == '/' {
            if let Some(&next) = chars.peek() {
                if next == '/' {
                    // Single line comment - skip until newline
                    chars.next();
                    while let Some(nc) = chars.next() {
                        if nc == '\n' {
                            result.push('\n');
                            break;
                        }
                    }
                    continue;
                } else if next == '*' {
                    // Block comment - skip until */
                    chars.next();
                    while let Some(nc) = chars.next() {
                        if nc == '*' {
                            if let Some(&'/' ) = chars.peek() {
                                chars.next();
                                break;
                            }
                        }
                    }
                    continue;
                }
            }
        }
        
        result.push(c);
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_strip_json_comments() {
        let input = r#"{
            // This is a comment
            "key": "value", // inline comment
            /* block
               comment */
            "key2": "value2"
        }"#;
        
        let result = strip_json_comments(input);
        assert!(!result.contains("//"));
        assert!(!result.contains("/*"));
        assert!(result.contains("\"key\""));
    }
}
