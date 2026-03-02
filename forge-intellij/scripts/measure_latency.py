#!/usr/bin/env python3
import requests
import json
import time

def measure_latency():
    url = "http://localhost:8080/chat/stream"
    
    payload = {
        "question": "hello",
        "workspace_id": "test-workspace",
        "conversation_id": f"test-{int(time.time())}",
        "attached_files": []
    }
    
    print("Sending simple 'hello' request to measure latency...")
    start_time = time.time()
    
    response = requests.post(url, json=payload, stream=True, timeout=60)
    
    first_byte_time = None
    tool_start_time = None
    done_time = None
    
    for line in response.iter_lines():
        if not line:
            continue
            
        if first_byte_time is None:
            first_byte_time = time.time()
            print(f"⏱️  Time to first byte: {(first_byte_time - start_time)*1000:.0f}ms")
            
        line = line.decode('utf-8')
        
        if line.startswith('event: tool_start') and tool_start_time is None:
            tool_start_time = time.time()
            print(f"⏱️  Time to tool_start: {(tool_start_time - start_time)*1000:.0f}ms")
            
        if line.startswith('event: done'):
            done_time = time.time()
            print(f"⏱️  Time to completion: {(done_time - start_time)*1000:.0f}ms")
    
    total_time = time.time() - start_time
    print(f"\n📊 Total request time: {total_time*1000:.0f}ms")
    
    if total_time > 5:
        print("⚠️  WARNING: Response took more than 5 seconds!")
    elif total_time > 2:
        print("⚠️  Response is slower than expected (>2s)")
    else:
        print("✅ Response time is acceptable")

if __name__ == "__main__":
    measure_latency()
