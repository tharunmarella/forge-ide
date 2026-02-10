# Testing Langfuse Integration - Step by Step

## Current Status

✅ **Code is built with Langfuse support**
✅ **Langfuse feature is compiled in by default**
✅ **Environment variables are set in the launch script**
❓ **Need to test with an actual AI query**

## How to Test

### 1. Launch the IDE
```bash
./launch_with_langfuse.sh
```

This will:
- Kill any existing Lapce instances
- Export Langfuse credentials
- Start the IDE with proper environment variables

### 2. Make an AI Query

In the Forge IDE:
1. Open any file or project
2. Use the AI chat feature
3. Ask any question (e.g., "explain this code", "list files", etc.)

### 3. Check for Traces

**Option A: Check Local Traces**
```bash
ls -lh /tmp/forge-traces/
```

You should see `.jsonl` files being created.

**Option B: Monitor in Real-Time**
```bash
./monitor_activity.sh
```

This will watch for:
- New trace files in `/tmp/forge-traces/`
- Langfuse activity

### 4. Look for Langfuse Logs

When you make an AI query, the system should log:

```
[FORGE] Checking Langfuse configuration...
[FORGE] LANGFUSE_PUBLIC_KEY present: true
[FORGE] LANGFUSE_SECRET_KEY present: true
[FORGE] Langfuse observability enabled
[Langfuse] Client initialized successfully
[FORGE] Langfuse trace: https://us.cloud.langfuse.com/trace/YOUR-TRACE-ID
```

## Debugging Steps

### If You Don't See Langfuse Logs

1. **Check if the IDE is actually using our binary**:
   ```bash
   ps aux | grep lapce
   ```
   Should show: `./target/release/lapce`

2. **Verify environment variables are set**:
   The launch script should have started the IDE with:
   - `LANGFUSE_PUBLIC_KEY`
   - `LANGFUSE_SECRET_KEY`
   - `LANGFUSE_BASE_URL`

3. **Check trace files are being created**:
   ```bash
   ls -lht /tmp/forge-traces/ | head -5
   ```
   If trace files ARE being created, then the agent is working but Langfuse might not be initialized.

4. **Look at the most recent trace file**:
   ```bash
   tail -20 $(ls -t /tmp/forge-traces/*.jsonl | head -1)
   ```
   This will show if any errors occurred.

## Expected Behavior

When working correctly, for each AI query you should see:

1. **Local trace file created**: `/tmp/forge-traces/forge-traces-TIMESTAMP.jsonl`
2. **Langfuse initialization**: Log messages about Langfuse being enabled
3. **Langfuse trace URL**: A URL like `https://us.cloud.langfuse.com/trace/...`

You can then click the trace URL to see the complete trace in Langfuse's web interface!

## Troubleshooting

### "Langfuse not configured" message

This means the environment variables aren't being read. Make sure you:
1. Started the IDE using `./launch_with_langfuse.sh`
2. Didn't click the Lapce icon in Dock (that won't have env vars)

### No trace files at all

This means the AI agent isn't running. Check:
1. Is your AI API key configured in Forge?
2. Are you actually making an AI query?

### Trace files created but no Langfuse logs

This is the current situation. It could mean:
1. Environment variables aren't being inherited by the proxy
2. The Langfuse client is failing silently
3. Logs are going somewhere else

The code is definitely compiled in and should work - we just need to ensure the environment variables reach the running process.

## Next Step

**Please try making an AI query in the IDE now** and then let me know:
1. Do you see trace files being created in `/tmp/forge-traces/`?
2. Can you share the most recent trace file?
3. Do you see any Langfuse-related output anywhere?

This will help me pinpoint exactly where the issue is!
