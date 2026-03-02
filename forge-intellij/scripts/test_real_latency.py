#!/usr/bin/env python3
import requests
import json
import time

def test_real_query():
    url = "http://localhost:8080/chat/stream"
    
    payload = {
        "question": "what is this project about?",
        "workspace_id": "test-workspace",
        "conversation_id": f"test-{int(time.time())}",
        "attached_files": []
    }
    
    print(f"Testing: {payload['question']}")
    start_time = time.time()
    
    response = requests.post(url, json=payload, stream=True, timeout=60)
    
    first_byte_time = None
    tool_start_time = None
    
    for line in response.iter_lines():
        if not line:
            continue
            
        if first_byte_time is None:
            first_byte_time = time.time()
            print(f"⏱️  First byte: {(first_byte_time - start_time)*1000:.0f}ms")
            
        line = line.decode('utf-8')
        
        if line.startswith('event: tool_start') and tool_start_time is None:
            tool_start_time = time.time()
            print(f"⏱️  Tool start: {(tool_start_time - start_time)*1000:.0f}ms")
            
        if line.startswith('event: done'):
            done_time = time.time()
            print(f"⏱️  Complete: {(done_time - start_time)*1000:.0f}ms")
            break
    
    total = time.time() - start_time
    print(f"\n📊 Total: {total*1000:.0f}ms")
    
    if total < 3:
        print("✅ EXCELLENT latency (<3s)")
    elif total < 5:
        print("✅ Good latency (<5s)")
    else:
        print("⚠️  Could be better (>5s)")

if __name__ == "__main__":
    test_real_query()
