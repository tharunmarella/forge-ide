#!/usr/bin/env python3
import requests
import json

def test_sse_events():
    url = "http://localhost:8080/chat/stream"
    
    payload = {
        "question": "list all files in the project",
        "workspace_id": "test-workspace",
        "conversation_id": "test-conv-" + str(int(1000 * __import__('time').time())),
        "attached_files": []
    }
    
    print(f"Sending request: {payload['question']}")
    print(f"Conversation ID: {payload['conversation_id']}\n")
    
    response = requests.post(url, json=payload, stream=True, timeout=60)
    
    event_counts = {}
    tool_start_events = []
    
    current_event = None
    
    for line in response.iter_lines():
        if not line:
            continue
            
        line = line.decode('utf-8')
        
        if line.startswith('event:'):
            current_event = line.split(':', 1)[1].strip()
            event_counts[current_event] = event_counts.get(current_event, 0) + 1
            
        if line.startswith('data:'):
            data_str = line.split(':', 1)[1].strip()
            try:
                data = json.loads(data_str)
                
                if current_event == 'tool_start':
                    tool_start_events.append(data)
                    print(f"🔧 tool_start #{len(tool_start_events)}: {data.get('tool_name')}")
                    
                elif current_event == 'text_delta':
                    print(data.get('text', ''), end='', flush=True)
                    
            except json.JSONDecodeError:
                pass
    
    print("\n\n" + "=" * 60)
    print("EVENT SUMMARY:")
    for event_type, count in sorted(event_counts.items()):
        print(f"  {event_type}: {count}")
    
    print(f"\nTOOL START EVENTS: {len(tool_start_events)}")
    for i, tool in enumerate(tool_start_events, 1):
        print(f"  {i}. {tool.get('tool_name')} (id: {tool.get('tool_call_id', 'N/A')[:8]}...)")

if __name__ == "__main__":
    test_sse_events()
