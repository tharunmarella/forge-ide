#!/bin/bash

# Kill any existing IDE instances
pkill -9 lapce 2>/dev/null

# Set Langfuse environment variables
export LANGFUSE_PUBLIC_KEY="pk-lf-f77359de-9119-4542-8224-35017647723a"
export LANGFUSE_SECRET_KEY="sk-lf-e2e3fd5c-6f06-43fa-ab75-7757a6a66dc9"
export LANGFUSE_BASE_URL="https://us.cloud.langfuse.com"

# Navigate to project directory
cd "$(dirname "$0")"

echo "🚀 Starting Forge IDE with Langfuse observability..."
echo "📊 Langfuse trace URLs will appear in: /tmp/forge_ide.log"
echo ""

# Start the IDE
./target/release/lapce > /tmp/forge_ide.log 2>&1 &

# Give it a moment to start
sleep 2

if pgrep -x "lapce" > /dev/null; then
    echo "✅ Forge IDE started successfully!"
    echo ""
    echo "To monitor Langfuse traces in real-time, run:"
    echo "  tail -f /tmp/forge_ide.log | grep -i langfuse"
else
    echo "❌ Failed to start Forge IDE"
    exit 1
fi
