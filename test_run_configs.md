# Testing Run Configuration Tools

## New Agent Tools

The agent now has access to three new tools for managing project execution:

### 1. `list_run_configs()`
Lists all available run configurations detected from the project:
- npm/yarn/pnpm scripts from package.json
- Cargo workspace members and binary targets
- Python main modules
- Go main packages
- Maven/Gradle tasks
- VSCode launch.json configurations

**Example usage in chat:**
```
"List all available run configurations for this project"
```

### 2. `run_project(config_name, command, mode)`
Runs a project using the IDE's run system:
- Opens a proper terminal tab with UI
- Respects project environment setup
- Integrates with debug system
- Better output handling than `execute_command`

**Parameters:**
- `config_name` (optional): Name from `list_run_configs()` (e.g., "npm run dev")
- `command` (optional): Custom command if not using a detected config
- `mode`: "run" (default) or "debug"

**Example usage in chat:**
```
"Run the dev server"
"Run cargo build --release"
"Run npm test in debug mode"
```

### 3. `stop_project(config_name)`
Stops a running project/process started with `run_project()`.

**Parameters:**
- `config_name` (optional): Config to stop. If empty, stops the most recent one.

**Example usage in chat:**
```
"Stop the dev server"
"Stop the most recent process"
```

## How It Works

### Backend (forge-search)
- Python tools defined in `/forge-search/app/core/agent.py`
- Return `"PENDING_IDE_EXECUTION"` to indicate IDE should handle them
- Added to `IDE_TOOLS` list for agent access

### Agent (forge-agent)
- Rust implementation in `/forge-agent/src/tools/run_config.rs`
- Registered in `/forge-agent/src/tools/mod.rs`
- JSON schemas define tool parameters and descriptions

### RPC Layer (lapce-rpc)
- New requests: `AgentListRunConfigs`, `AgentRunProject`, `AgentStopProject`
- New responses: `AgentListRunConfigsResponse`, `AgentRunProjectResponse`, `AgentStopProjectResponse`
- Methods in `ProxyRpcHandler`

### Proxy (lapce-proxy)
- Handlers in `/lapce-proxy/src/dispatch.rs`
- Uses existing `run_config_detector` for detection
- Forwards execution requests to IDE for UI integration

## Test Cases

1. **List configs in a Node.js project:**
   - Open a project with package.json
   - Ask: "What can I run in this project?"
   - Expected: Lists npm scripts (dev, build, test, etc.)

2. **List configs in a Rust project:**
   - Open a Cargo workspace
   - Ask: "Show me available run configurations"
   - Expected: Lists cargo bins and workspace members

3. **Run a config:**
   - Ask: "Run the dev server"
   - Expected: Opens terminal, starts dev server with proper setup

4. **Run with debug mode:**
   - Ask: "Run tests in debug mode"
   - Expected: Starts with debugger enabled

5. **Stop a process:**
   - While process running, ask: "Stop the dev server"
   - Expected: Kills the process gracefully

## Benefits Over execute_command

1. ✅ **Better UI Integration** - Opens in proper terminal tabs
2. ✅ **Environment Setup** - Respects project's environment variables
3. ✅ **Debug Support** - Can enable breakpoints with mode="debug"
4. ✅ **Process Management** - Easy stop/restart controls
5. ✅ **Auto-detection** - Finds runnable targets automatically
6. ✅ **Working Directory** - Uses correct directory for each config

## Next Steps

The IDE needs to implement the final UI integration to:
- Actually execute the commands in terminal tabs
- Track running processes for stop functionality
- Integrate with the debug system when mode="debug"

For now, the RPC layer returns success with descriptive messages that the IDE can use.
