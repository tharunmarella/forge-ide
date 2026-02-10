# 🚀 Quick Start: Langfuse Setup for Your Forge IDE

## Your Credentials (Already Configured!)

✅ Your Langfuse account is set up and credentials are stored in `.env`:
- **Base URL**: https://us.cloud.langfuse.com
- **Public Key**: pk-lf-9a4f4439-c185-4179-8d1f-fed226fbc086
- **Secret Key**: sk-lf-cde3c29a-eec9-45c3-9c19-47c96cdf0ba6

## One-Command Setup

```bash
cd /Users/tharun/Documents/projects/forge-ide
./setup_langfuse.sh
```

This will:
1. ✅ Verify your Langfuse connection
2. ✅ Build forge-agent with Langfuse enabled
3. ✅ Show you the next steps

## Manual Setup (Alternative)

If you prefer step-by-step:

### 1. Load Environment Variables

```bash
# Option A: Source the .env file
export $(cat .env | xargs)

# Option B: Add to your shell profile (~/.zshrc or ~/.bashrc)
echo 'export LANGFUSE_PUBLIC_KEY="pk-lf-9a4f4439-c185-4179-8d1f-fed226fbc086"' >> ~/.zshrc
echo 'export LANGFUSE_SECRET_KEY="sk-lf-cde3c29a-eec9-45c3-9c19-47c96cdf0ba6"' >> ~/.zshrc
echo 'export LANGFUSE_BASE_URL="https://us.cloud.langfuse.com"' >> ~/.zshrc
source ~/.zshrc
```

### 2. Test Connection

```bash
python3 test_langfuse_simple.py
```

Expected output:
```
✅ SUCCESS! Langfuse credentials are valid!
```

### 3. Build with Langfuse

```bash
# Just the agent (faster for testing)
cd forge-agent
cargo build --release --features langfuse

# Or the full IDE
cd ..
cargo build --release --features langfuse
```

## Using Langfuse

### When You Run Queries

Look for these log messages:

```
[FORGE] Langfuse observability enabled
[FORGE] Langfuse trace: https://us.cloud.langfuse.com/trace/abc-123-def-456
```

### Open the Trace

Click the URL in your logs to see:

```
┌─────────────────────────────────────────────┐
│  Forge Agent Session                        │
│  Provider: anthropic, Model: claude-3-5     │
├─────────────────────────────────────────────┤
│  Turn 1 (4.4s) - Generation                 │
│  ├─ Tool: grep (0.13s)                      │
│  └─ Tool: grep (0.13s)                      │
│                                             │
│  Turn 2 (6.5s) - Generation                 │
│  └─ Tool: grep (0.01s)                      │
│                                             │
│  ...                                        │
│                                             │
│  Turn 17 (3.2s) - Generation                │
│  └─ Tool: grep (0.01s)                      │
├─────────────────────────────────────────────┤
│  Total: 76.4s                               │
│  API: 75.76s (99.2%) ← BOTTLENECK          │
│  Tools: 0.55s (0.7%)                        │
└─────────────────────────────────────────────┘
```

### Real-Time Monitoring

Your Langfuse dashboard: https://us.cloud.langfuse.com

Here you can:
- 📊 See all your traces
- 🔍 Search and filter sessions
- 📈 Track performance over time
- 💰 Monitor token usage and costs
- 🏷️ Organize with tags
- 👥 Share with teammates

## Example: Database Query

When you ask: _"how does the natural language to query work"_

1. **Make the query** in the database manager
2. **Check logs** for the Langfuse URL
3. **Open URL** in browser
4. **See the breakdown**:
   - 17 turns
   - 99.2% time in API calls
   - Agent exploring step-by-step
5. **Identify optimization**:
   - Use faster model (GPT-4o-mini)
   - Reduce turns (better context)
   - Add caching

Result: **76.4s → 5-10s** (87% faster!)

## Troubleshooting

### Environment variables not loaded?

```bash
# Verify they're set
echo $LANGFUSE_PUBLIC_KEY

# If empty, reload
export $(cat .env | xargs)
```

### Build errors?

```bash
# Clean and rebuild
cargo clean
cd forge-agent
cargo build --release --features langfuse
```

### Can't see traces?

1. Check you rebuilt with `--features langfuse`
2. Verify logs show "Langfuse observability enabled"
3. Check network connection to https://us.cloud.langfuse.com
4. Look for error messages in logs

## What's Next?

1. **Run a test query** to generate your first trace
2. **Explore the Langfuse UI** to understand the metrics
3. **Use insights** to optimize your agent
4. **Compare performance** across different models/prompts

## Key Files

- **`.env`** - Your credentials (already configured!)
- **`setup_langfuse.sh`** - One-command setup script
- **`test_langfuse_simple.py`** - Test your connection
- **`LANGFUSE_GUIDE.md`** - Comprehensive documentation
- **`LANGFUSE_CHECKLIST.md`** - Step-by-step setup guide
- **`OBSERVABILITY.md`** - Performance analysis guide

## Quick Commands

```bash
# Test connection
python3 test_langfuse_simple.py

# Build with Langfuse
cargo build --release --features langfuse

# Run the IDE
./target/release/lapce

# Check traces directory (fallback logs)
ls -la ~/Library/Application\ Support/forge-ide/traces/

# Analyze local trace
python3 analyze_trace.py
```

## Support

Need help?
1. Check the documentation files listed above
2. Verify environment variables are set
3. Test connection with `test_langfuse_simple.py`
4. Check logs in `~/.local/share/forge-ide/logs/`

---

**Your Langfuse Dashboard**: https://us.cloud.langfuse.com  
**Status**: ✅ Credentials configured and validated  
**Next Step**: Run `./setup_langfuse.sh` to build with Langfuse enabled!

Happy debugging! 🎉
