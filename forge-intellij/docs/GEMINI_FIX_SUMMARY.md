# Gemini 3 Tool Calling Fix - Summary

## Problem
The Forge backend agent was stuck at "thinking" and never executing tools when using `gemini-3-flash-preview` and `gemini-3-pro-preview` models. MongoDB traces showed `tool_calls: 0` consistently.

## Root Causes

### 1. Schema Pollution Bug
**Issue:** LangChain's `InjectedState` annotation was not properly stripping the `state` parameter from tool schemas sent to Gemini.

**Impact:** 
- The `AgentState` object (containing entire message history + workspace metadata) was included in every tool's schema
- This created deeply nested schemas that exceeded Gemini's 10-level depth limit
- Gemini saw it couldn't provide the required `state` parameter and gave up on tool calling entirely

**Fix (commit f3a998e):**
```python
# Added schema scrubbing in app/core/agent.py
for t in ALL_TOOLS:
    if hasattr(t, "args_schema") and t.args_schema:
        # Remove 'state' field from the schema
        fields = {
            name: (field.annotation, field) 
            for name, field in t.args_schema.model_fields.items() 
            if name != 'state'
        }
        from pydantic import create_model
        t.args_schema = create_model(f"{t.name}Schema", **fields)
```

### 2. Streaming Parser Bug
**Issue:** LangChain's streaming response parser has a known edge case where it drops tool calls when both conversational text AND tool calls are present in the same streamed response.

**Impact:**
- The prompts explicitly told Gemini: "Always explain what you're doing and why before each tool call"
- Gemini obediently output: "I will start by exploring..." followed by the tool call
- The streaming parser successfully parsed the text but reported `tool_calls: 0`

**Fix (commit 47c34c7):**
```python
# Updated app/core/prompts.py
AGENT_SYSTEM_PROMPT = """You are an expert software engineer in Forge IDE. 
Execute tasks with tools — never just describe what you'd do. 
DO NOT output conversational text before tool calls."""

# Changed from:
"Always explain what you're doing and why before each tool call."
# To:
"DO NOT output conversational text before calling a tool. Just call the tool directly."
```

## Additional Fixes

### 3. Temperature Coercion (commit f3a998e)
Removed `gemini-3` from the hardcoded temperature=1.0 coercion in `app/core/llm.py`, allowing it to use `temperature=0.1` for reliable tool calling.

### 4. MongoDB Telemetry Bug (commit 554bc38)
Fixed `app/api/chat.py` to include `messages` array in MongoDB logging, ensuring accurate `tool_call_count` metrics.

### 5. Parallel Tool Prevention (commit f3a998e)
Added explicit prompt instruction: "Call ONLY ONE tool per response. Do NOT call multiple tools in parallel."

## Test Results

### Before Fix
```
tool_calls: 0
Status: Agent stuck at "thinking"
```

### After Fix
```
tool_calls: 1+
Status: Agent successfully executing tools
```

## Docker Compose Setup
Created `docker-compose.yml` with:
- PostgreSQL with pgvector
- Redis
- MongoDB
- Forge API

Run locally with:
```bash
cd forge-search
docker-compose up -d
```

## Commits
- Backend: `f3a998e`, `47c34c7`, `2db10b9`, `554bc38`, `a8419d4`
- Plugin: `a3fccf4`

## Environment
- Python 3.10.19
- litellm: unknown version (no __version__ attribute)
- langchain-core: 1.2.16
- langchain-litellm: 0.6.1

---
*Fixed: 2026-03-02*
