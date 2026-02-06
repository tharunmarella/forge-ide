//! Run configuration auto-detection from project files
//! 
//! Detects run scripts from:
//! - package.json (npm/yarn/pnpm scripts)
//! - Cargo.toml (cargo commands)
//! - Makefile (make targets)
//! - pyproject.toml / setup.py (Python)
//! - go.mod (Go)

use std::path::Path;
use std::fs;
use std::collections::HashMap;

use lapce_rpc::proxy::DetectedRunConfig;
use serde::Deserialize;

/// Detect all run configurations from a workspace
pub fn detect_run_configs(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    
    // Detect from various project files
    configs.extend(detect_npm_scripts(workspace));
    configs.extend(detect_cargo_commands(workspace));
    configs.extend(detect_makefile_targets(workspace));
    configs.extend(detect_python_commands(workspace));
    configs.extend(detect_go_commands(workspace));
    
    configs
}

/// Detect npm/yarn/pnpm scripts from package.json
fn detect_npm_scripts(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    let package_json = workspace.join("package.json");
    
    if !package_json.exists() {
        return configs;
    }
    
    #[derive(Deserialize)]
    struct PackageJson {
        scripts: Option<HashMap<String, String>>,
    }
    
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(pkg) = serde_json::from_str::<PackageJson>(&content) {
            if let Some(scripts) = pkg.scripts {
                // Detect package manager
                let pm = detect_package_manager(workspace);
                
                for (name, _command) in scripts {
                    configs.push(DetectedRunConfig {
                        name: format!("{} {}", pm, name),
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

/// Detect which package manager is being used
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

/// Detect cargo commands from Cargo.toml
fn detect_cargo_commands(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    let cargo_toml = workspace.join("Cargo.toml");
    
    if !cargo_toml.exists() {
        return configs;
    }
    
    // Standard cargo commands
    let cargo_commands = [
        ("cargo run", vec!["run"]),
        ("cargo build", vec!["build"]),
        ("cargo test", vec!["test"]),
        ("cargo check", vec!["check"]),
        ("cargo build --release", vec!["build", "--release"]),
        ("cargo run --release", vec!["run", "--release"]),
    ];
    
    for (name, args) in cargo_commands {
        configs.push(DetectedRunConfig {
            name: name.to_string(),
            config_type: "cargo".to_string(),
            command: "cargo".to_string(),
            args: args.into_iter().map(String::from).collect(),
            cwd: Some(workspace.to_string_lossy().to_string()),
            source: "Cargo.toml".to_string(),
        });
    }
    
    // Detect binary targets
    #[derive(Deserialize)]
    struct CargoToml {
        bin: Option<Vec<BinTarget>>,
    }
    
    #[derive(Deserialize)]
    struct BinTarget {
        name: String,
    }
    
    if let Ok(content) = fs::read_to_string(&cargo_toml) {
        if let Ok(cargo) = toml::from_str::<CargoToml>(&content) {
            if let Some(bins) = cargo.bin {
                for bin in bins {
                    configs.push(DetectedRunConfig {
                        name: format!("cargo run --bin {}", bin.name),
                        config_type: "cargo".to_string(),
                        command: "cargo".to_string(),
                        args: vec!["run".to_string(), "--bin".to_string(), bin.name],
                        cwd: Some(workspace.to_string_lossy().to_string()),
                        source: "Cargo.toml".to_string(),
                    });
                }
            }
        }
    }
    
    configs
}

/// Detect Makefile targets
fn detect_makefile_targets(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    
    // Check for Makefile or makefile
    let makefile = if workspace.join("Makefile").exists() {
        workspace.join("Makefile")
    } else if workspace.join("makefile").exists() {
        workspace.join("makefile")
    } else {
        return configs;
    };
    
    if let Ok(content) = fs::read_to_string(&makefile) {
        // Parse Makefile targets (lines starting with target: or target :)
        for line in content.lines() {
            let line = line.trim();
            
            // Skip comments, empty lines, and .PHONY
            if line.is_empty() || line.starts_with('#') || line.starts_with('.') {
                continue;
            }
            
            // Match target definitions
            if let Some(colon_pos) = line.find(':') {
                let target = line[..colon_pos].trim();
                
                // Skip if target contains special characters or is a variable
                if target.is_empty() 
                    || target.contains('$') 
                    || target.contains('%')
                    || target.contains(' ')
                    || target.contains('\t')
                {
                    continue;
                }
                
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
    
    configs
}

/// Detect Python run commands
fn detect_python_commands(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    
    let has_python_project = workspace.join("pyproject.toml").exists()
        || workspace.join("setup.py").exists()
        || workspace.join("requirements.txt").exists();
    
    if !has_python_project {
        return configs;
    }
    
    // Check for common entry points
    if workspace.join("main.py").exists() {
        configs.push(DetectedRunConfig {
            name: "python main.py".to_string(),
            config_type: "python".to_string(),
            command: "python".to_string(),
            args: vec!["main.py".to_string()],
            cwd: Some(workspace.to_string_lossy().to_string()),
            source: "main.py".to_string(),
        });
    }
    
    if workspace.join("app.py").exists() {
        configs.push(DetectedRunConfig {
            name: "python app.py".to_string(),
            config_type: "python".to_string(),
            command: "python".to_string(),
            args: vec!["app.py".to_string()],
            cwd: Some(workspace.to_string_lossy().to_string()),
            source: "app.py".to_string(),
        });
    }
    
    // Django
    if workspace.join("manage.py").exists() {
        configs.push(DetectedRunConfig {
            name: "python manage.py runserver".to_string(),
            config_type: "python".to_string(),
            command: "python".to_string(),
            args: vec!["manage.py".to_string(), "runserver".to_string()],
            cwd: Some(workspace.to_string_lossy().to_string()),
            source: "manage.py".to_string(),
        });
    }
    
    // Pytest
    if workspace.join("pytest.ini").exists() 
        || workspace.join("pyproject.toml").exists()
        || workspace.join("tests").is_dir()
    {
        configs.push(DetectedRunConfig {
            name: "pytest".to_string(),
            config_type: "python".to_string(),
            command: "pytest".to_string(),
            args: vec![],
            cwd: Some(workspace.to_string_lossy().to_string()),
            source: "pytest".to_string(),
        });
    }
    
    configs
}

/// Detect Go commands
fn detect_go_commands(workspace: &Path) -> Vec<DetectedRunConfig> {
    let mut configs = Vec::new();
    
    if !workspace.join("go.mod").exists() {
        return configs;
    }
    
    let go_commands = [
        ("go run .", vec!["run", "."]),
        ("go build", vec!["build"]),
        ("go test", vec!["test", "./..."]),
        ("go test -v", vec!["test", "-v", "./..."]),
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
    
    configs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;
    
    #[test]
    fn test_detect_npm_scripts() {
        let dir = tempdir().unwrap();
        let package_json = dir.path().join("package.json");
        let mut file = File::create(&package_json).unwrap();
        writeln!(file, r#"{{"scripts": {{"start": "node index.js", "test": "jest"}}}}"#).unwrap();
        
        let configs = detect_npm_scripts(dir.path());
        assert_eq!(configs.len(), 2);
        assert!(configs.iter().any(|c| c.name == "npm start"));
        assert!(configs.iter().any(|c| c.name == "npm test"));
    }
    
    #[test]
    fn test_detect_cargo_commands() {
        let dir = tempdir().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        let mut file = File::create(&cargo_toml).unwrap();
        writeln!(file, r#"[package]
name = "test"
version = "0.1.0""#).unwrap();
        
        let configs = detect_cargo_commands(dir.path());
        assert!(configs.len() >= 4);
        assert!(configs.iter().any(|c| c.name == "cargo run"));
        assert!(configs.iter().any(|c| c.name == "cargo test"));
    }
}
