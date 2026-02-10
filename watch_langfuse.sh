#!/bin/bash
# Monitor Langfuse traces in real-time

echo "=================================================="
echo "🔍 Langfuse Trace Monitor"
echo "=================================================="
echo ""
echo "Watching for Langfuse traces..."
echo "Make an AI query in Forge IDE to see traces appear here."
echo ""
echo "Press Ctrl+C to stop"
echo ""

tail -f /tmp/forge_ide.log | grep --line-buffered -E "Langfuse|FORGE.*trace"
