# Langfuse Integration Guide

Forge-IDE now supports [Langfuse](https://langfuse.com) for comprehensive LLM observability! This gives you a beautiful web UI to analyze where time is spent, debug issues, and optimize performance.

## Why Langfuse?

Instead of parsing local log files, Langfuse provides:

- 🎯 **Visual Trace Analysis** - See exactly where time is spent in each agent session
- 📊 **Performance Metrics** - Track API latency, tool execution time, token usage
- 🔍 **Debugging Tools** - Inspect prompts, responses, and tool calls in a web UI
- 📈 **Analytics Dashboard** - Compare performance across sessions and models
- 🤝 **Team Collaboration** - Share traces with teammates for debugging

## Quick Start

### 1. Get Langfuse Credentials

#### Option A: Cloud (Easiest)
1. Sign up at [cloud.langfuse.com](https://cloud.langfuse.com)
2. Create a new project
3. Copy your Public Key and Secret Key

#### Option B: Self-Hosted
```bash
# Run Langfuse locally with Docker
docker run -d \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://... \
  langfuse/langfuse:latest
```

### 2. Configure Environment Variables

Add to your `.env`, `.zshrc`, or `.bashrc`:

```bash
# Langfuse Configuration
export LANGFUSE_PUBLIC_KEY="pk-lf-..."
export LANGFUSE_SECRET_KEY="sk-lf-..."

# Optional: For self-hosted instances
# export LANGFUSE_BASE_URL="http://localhost:3000"
```

### 3. Enable Langfuse Feature

Rebuild forge-ide with the langfuse feature enabled:

```bash
cd /path/to/forge-ide
cargo build --release --features langfuse
```

### 4. Use It!

When you run the IDE, Langfuse will automatically track all agent sessions:

```bash
# The IDE will log the trace URL on agent start:
[FORGE] Langfuse observability enabled
[FORGE] Langfuse trace: https://cloud.langfuse.com/trace/abc-123-def-456
```

Click the URL to see your trace in real-time!

## What Gets Tracked?

### Session-Level (Traces)
- Provider and model used
- User ID (if configured)
- Session metadata (workspace, query)
- Total duration and turn count

### API Calls (Generations)
- Each LLM API call (GPT, Claude, Gemini)
- Input prompts and output responses
- Latency for each turn
- Token usage (if reported by provider)

### Tool Executions (Spans)
- Each tool call (grep, read_file, etc.)
- Tool arguments and results
- Execution time
- Success/failure status

## Performance Analysis

After running a query, open the Langfuse trace URL to see:

### Timeline View
Visual representation of where time is spent:
```
Turn 1: ████████████████ (4.4s) API Call
  └─ Tool: grep ■ (0.13s)
Turn 2: ███████████████ (6.5s) API Call
  └─ Tool: grep ■ (0.13s)
...
```

### Metrics
- **Total Duration**: 76.4s
- **API Time**: 75.76s (99.2%)
- **Tool Time**: 0.55s (0.7%)
- **Turn Count**: 17
- **Most Expensive Turn**: Turn 7 (7.16s)

### Bottleneck Identification
Langfuse automatically highlights:
- Slow API calls
- Repeated tool executions
- High token usage
- Error patterns

## Example: Database Query Analysis

When you ask the database manager "how does natural language to query work", the Langfuse trace shows:

1. **Turn 1** (4.4s)
   - Prompt: 36,126 chars (includes context)
   - 2 grep tools called
   
2. **Turn 2-17** (3-7s each)
   - Sequential API calls
   - Minimal tool usage
   - **Insight**: Agent is exploring step-by-step

**Optimization Identified**: Reduce turns by providing better context upfront.

## Advanced Configuration

### Custom Metadata

The integration automatically includes:
- Provider and model
- Workspace path
- Prompt length
- Turn number and elapsed time

### User Tracking

To track queries by user:

```rust
// In your code that creates the LangfuseHook:
let hook = LangfuseHook::new(
    client,
    "my-session".to_string(),
    Some("user-123".to_string()),  // <-- Add user ID
    metadata
).await?;
```

### Session Tagging

Sessions are automatically tagged with:
- `forge-agent`
- `ai-coding`
- Provider name

## Troubleshooting

### "Langfuse not configured" message

Make sure:
1. Environment variables are set correctly
2. You rebuilt with `--features langfuse`
3. Variables are exported in your shell

Test with:
```bash
echo $LANGFUSE_PUBLIC_KEY
echo $LANGFUSE_SECRET_KEY
```

### Traces not showing up

1. Check Langfuse server is running (cloud or self-hosted)
2. Verify API keys are correct
3. Check logs for connection errors:
   ```bash
   # Look for Langfuse-related warnings
   tail -f ~/.local/share/forge-ide/logs/proxy.log
   ```

### Missing data in traces

The integration logs:
- ✅ Completion calls (LLM API)
- ✅ Tool executions
- ✅ Performance metrics
- ❌ Text deltas (too noisy)

If you need more granular data, enable file traces (always active).

## Comparing with File Traces

Both tracing methods are complementary:

### File Traces (JSONL)
- ✅ Always available (no network needed)
- ✅ Complete raw data
- ✅ Good for automation/scripts
- ❌ Requires manual parsing
- ❌ No visualization

### Langfuse
- ✅ Beautiful web UI
- ✅ Automatic analysis
- ✅ Team collaboration
- ✅ Analytics over time
- ❌ Requires network
- ❌ Additional setup

**Recommendation**: Use both! File traces are your backup and Langfuse is your primary debugging tool.

## Cost Optimization with Langfuse

Use Langfuse to identify expensive patterns:

1. **High API Time** (99%+)
   - Switch to faster model
   - Reduce context size
   - Use caching

2. **High Turn Count** (15+)
   - Improve initial prompt
   - Provide better context
   - Use direct completion instead of agent

3. **Repeated Tool Calls**
   - Cache results
   - Batch operations
   - Improve search strategy

4. **Slow Tools** (>0.1s)
   - Optimize grep patterns
   - Index codebase
   - Use faster search methods

## Integration Details

### Architecture

```
User Query
    ↓
Forge Agent (dispatch.rs)
    ↓
Agent Loop (rig-core)
    ↓
LangfuseHook (langfuse_hook.rs)
    ↓
Langfuse API
    ↓
Langfuse Dashboard
```

### What's Sent to Langfuse

**Metadata Only**:
- Trace ID, session name
- Model provider and name
- Turn numbers
- Timestamps and durations
- Tool names and argument lengths
- Result lengths (not full content)

**Truncated Data**:
- First turn prompt: 8000 chars
- Subsequent prompts: 1000 chars
- Tool arguments: 2000 chars
- Tool results: 500 char preview

**Not Sent**:
- Full file contents
- Sensitive data (credentials, keys)
- Complete codebase
- User's personal files

### Privacy Note

Langfuse runs as a separate service. If privacy is a concern:
- Use self-hosted Langfuse
- Review truncation limits in `langfuse_hook.rs`
- Disable feature if not needed
- File traces remain completely local

## Further Reading

- [Langfuse Documentation](https://langfuse.com/docs)
- [Langfuse API Reference](https://api.reference.langfuse.com)
- [Self-Hosting Guide](https://langfuse.com/docs/deployment/self-host)
- [Trace Analysis Best Practices](https://langfuse.com/docs/tracing)

## Support

Issues with Langfuse integration?
1. Check environment variables
2. Verify network connectivity
3. Enable debug logging: `RUST_LOG=forge_agent=debug`
4. File an issue with log snippets

---

**Generated**: 2026-02-09
**Feature**: Langfuse observability integration
