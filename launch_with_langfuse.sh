#!/bin/bash
# Langfuse-enabled Forge IDE launcher

# Kill any existing instances
pkill -9 lapce 2>/dev/null

# Navigate to project directory
cd "$(dirname "$0")"

# Export Langfuse environment variables
export LANGFUSE_PUBLIC_KEY="pk-lf-9a4f4439-c185-4179-8d1f-fed226fbc086"
export LANGFUSE_SECRET_KEY="sk-lf-cde3c29a-eec9-45c3-9c19-47c96cdf0ba6"
export LANGFUSE_BASE_URL="https://us.cloud.langfuse.com"

# Also set RUST_LOG for detailed tracing
export RUST_LOG="info,forge_agent=debug"

echo "🚀 Starting Forge IDE with Langfuse observability..."
echo ""
echo "📊 Langfuse Configuration:"
echo "   Public Key: ${LANGFUSE_PUBLIC_KEY:0:20}..."
echo "   Base URL: $LANGFUSE_BASE_URL"
echo ""
echo "📝 Logs will be written to: forge_ide.log in current directory"
echo ""

# Start the IDE
./target/release/lapce > forge_ide.log 2>&1 &

# Give it a moment to start
sleep 3

if pgrep -q "lapce"; then
    echo "✅ Forge IDE started successfully!"
    echo ""
    echo "🔍 To monitor logs in real-time:"
    echo "   tail -f forge_ide.log"
    echo ""
    echo "🎯 To see Langfuse-specific logs:"
    echo "   tail -f forge_ide.log | grep -i langfuse"
    echo ""
    echo "Now make an AI query in the IDE to see Langfuse traces!"
else
    echo "❌ Failed to start Forge IDE"
    echo "Check forge_ide.log for errors"
    exit 1
fi
