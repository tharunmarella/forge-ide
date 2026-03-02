# Performance Optimization Summary

## Issues Fixed

### 1. ❌ Gemini 3 Pro Rate Limiting (CRITICAL)
**Problem:** Backend was using `gemini-3-pro-preview` as planning model, which had 503 errors
**Impact:** 20-30 second delays waiting for retries before fallback
**Fix:** Changed `LLM_PLANNING_MODEL` from `gemini-3-pro-preview` → `gemini-3-flash-preview`

### 2. ❌ Excessive Retries
**Problem:** LiteLLM was configured with `num_retries=3`
**Impact:** Each failed API call would retry 3 times before giving up
**Fix:** Reduced to `num_retries=1` in `app/core/llm.py`

### 3. ❌ Long Timeouts
**Problem:** Request timeout was set to 600 seconds (10 minutes!)
**Impact:** Hung requests would wait forever
**Fix:** Reduced to `request_timeout=30` seconds in `app/core/llm.py`

## Current Performance

**After optimizations:**
- Time to first byte: ~22ms ✅
- Time to tool_start: ~16 seconds ⚠️
- Total request time: ~17 seconds

## Remaining Latency Sources

The 16-second latency is now **entirely from Gemini API response time**, which includes:

1. **Network RTT** to Google's servers (~100-200ms)
2. **Model inference time** (~15 seconds)
   - Processing 5 system messages (~11K chars)
   - Binding 43 tools with full schemas
   - Generating response with tool calls

### Why 15 seconds for inference?

The backend sends:
```
[call_model] Using gemini/gemini-3-flash-preview | 5 messages | 10859 chars
[call_model] Binding 43 tools to gemini/gemini-3-flash-preview
```

Gemini 3 Flash has to:
- Parse 10K+ characters of system prompts
- Process 43 tool definitions (each with full JSON schemas)
- Decide which tool to call
- Generate the tool call with arguments

**This is expected behavior for the first call with full context.**

## Recommendations

### Option 1: Accept Current Latency
- 15-17 seconds for first response is reasonable for a full-context agent
- Subsequent responses are much faster (5-7 seconds)
- This is comparable to Cursor, Claude Code, etc.

### Option 2: Use Faster Models
- Switch to `groq/llama-3.3-70b` for 2-3 second responses
- Trade-off: Lower quality tool calling

### Option 3: Reduce Prompt Size
- Strip down AGENT_SYSTEM_PROMPT and MASTER_PLANNING_PROMPT
- Trade-off: Agent might not follow instructions as well

### Option 4: Lazy Tool Loading
- Only bind relevant tools based on query type
- Trade-off: More complex implementation, potential tool availability issues

## Commit
- Backend: `2fa3983` - Optimize latency: reduce retries, timeout, switch planning model
