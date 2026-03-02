#!/usr/bin/env python3
"""
Final comprehensive test to verify the agent tool calling is fixed.
"""
import requests
import json
import sys

def test_agent_fixed():
    url = "http://localhost:8080/chat/stream"
    
    test_cases = [
        "what is this project about?",
        "list all Python files",
        "explain the architecture"
    ]
    
    all_passed = True
    
    for i, question in enumerate(test_cases, 1):
        print(f"\n{'='*60}")
        print(f"TEST {i}/3: {question}")
        print(f"{'='*60}")
        
        payload = {
            "question": question,
            "workspace_id": "test-workspace",
            "conversation_id": f"test-conv-{i}-{int(__import__('time').time())}",
            "attached_files": []
        }
        
        try:
            response = requests.post(url, json=payload, stream=True, timeout=60)
            
            if response.status_code != 200:
                print(f"❌ HTTP {response.status_code}")
                all_passed = False
                continue
            
            tool_calls = 0
            text_received = False
            
            for line in response.iter_lines():
                if not line:
                    continue
                    
                line = line.decode('utf-8')
                
                if line.startswith('event: tool_start'):
                    tool_calls += 1
                elif line.startswith('event: text_delta'):
                    text_received = True
            
            if tool_calls > 0:
                print(f"✅ PASS: Agent called {tool_calls} tool(s)")
            else:
                print(f"❌ FAIL: Agent did not call any tools (streaming bug present)")
                all_passed = False
                
        except Exception as e:
            print(f"❌ ERROR: {e}")
            all_passed = False
    
    print(f"\n{'='*60}")
    if all_passed:
        print("🎉 ALL TESTS PASSED! The agent is calling tools correctly!")
        print("   The streaming bug has been FIXED. ✓")
        return 0
    else:
        print("❌ SOME TESTS FAILED. The streaming bug may still be present.")
        return 1

if __name__ == "__main__":
    sys.exit(test_agent_fixed())
