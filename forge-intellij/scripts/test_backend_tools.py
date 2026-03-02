#!/usr/bin/env python3
import requests
import json
import time

def test_agent_tools():
    url = "http://localhost:8080/chat/stream"
    
    payload = {
        "question": "what is this project about?",
        "workspace_id": "test-workspace",
        "conversation_id": "test-conv-123",
        "attached_files": []
    }
    
    print("Sending request to backend...")
    print(f"Question: {payload['question']}\n")
    
    response = requests.post(url, json=payload, stream=True, timeout=60)
    
    if response.status_code != 200:
        print(f"❌ Error: HTTP {response.status_code}")
        print(response.text)
        return
    
    print("📡 Streaming response:\n")
    print("-" * 60)
    
    tool_calls = []
    text_chunks = []
    
    for line in response.iter_lines():
        if not line:
            continue
            
        line = line.decode('utf-8')
        
        if line.startswith('event:'):
            event_type = line.split(':', 1)[1].strip()
            continue
            
        if line.startswith('data:'):
            try:
                data_str = line.split(':', 1)[1].strip()
                data = json.loads(data_str)
                
                if 'text' in data:
                    text = data['text']
                    text_chunks.append(text)
                    print(text, end='', flush=True)
                    
                elif 'tool_name' in data:
                    tool_calls.append(data)
                    print(f"\n\n🔧 Tool Call: {data['tool_name']}")
                    if 'arguments' in data:
                        print(f"   Arguments: {json.dumps(data['arguments'], indent=2)}")
                        
            except json.JSONDecodeError:
                pass
    
    print("\n" + "-" * 60)
    print(f"\n✅ Test Complete!")
    print(f"   Text chunks received: {len(text_chunks)}")
    print(f"   Tool calls made: {len(tool_calls)}")
    
    if len(tool_calls) > 0:
        print(f"\n🎉 SUCCESS! Agent is calling tools properly!")
        for i, tc in enumerate(tool_calls, 1):
            print(f"   {i}. {tc['tool_name']}")
    else:
        print(f"\n❌ FAILURE: Agent did not call any tools (tool_calls: 0)")
        print(f"   This means the streaming bug is still present.")

if __name__ == "__main__":
    test_agent_tools()
