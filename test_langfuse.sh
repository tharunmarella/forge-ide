#!/bin/bash

echo "🧪 Testing Langfuse Integration"
echo "================================"
echo ""

# Check if IDE is running
if pgrep -x "lapce" > /dev/null; then
    echo "✅ IDE is running (PID: $(pgrep -x lapce))"
else
    echo "❌ IDE is NOT running"
    echo "   Run: ./launch_with_langfuse.sh"
    exit 1
fi

echo ""
echo "📝 Testing environment variable access..."
echo ""

# Create a test script that the agent can run
cat > /tmp/test_langfuse_env.sh << 'EOF'
#!/bin/bash
echo "LANGFUSE_PUBLIC_KEY: ${LANGFUSE_PUBLIC_KEY:-NOT SET}"
echo "LANGFUSE_SECRET_KEY: ${LANGFUSE_SECRET_KEY:-NOT SET}"
echo "LANGFUSE_BASE_URL: ${LANGFUSE_BASE_URL:-NOT SET}"
EOF

chmod +x /tmp/test_langfuse_env.sh

# Export env vars and run test
export LANGFUSE_PUBLIC_KEY="pk-lf-7806f2ce-cc93-4ea0-8c1e-2aba09b9b9e9"
export LANGFUSE_SECRET_KEY="sk-lf-0f8be63d-5ea1-4291-ae14-a4dbff3eb46d"
export LANGFUSE_BASE_URL="https://us.cloud.langfuse.com"

echo "Current shell environment:"
/tmp/test_langfuse_env.sh

echo ""
echo "================================"
echo ""
echo "⚠️  IMPORTANT: Have you actually made an AI query?"
echo ""
echo "To test Langfuse, you MUST:"
echo ""
echo "1. Open the Forge IDE (should already be running)"
echo "2. Open the AI chat panel"
echo "3. Ask ANY question, like:"
echo "   - 'list all files in this project'"
echo "   - 'explain what this code does'"
echo "   - 'help me debug this'"
echo ""
echo "4. Then run this to check for traces:"
echo "   ls -lh /tmp/forge-traces/"
echo ""
echo "If NO trace files appear, the AI agent isn't running."
echo "This could mean:"
echo "  - No AI API key configured in settings"
echo "  - AI feature not enabled"
echo "  - You haven't actually made a query yet"
echo ""
echo "================================"
