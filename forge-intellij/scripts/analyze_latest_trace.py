import asyncio
from pymongo import MongoClient

def analyze_latest_trace():
    client = MongoClient("mongodb://mongo:IYXQEyLlmzsXejgVDPcSQRwlYkzRcCXO@mainline.proxy.rlwy.net:20298")
    db = client["forge_traces"]
    
    # Get the latest trace
    latest_trace = db.traces.find_one({}, sort=[("timestamp", -1)])
    if not latest_trace:
        print("No traces found")
        return
        
    print(f"Latest trace ID: {latest_trace.get('thread_id')}")
    print(f"Timestamp: {latest_trace.get('timestamp')}")
    
    messages = latest_trace.get('messages', [])
    print(f"Total messages logged: {len(messages)}")
    
    for msg in messages:
        if msg.get('type') == 'ai':
            print("-" * 40)
            print(f"AI Message Content: {msg.get('content')}")
            print(f"Tool Calls: {msg.get('tool_calls')}")
            print(f"Response Metadata: {msg.get('response_metadata')}")

if __name__ == "__main__":
    analyze_latest_trace()