# 🔍 Observability & Performance Analysis

## Overview

Forge-IDE includes comprehensive observability features to help you understand where time is spent during AI agent sessions and optimize performance.

## Quick Start

### Option 1: Langfuse (Recommended)

Beautiful web UI for LLM observability.

**Setup** (5 minutes):
```bash
# 1. Get credentials from https://cloud.langfuse.com
export LANGFUSE_PUBLIC_KEY="pk-lf-..."
export LANGFUSE_SECRET_KEY="sk-lf-..."

# 2. Build with langfuse feature
cargo build --release --features langfuse

# 3. Use the IDE - traces appear automatically!
```

**Result**: Click trace URLs in logs to see beautiful visualizations of:
- Where time is spent (API vs tools)
- Turn-by-turn breakdown
- Token usage and costs
- Performance bottlenecks

📖 **Full Guide**: See [LANGFUSE_GUIDE.md](./LANGFUSE_GUIDE.md)  
✅ **Quick Setup**: See [LANGFUSE_CHECKLIST.md](./LANGFUSE_CHECKLIST.md)

### Option 2: Local File Traces

Always-on, local-only trace files in JSONL format.

**Location**: `~/Library/Application Support/forge-ide/traces/`

**Usage**:
```bash
# Analyze most recent trace
python3 analyze_trace.py

# Tail live trace
tail -f ~/Library/Application\ Support/forge-ide/traces/agent-$(ls -t ~/Library/Application\ Support/forge-ide/traces/ | head -1)
```

## Example: Database Query Performance

### Problem
User asks database manager: "how does natural language to query work"
- Takes **76.4 seconds** ⚠️
- Feels slow and unresponsive

### Analysis with Langfuse

Open the trace URL and see:

```
Total Duration: 76.4s
├─ API Calls: 75.76s (99.2%) ← BOTTLENECK
├─ Tool Execution: 0.55s (0.7%)
└─ Overhead: 0.09s (0.1%)

Turn Breakdown:
├─ Turn 1: 4.4s (36K char prompt)
├─ Turn 2: 6.5s (339 char prompt) ← Why so slow?
├─ Turn 3: 5.6s (186 char prompt) ← Why so slow?
...
└─ Turn 17: 3.2s (1994 char prompt)

Insight: Agent is exploring step-by-step (17 turns!)
```

### Root Cause

**99.2% of time is AI API calls**, not tools. The agent:
- Makes 17 sequential API calls
- Each takes 3-7 seconds (network + model thinking)
- Even tiny prompts (186 chars) take 5+ seconds

### Solution

Three optimizations identified:

1. **Switch to faster model** → 50-70% faster
   - GPT-4o-mini instead of GPT-4
   - Claude Haiku instead of Sonnet

2. **Reduce turns** → 88-94% fewer API calls
   - Provide schema in initial prompt
   - Skip exploration phase
   - Direct completion instead of agent loop

3. **Add caching** → Near-instant for common queries
   - Cache schema information
   - Memoize common patterns

**Expected Result**: 76.4s → 5-10s (87-93% faster!) 🚀

## Performance Analysis Tools

### 1. Langfuse Dashboard

**What it shows**:
- ✅ Visual timeline of all operations
- ✅ Automatic bottleneck detection
- ✅ Token usage and costs
- ✅ Comparison across sessions
- ✅ Team collaboration

**When to use**: Primary debugging tool

### 2. Local Trace Analysis

**What it shows**:
- ✅ Raw event data in JSONL format
- ✅ Precise timestamps
- ✅ Complete tool arguments/results
- ✅ Works offline

**When to use**: Automation, scripting, when offline

### 3. Manual Log Inspection

**What it shows**:
- ✅ Low-level system events
- ✅ Error stack traces
- ✅ Module-specific debug info

**When to use**: Debugging system issues, errors

## Common Performance Issues

### Issue 1: High API Time (99%+)

**Symptoms**:
- Long wait times
- Small prompts still slow
- Many sequential turns

**Solutions**:
- Use faster/smaller model
- Reduce prompt size
- Add context caching
- Use streaming for better UX

### Issue 2: High Turn Count (15+)

**Symptoms**:
- Agent exploring "just in case"
- Repeated searches
- Reading unnecessary files

**Solutions**:
- Improve initial prompt with context
- Use direct completion instead of agent
- Add pre-computed repo map
- Better tool selection prompts

### Issue 3: Slow Tools (>0.1s)

**Symptoms**:
- Grep/search taking seconds
- File reads blocking
- Repeated tool calls

**Solutions**:
- Index codebase (ripgrep → indexed search)
- Cache file contents
- Batch operations (read_many_files)
- Use memoization

### Issue 4: Memory/Context Issues

**Symptoms**:
- Context window errors
- Slow prompt processing
- High token costs

**Solutions**:
- Implement context pruning
- Use embeddings for retrieval
- Smart context selection
- Hierarchical summaries

## Best Practices

### During Development

1. **Always check trace URLs** after slow operations
2. **Use Langfuse tags** to organize experiments
3. **Compare before/after** optimization attempts
4. **Share traces** with team for debugging
5. **Monitor token usage** to control costs

### For Production

1. **Set up alerting** for slow sessions (>30s)
2. **Track p50, p90, p99** latencies in Langfuse
3. **Use caching aggressively** for common queries
4. **Monitor API quotas** and rate limits
5. **Keep file traces** as backup/audit log

## Cost Optimization

Use observability to reduce costs:

| Optimization | Impact | Difficulty |
|--------------|--------|------------|
| Switch to cheaper model | -50-70% cost | Easy |
| Add context caching | -80-90% tokens | Medium |
| Reduce turn count | -60-80% calls | Medium |
| Better prompts | -20-40% tokens | Easy |
| Index codebase | -30-50% calls | Hard |

**ROI**: Typical optimization reduces costs by 60-80% while improving speed!

## Documentation

- **[LANGFUSE_GUIDE.md](./LANGFUSE_GUIDE.md)** - Comprehensive Langfuse setup and usage
- **[LANGFUSE_CHECKLIST.md](./LANGFUSE_CHECKLIST.md)** - Quick setup checklist
- **[LANGFUSE_INTEGRATION.md](./LANGFUSE_INTEGRATION.md)** - Technical integration details
- **[PERFORMANCE_ANALYSIS.md](./PERFORMANCE_ANALYSIS.md)** - Detailed performance study

## Tools

- **`analyze_trace.py`** - Analyze local JSONL traces
- **`test_langfuse.py`** - Test Langfuse connection

## Support

Issues with observability features?

1. Check environment variables are set
2. Verify feature is enabled in build
3. Review logs for errors
4. Test with provided scripts
5. File an issue with trace/logs

---

**Remember**: You can't optimize what you don't measure! 📊

Use these tools to:
- ✅ Identify bottlenecks instantly
- ✅ Validate optimizations work
- ✅ Track costs and token usage
- ✅ Debug issues collaboratively
- ✅ Build faster, cheaper AI features
