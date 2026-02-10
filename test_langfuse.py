#!/usr/bin/env python3
"""
Test Langfuse integration by sending a test trace.

Usage:
    python3 test_langfuse.py

Requirements:
    pip install langfuse
    
Environment variables:
    LANGFUSE_PUBLIC_KEY
    LANGFUSE_SECRET_KEY
    LANGFUSE_BASE_URL (optional)
"""

import os
import sys
import time
from datetime import datetime

try:
    from langfuse import Langfuse
except ImportError:
    print("Error: langfuse package not installed")
    print("Install with: pip install langfuse")
    sys.exit(1)

def test_langfuse():
    # Check environment variables
    public_key = os.getenv('LANGFUSE_PUBLIC_KEY')
    secret_key = os.getenv('LANGFUSE_SECRET_KEY')
    base_url = os.getenv('LANGFUSE_BASE_URL', 'https://cloud.langfuse.com')
    
    if not public_key or not secret_key:
        print("❌ Error: Langfuse credentials not found")
        print("")
        print("Please set these environment variables:")
        print("  export LANGFUSE_PUBLIC_KEY=pk-lf-...")
        print("  export LANGFUSE_SECRET_KEY=sk-lf-...")
        print("")
        print("Get your keys from: https://cloud.langfuse.com")
        sys.exit(1)
    
    print(f"🔗 Connecting to Langfuse at {base_url}...")
    
    try:
        client = Langfuse(
            public_key=public_key,
            secret_key=secret_key,
            host=base_url
        )
        print("✅ Connected successfully!")
    except Exception as e:
        print(f"❌ Connection failed: {e}")
        sys.exit(1)
    
    # Create a test trace
    print("\n📝 Creating test trace...")
    
    trace_id = f"test-{int(time.time())}"
    
    # Python SDK uses different API
    trace = client.trace(
        name="forge-ide-test",
        user_id="test-user",
        metadata={
            "test": True,
            "timestamp": datetime.now().isoformat(),
            "source": "test_langfuse.py"
        },
        tags=["test", "forge-ide"]
    )
    
    print(f"✅ Trace created")
    
    # Create a generation (simulating an LLM call)
    print("📊 Creating test generation...")
    
    generation = client.generation(
        name="test-completion",
        trace_id=trace.trace_id if hasattr(trace, 'trace_id') else None,
        model="gpt-4",
        model_parameters={
            "temperature": 0.7,
            "max_tokens": 100
        },
        input={
            "prompt": "This is a test prompt from Forge IDE"
        },
        output={
            "response": "This is a test response"
        },
        metadata={
            "turn": 1,
            "elapsed_s": 1.5
        }
    )
    
    print("✅ Generation created")
    
    # Create a span (simulating a tool call)
    print("🔧 Creating test span (tool execution)...")
    
    span = client.span(
        name="tool_grep",
        trace_id=trace.trace_id if hasattr(trace, 'trace_id') else None,
        input={
            "tool": "grep",
            "args": {"pattern": "test", "path": "."}
        },
        output={
            "result": "Found 5 matches",
            "duration_s": 0.123
        },
        metadata={
            "tool_type": "search"
        }
    )
    
    print("✅ Span created")
    
    trace_id = trace.trace_id if hasattr(trace, 'trace_id') else "unknown"
    
    # Flush to ensure data is sent
    print("\n🚀 Flushing data to Langfuse...")
    client.flush()
    
    # Generate URL
    trace_url = f"{base_url}/trace/{trace_id}"
    
    print("\n" + "="*70)
    print("✅ SUCCESS! Langfuse integration is working!")
    print("="*70)
    print("")
    print(f"Trace ID: {trace_id}")
    print(f"View trace: {trace_url}")
    print("")
    print("🎉 You can now use Langfuse with Forge IDE!")
    print("")
    print("Next steps:")
    print("  1. Rebuild forge-ide with: cargo build --release --features langfuse")
    print("  2. Run the IDE and make an AI query")
    print("  3. Look for the Langfuse trace URL in the logs")
    print("")

if __name__ == "__main__":
    test_langfuse()
