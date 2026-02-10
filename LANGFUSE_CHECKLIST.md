# Langfuse Setup Checklist

Quick reference for setting up Langfuse observability.

## ☑️ Prerequisites

- [ ] Rust 1.83+ installed
- [ ] Python 3.x (optional, for test script)
- [ ] Network access to Langfuse cloud or self-hosted instance

## ☑️ Setup Steps

### 1. Get Credentials (5 minutes)

- [ ] Visit https://cloud.langfuse.com
- [ ] Create account
- [ ] Create new project
- [ ] Copy Public Key (starts with `pk-lf-`)
- [ ] Copy Secret Key (starts with `sk-lf-`)

### 2. Configure Environment (1 minute)

Add to `~/.zshrc`, `~/.bashrc`, or create `.env`:

```bash
export LANGFUSE_PUBLIC_KEY="pk-lf-YOUR-KEY-HERE"
export LANGFUSE_SECRET_KEY="sk-lf-YOUR-KEY-HERE"
```

Then:
```bash
source ~/.zshrc  # or ~/.bashrc
```

- [ ] Environment variables set
- [ ] Shell restarted/sourced

### 3. Test Connection (Optional, 2 minutes)

```bash
pip install langfuse
python3 test_langfuse.py
```

Expected output:
```
✅ SUCCESS! Langfuse integration is working!
View trace: https://cloud.langfuse.com/trace/test-1234567890
```

- [ ] Test successful
- [ ] Can view test trace in Langfuse UI

### 4. Build with Langfuse (5 minutes)

```bash
cd /path/to/forge-ide
cargo build --release --features langfuse
```

- [ ] Build completed without errors
- [ ] Feature enabled (check for warnings)

### 5. Verify Integration (1 minute)

Run the IDE and make an AI query. Check logs for:

```
[FORGE] Langfuse observability enabled
[FORGE] Langfuse trace: https://cloud.langfuse.com/trace/abc-123
```

- [ ] See "Langfuse observability enabled" in logs
- [ ] Trace URL appears
- [ ] Can open trace in browser

## ✅ Success!

You're now tracking all AI agent sessions in Langfuse!

## 🔧 Troubleshooting

### Can't see "Langfuse observability enabled"

```bash
# Verify environment variables in the running shell
env | grep LANGFUSE

# Rebuild to ensure feature is compiled in
cargo clean
cargo build --release --features langfuse
```

### Trace URL not appearing

Check logs for errors:
```bash
tail -f ~/.local/share/forge-ide/logs/proxy.log | grep -i langfuse
```

Common issues:
- Wrong API keys → Check spelling in environment variables
- Network blocked → Check firewall/VPN settings
- Self-hosted not running → Start Langfuse server

### "Failed to create Langfuse hook"

- Verify keys are correct (no typos)
- Check network connection to Langfuse server
- Try test script to isolate issue

## 📚 Documentation

- Full guide: `LANGFUSE_GUIDE.md`
- Integration details: `LANGFUSE_INTEGRATION.md`
- Performance analysis: `PERFORMANCE_ANALYSIS.md`

## 🎯 Quick Tips

1. **Save time**: Use cloud.langfuse.com (free tier) instead of self-hosting
2. **Debug faster**: Open trace URL immediately after slow queries
3. **Compare sessions**: Use tags to group related queries
4. **Share traces**: Send URL to teammates for collaborative debugging
5. **Monitor costs**: Track token usage across all providers

---

**Total Setup Time**: ~15 minutes  
**Difficulty**: Easy  
**Value**: High - Instant LLM observability 🚀
