//! Proto SDK Manager Integration
//!
//! This module integrates with proto (https://moonrepo.dev/proto) to provide
//! multi-language SDK/toolchain management capabilities.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

/// Represents an installed tool managed by proto
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtoTool {
    pub name: String,
    pub version: String,
    pub path: Option<PathBuf>,
    pub is_default: bool,
}

/// Represents a tool available for installation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableTool {
    pub name: String,
    pub description: String,
    pub versions: Vec<String>,
}

/// Project-level tool configuration from .prototools
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProtoToolsConfig {
    pub tools: HashMap<String, String>,
}

/// Proto manager for SDK/toolchain operations
pub struct ProtoManager {
    workspace: Option<PathBuf>,
}

impl ProtoManager {
    pub fn new(workspace: Option<PathBuf>) -> Self {
        Self { workspace }
    }

    /// Check if proto is installed on the system
    pub fn is_proto_installed() -> bool {
        Command::new("proto")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get proto version
    pub fn get_proto_version() -> Result<String> {
        let output = Command::new("proto")
            .arg("--version")
            .output()
            .context("Failed to run proto --version")?;

        if !output.status.success() {
            return Err(anyhow!("proto --version failed"));
        }

        let version = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();
        Ok(version)
    }

    /// List all installed tools
    pub fn list_installed_tools(&self) -> Result<Vec<ProtoTool>> {
        let output = Command::new("proto")
            .arg("list")
            .arg("--json")
            .output()
            .context("Failed to run proto list")?;

        if !output.status.success() {
            // Try without --json for older versions
            return self.list_installed_tools_text();
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Parse JSON output
        // Proto outputs: { "tool_name": { "versions": [...], "default": "x.y.z" } }
        let parsed: HashMap<String, serde_json::Value> = 
            serde_json::from_str(&stdout).unwrap_or_default();

        let mut tools = Vec::new();
        for (name, info) in parsed {
            if let Some(obj) = info.as_object() {
                let default_version = obj.get("default")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                
                if let Some(versions) = obj.get("versions").and_then(|v| v.as_array()) {
                    for ver in versions {
                        if let Some(version) = ver.as_str() {
                            tools.push(ProtoTool {
                                name: name.clone(),
                                version: version.to_string(),
                                path: None,
                                is_default: version == default_version,
                            });
                        }
                    }
                }
            }
        }

        Ok(tools)
    }

    /// Fallback: parse text output from proto list
    fn list_installed_tools_text(&self) -> Result<Vec<ProtoTool>> {
        let output = Command::new("proto")
            .arg("list")
            .output()
            .context("Failed to run proto list")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut tools = Vec::new();

        // Parse text output line by line
        // Format: "tool_name - version (default)" or "tool_name - version"
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("No tools") {
                continue;
            }

            // Try to parse "name - version" format
            if let Some((name, rest)) = line.split_once(" - ") {
                let is_default = rest.contains("(default)");
                let version = rest.replace("(default)", "").trim().to_string();
                
                tools.push(ProtoTool {
                    name: name.trim().to_string(),
                    version,
                    path: None,
                    is_default,
                });
            }
        }

        Ok(tools)
    }

    /// Install a tool with a specific version
    pub fn install_tool(&self, tool: &str, version: &str) -> Result<String> {
        let mut cmd = Command::new("proto");
        cmd.arg("install").arg(tool);
        
        if !version.is_empty() && version != "latest" {
            cmd.arg(version);
        }

        // Set working directory if workspace is set
        if let Some(ref ws) = self.workspace {
            cmd.current_dir(ws);
        }

        let output = cmd.output()
            .context(format!("Failed to run proto install {} {}", tool, version))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(anyhow!("Failed to install {}: {}", tool, stderr));
        }

        Ok(format!("{}\n{}", stdout, stderr))
    }

    /// Uninstall a tool version
    pub fn uninstall_tool(&self, tool: &str, version: &str) -> Result<String> {
        let output = Command::new("proto")
            .arg("uninstall")
            .arg(tool)
            .arg(version)
            .output()
            .context(format!("Failed to run proto uninstall {} {}", tool, version))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to uninstall {}: {}", tool, stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get the binary path for a tool
    pub fn get_tool_bin_path(&self, tool: &str) -> Result<PathBuf> {
        let output = Command::new("proto")
            .arg("bin")
            .arg(tool)
            .output()
            .context(format!("Failed to run proto bin {}", tool))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to get bin path for {}: {}", tool, stderr));
        }

        let path = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();
        Ok(PathBuf::from(path))
    }

