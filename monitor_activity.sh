#!/bin/bash

echo "🔍 Monitoring Forge IDE activity..."
echo ""
echo "Watching for:"
echo "  - Local trace files in /tmp/forge-traces/"
echo "  - Any Langfuse-related activity"
echo ""
echo "Press Ctrl+C to stop"
echo ""

# Watch for new trace files
watch -n 2 '
    echo "=== Local Traces ===" 
    ls -lht /tmp/forge-traces/ 2>/dev/null | head -5 || echo "No traces yet"
    echo ""
    echo "=== Environment Check ==="
    if pgrep -x "lapce" > /dev/null; then
        echo "✅ Lapce is running (PID: $(pgrep -x lapce))"
    else
        echo "❌ Lapce not running"
    fi
    echo ""
    echo "💡 Make an AI query in Forge IDE to generate traces!"
'
