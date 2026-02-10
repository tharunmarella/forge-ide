#!/usr/bin/env python3
"""
Quick test to verify Langfuse credentials work.

Usage:
    python3 test_langfuse_simple.py

Environment variables:
    LANGFUSE_PUBLIC_KEY
    LANGFUSE_SECRET_KEY
    LANGFUSE_BASE_URL (optional)
"""

import os
import sys

try:
    from langfuse import Langfuse
except ImportError:
    print("❌ Error: langfuse package not installed")
    print("Install with: pip install langfuse")
    sys.exit(1)

def test_connection():
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
        return False
    
    print(f"🔗 Testing connection to Langfuse...")
    print(f"   Base URL: {base_url}")
    print(f"   Public Key: {public_key[:15]}...")
    
    try:
        # Initialize client - this will validate credentials
        client = Langfuse(
            public_key=public_key,
            secret_key=secret_key,
            host=base_url
        )
        
        print("✅ Connection successful!")
        print("")
        print("="*70)
        print("✅ SUCCESS! Langfuse credentials are valid!")
        print("="*70)
        print("")
        print("🎉 You can now use Langfuse with Forge IDE!")
        print("")
        print("Next steps:")
        print("  1. Build forge-ide with langfuse feature:")
        print("     cargo build --release --features langfuse")
        print("")
        print("  2. Run the IDE and make an AI query")
        print("")
        print("  3. Look for the Langfuse trace URL in the logs:")
        print("     [FORGE] Langfuse trace: https://...")
        print("")
        print("  4. Open the URL in your browser to see the trace!")
        print("")
        return True
        
    except Exception as e:
        print(f"❌ Connection failed: {e}")
        print("")
        print("Common issues:")
        print("  - Wrong API keys (check for typos)")
        print("  - Wrong base URL (US cloud is https://us.cloud.langfuse.com)")
        print("  - Network/firewall blocking connection")
        print("")
        return False

if __name__ == "__main__":
    success = test_connection()
    sys.exit(0 if success else 1)
