# SDK Manager Tool Guide

## Overview

The `sdk_manager` tool provides the agent with access to the IDE's integrated SDK management system, powered by [Proto](https://moonrepo.dev/proto). This is the **recommended approach** for installing development tools instead of raw shell commands.

## Why Use SDK Manager Instead of Raw Commands?

✅ **Benefits of `sdk_manager`:**
- **No hallucinations**: Uses proven Proto installation methods
- **Cross-platform**: Works consistently on Windows, macOS, Linux
- **Version management**: Handles specific versions reliably
- **Project detection**: Automatically detects what tools a project needs
- **PATH management**: Automatically sets up binaries in PATH
- **Error handling**: Clear error messages, not cryptic shell failures
- **Consistent API**: Same interface regardless of platform

❌ **Problems with raw commands:**
- Platform-specific installation commands (apt, brew, winget, etc.)
- Risk of incorrect or outdated commands
- Manual PATH management required
- Inconsistent behavior across systems
- Complex error handling

## Available Operations

### 1. `detect_project` - Analyze Project Requirements
```json
{
  "operation": "detect_project"
}
```
Scans the current project directory for common files and suggests needed tools:
- `Cargo.toml` → Rust (stable)
- `package.json` → Node.js (lts)
- `requirements.txt`/`pyproject.toml` → Python (3.12)
- `go.mod` → Go (1.22)
- `deno.json` → Deno (latest)
- `bun.lockb` → Bun (latest)

### 2. `install` - Install a Tool
```json
{
  "operation": "install",
  "tool": "node",
  "version": "18.17.0",
  "pin": true
}
```
- `tool`: Tool name (node, python, rust, go, java, etc.)
- `version`: Specific version or "latest"
- `pin`: Whether to make the tool available in PATH (default: true)

### 3. `versions` - List Available Versions
```json
{
  "operation": "versions",
  "tool": "node"
}
```
Shows all available versions for a specific tool, including which ones are installed.

### 4. `list_installed` - Show Installed Tools
```json
{
  "operation": "list_installed"
}
```
Lists all currently installed tools with their versions and metadata.

### 5. `list_available` - Show Available Tools
```json
{
  "operation": "list_available"
}
```
Lists all available tools/plugins that can be installed.

### 6. `uninstall` - Remove a Tool Version
```json
{
  "operation": "uninstall",
  "tool": "node",
  "version": "16.0.0"
}
```

## Common Usage Patterns

### Installing Missing Dependencies
```
1. sdk_manager(operation="detect_project")  # See what's needed
2. sdk_manager(operation="install", tool="node", version="lts")  # Install LTS Node
3. sdk_manager(operation="install", tool="python", version="3.12")  # Install Python
```

### Checking Tool Availability
```
1. sdk_manager(operation="versions", tool="rust")  # See available Rust versions
2. sdk_manager(operation="install", tool="rust", version="stable")  # Install stable
```

### Managing Project Setup
```
1. sdk_manager(operation="detect_project")  # Auto-detect needs
2. sdk_manager(operation="list_installed")  # Check what's already there
3. Install missing tools as needed
```

## Supported Tools

Proto supports 800+ tools including:
- **Languages**: Node.js, Python, Rust, Go, Java, PHP, Ruby, Deno, Bun
- **Build tools**: Maven, Gradle, Cargo, npm, yarn, pnpm
- **Databases**: PostgreSQL, MySQL, Redis
- **And many more via the plugin ecosystem

## Proto Commands Reference

The SDK manager uses these Proto commands under the hood:
- `proto install <tool> [version] [--pin]` - Install tool
- `proto plugin list [--versions]` - List tools/versions  
- `proto versions <tool>` - List available versions
- `proto uninstall <tool> <version>` - Remove tool

## Best Practices

1. **Always detect first**: Use `detect_project` to understand project needs
2. **Use specific versions**: Prefer specific versions over "latest" for reproducibility
3. **Pin by default**: Keep `pin: true` to make tools available in PATH
4. **Check before installing**: Use `list_installed` to avoid duplicate installations
5. **Handle errors gracefully**: The tool provides clear error messages

## Example Agent Workflow

```
User: "I need to run this Node.js project but I don't have Node installed"

Agent:
1. sdk_manager(operation="detect_project")  
   → "📦 Node.js project detected - suggested tool: node (lts)"

2. sdk_manager(operation="versions", tool="node")  
   → Shows available versions including LTS

3. sdk_manager(operation="install", tool="node", version="lts", pin=true)  
   → "✅ Successfully installed node lts"

4. Now can run: run_project() or execute_command("npm install")
```

This approach is much more reliable than trying to guess the right installation commands for different platforms!