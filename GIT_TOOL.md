# Git Tool for Agent

## Overview

Added a single unified `git` tool that exposes essential git operations to the agent, integrating with the IDE's native git system.

## Design Philosophy

Instead of exposing all 40+ git operations, we created **one tool with 9 essential operations** that cover 95% of common use cases:

1. **status** - Check repo status
2. **stage** - Stage files for commit
3. **unstage** - Unstage files  
4. **commit** - Commit with message
5. **push** - Push to remote
6. **pull** - Pull from remote
7. **branch** - List/create/switch branches
8. **log** - View commit history
9. **diff** - View file changes

## Benefits Over `execute_command`

✅ **IDE Integration** - Updates source control panel automatically  
✅ **Structured Responses** - Returns parsed git output  
✅ **Error Handling** - Better error messages and validation  
✅ **Type Safety** - Parameters are validated  
✅ **UI Updates** - Changes reflect in IDE immediately  

## Implementation

### Files Created/Modified

1. **forge-agent/src/tools/git.rs** - Rust tool implementation
2. **forge-agent/src/tools/mod.rs** - Tool registration and JSON schema
3. **forge-search/app/core/agent.py** - Python tool definition with examples

### Tool Schema

```json
{
  "name": "git",
  "parameters": {
    "operation": "status|stage|unstage|commit|push|pull|branch|log|diff",
    "paths": ["file1.rs", "file2.rs"],  // for stage/unstage
    "message": "commit message",         // for commit
    "action": "list|create|switch",      // for branch
    "name": "branch-name",               // for branch create/switch
    "limit": 10,                         // for log
    "path": "file.rs",                   // for diff
    "staged": false                      // for diff
  }
}
```

## Usage Examples

The agent can now use git naturally:

```python
# Check status
git(operation="status")

# Stage files
git(operation="stage", paths=["src/main.rs", "Cargo.toml"])

# Commit
git(operation="commit", message="Add new feature")

# Push
git(operation="push")

# Create branch
git(operation="branch", action="create", name="feature/new-ui")

# Switch branch
git(operation="branch", action="switch", name="main")

# View history
git(operation="log", limit=5)

# View changes
git(operation="diff", path="src/main.rs")
```

## Future Enhancements

If needed, we can add:
- **stash** - Save/restore work in progress
- **merge** - Merge branches
- **remote** - Manage remotes

But starting with these 9 operations keeps it simple and covers the most common workflows.

## Testing

To test:
1. Open IDE with git repository
2. Ask agent: "What's the git status?"
3. Ask agent: "Stage all my changes"
4. Ask agent: "Commit with message 'Update docs'"
5. Ask agent: "Push to remote"

The agent will use the native git tool instead of raw shell commands.