    /// Read .prototools configuration from workspace
    pub fn read_project_config(&self) -> Result<ProtoToolsConfig> {
        let workspace = self.workspace.as_ref()
            .ok_or_else(|| anyhow!("No workspace set"))?;

        let prototools_path = workspace.join(".prototools");
        
        if !prototools_path.exists() {
            return Ok(ProtoToolsConfig::default());
        }

        let content = std::fs::read_to_string(&prototools_path)
            .context("Failed to read .prototools")?;

        // Parse TOML - .prototools format:
        // [tools]
        // node = "20.10.0"
        // python = "3.11"
        let parsed: toml::Value = toml::from_str(&content)
            .context("Failed to parse .prototools")?;

        let mut config = ProtoToolsConfig::default();

        if let Some(tools) = parsed.get("tools").and_then(|t| t.as_table()) {
            for (name, version) in tools {
                if let Some(ver) = version.as_str() {
                    config.tools.insert(name.clone(), ver.to_string());
                }
            }
        }

        // Also check top-level tool definitions (alternative format)
        if let Some(table) = parsed.as_table() {
            for (key, value) in table {
                if key != "tools" && key != "plugins" && key != "settings" {
                    if let Some(ver) = value.as_str() {
                        config.tools.insert(key.clone(), ver.to_string());
                    }
                }
            }
        }

        Ok(config)
    }

    /// Create or update .prototools file
    pub fn write_project_config(&self, config: &ProtoToolsConfig) -> Result<()> {
        let workspace = self.workspace.as_ref()
            .ok_or_else(|| anyhow!("No workspace set"))?;

        let prototools_path = workspace.join(".prototools");

        let mut content = String::new();
        
        if !config.tools.is_empty() {
            for (name, version) in &config.tools {
                content.push_str(&format!("{} = \"{}\"\n", name, version));
            }
        }

        std::fs::write(&prototools_path, content)
            .context("Failed to write .prototools")?;

        Ok(())
    }

    /// Setup project tools (install all tools from .prototools)
    pub fn setup_project(&self) -> Result<String> {
        let mut cmd = Command::new("proto");
        cmd.arg("use");

        if let Some(ref ws) = self.workspace {
            cmd.current_dir(ws);
        }

        let output = cmd.output()
            .context("Failed to run proto use")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(anyhow!("Failed to setup project: {}", stderr));
        }

        Ok(format!("{}\n{}", stdout, stderr))
    }

    /// Detect which tools are needed based on project files
    pub fn detect_project_tools(workspace: &Path) -> Vec<(String, String)> {
        let mut tools = Vec::new();

        // Check for various project files and suggest tools
        if workspace.join("Cargo.toml").exists() {
            tools.push(("rust".to_string(), "stable".to_string()));
        }

        if workspace.join("package.json").exists() {
            tools.push(("node".to_string(), "lts".to_string()));
        }

        if workspace.join("requirements.txt").exists() 
            || workspace.join("pyproject.toml").exists()
            || workspace.join("setup.py").exists() 
        {
            tools.push(("python".to_string(), "latest".to_string()));
        }

        if workspace.join("go.mod").exists() {
            tools.push(("go".to_string(), "latest".to_string()));
        }

        if workspace.join("deno.json").exists() 
            || workspace.join("deno.jsonc").exists() 
        {
            tools.push(("deno".to_string(), "latest".to_string()));
        }

        if workspace.join("bun.lockb").exists() {
            tools.push(("bun".to_string(), "latest".to_string()));
        }

        tools
    }

    /// Search for available tool versions
    pub fn search_tool_versions(&self, tool: &str) -> Result<Vec<String>> {
        let output = Command::new("proto")
            .arg("list-remote")
            .arg(tool)
            .output()
            .context(format!("Failed to run proto list-remote {}", tool))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to list versions for {}: {}", tool, stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let versions: Vec<String> = stdout
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        Ok(versions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_project_tools() {
        // This would need a temp directory with project files
        // For now, just test that it doesn't panic
        let tools = ProtoManager::detect_project_tools(Path::new("/nonexistent"));
        assert!(tools.is_empty());
    }
}
