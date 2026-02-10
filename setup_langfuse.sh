#!/bin/bash
# Setup script for Langfuse integration with Forge IDE

set -e

echo "=================================================="
echo "Forge IDE - Langfuse Integration Setup"
echo "=================================================="
echo ""

# Set credentials
export LANGFUSE_SECRET_KEY="sk-lf-cde3c29a-eec9-45c3-9c19-47c96cdf0ba6"
export LANGFUSE_PUBLIC_KEY="pk-lf-9a4f4439-c185-4179-8d1f-fed226fbc086"
export LANGFUSE_BASE_URL="https://us.cloud.langfuse.com"

# Test connection
echo "Step 1: Testing Langfuse connection..."
python3 test_langfuse_simple.py
if [ $? -ne 0 ]; then
    echo ""
    echo "❌ Connection test failed. Please check your credentials."
    exit 1
fi

echo ""
echo "Step 2: Building forge-agent with Langfuse feature..."
cd forge-agent
cargo build --release --features langfuse

if [ $? -eq 0 ]; then
    echo ""
    echo "=================================================="
    echo "✅ Setup Complete!"
    echo "=================================================="
    echo ""
    echo "Langfuse is now enabled in forge-agent."
    echo ""
    echo "To enable in the full IDE:"
    echo "  cd .."
    echo "  cargo build --release"
    echo ""
    echo "When you run queries, look for:"
    echo "  [FORGE] Langfuse trace: https://us.cloud.langfuse.com/trace/..."
    echo ""
    echo "Open that URL to see your traces!"
    echo ""
else
    echo ""
    echo "❌ Build failed. Check the errors above."
    exit 1
fi
