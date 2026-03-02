#!/usr/bin/env python3
"""
Analyze agent behavior from MongoDB traces
"""

from pymongo import MongoClient
from datetime import datetime, timedelta
import json
from collections import Counter, defaultdict

# MongoDB connection
MONGO_URI = "mongodb://mongo:IYXQEyLlmzsXejgVDPcSQRwlYkzRcCXO@mainline.proxy.rlwy.net:20298"

def connect_to_mongo():
    """Connect to MongoDB and return client"""
    try:
        client = MongoClient(MONGO_URI, serverSelectionTimeoutMS=5000)
        # Test connection
        client.server_info()
        print("✓ Connected to MongoDB successfully")
        return client
    except Exception as e:
        print(f"✗ Failed to connect to MongoDB: {e}")
        return None

def list_databases(client):
    """List all databases"""
    print("\n=== Available Databases ===")
    dbs = client.list_database_names()
    for db in dbs:
        print(f"  - {db}")
    return dbs

def explore_collections(client, db_name):
    """Explore collections in a database"""
    print(f"\n=== Collections in '{db_name}' ===")
    db = client[db_name]
    collections = db.list_collection_names()
    for coll in collections:
        count = db[coll].count_documents({})
        print(f"  - {coll}: {count} documents")
    return collections

def analyze_traces(client, db_name="forge_search"):
    """Analyze agent traces"""
    print(f"\n=== Analyzing Agent Traces ===")
    
    try:
        db = client[db_name]
        
        # Check if traces collection exists
        if 'traces' not in db.list_collection_names():
            print(f"✗ No 'traces' collection found in {db_name}")
            return
        
        traces_coll = db['traces']
        total_traces = traces_coll.count_documents({})
        print(f"\nTotal traces: {total_traces}")
        
        # Get recent traces (last 24 hours)
        yesterday = datetime.utcnow() - timedelta(days=1)
        recent_traces = list(traces_coll.find(
            {"created_at": {"$gte": yesterday}}
        ).sort("created_at", -1).limit(50))
        
        print(f"\nRecent traces (last 24h): {len(recent_traces)}")
        
        # Analyze tool usage
        tool_counts = Counter()
        model_usage = Counter()
        error_counts = defaultdict(int)
        total_tools_executed = 0
        successful_tools = 0
        failed_tools = 0
        
        for trace in recent_traces:
            # Model used
            if 'model' in trace:
                model_usage[trace['model']] += 1
            
            # Extract tool calls from messages
            messages = trace.get('messages', [])
            for msg in messages:
                if isinstance(msg, dict) and msg.get('type') == 'ai':
                    tool_calls = msg.get('tool_calls', [])
                    for tool_call in tool_calls:
                        tool_name = tool_call.get('name', 'unknown')
                        tool_counts[tool_name] += 1
                        total_tools_executed += 1
                
                # Check for tool results
                if isinstance(msg, dict) and msg.get('type') == 'tool':
                    if msg.get('status') == 'error':
                        failed_tools += 1
                        error_msg = msg.get('content', '')
                        error_counts[error_msg[:100]] += 1  # First 100 chars
                    else:
                        successful_tools += 1
        
        # Print analysis
        print("\n=== Tool Usage Statistics ===")
        if tool_counts:
            for tool, count in tool_counts.most_common(20):
                print(f"  {tool}: {count}")
        else:
            print("  No tools found in recent traces")
        
        print("\n=== Model Usage ===")
        for model, count in model_usage.most_common():
            print(f"  {model}: {count}")
        
        print(f"\n=== Tool Execution Summary ===")
        print(f"  Total tools executed: {total_tools_executed}")
        print(f"  Successful: {successful_tools}")
        print(f"  Failed: {failed_tools}")
        if total_tools_executed > 0:
            success_rate = (successful_tools / total_tools_executed) * 100
            print(f"  Success rate: {success_rate:.1f}%")
        
        if error_counts:
            print("\n=== Top Errors ===")
            for error, count in sorted(error_counts.items(), key=lambda x: x[1], reverse=True)[:5]:
                print(f"  [{count}x] {error}")
        
        # Show sample recent conversation
        print("\n=== Recent Conversation Sample ===")
        if recent_traces:
            latest = recent_traces[0]
            print(f"\nThread ID: {latest.get('thread_id', 'N/A')}")
            print(f"Created: {latest.get('created_at', 'N/A')}")
            print(f"Status: {latest.get('status', 'N/A')}")
            print(f"Model: {latest.get('model', 'N/A')}")
            
            messages = latest.get('messages', [])
            print(f"\nMessage flow ({len(messages)} messages):")
            for i, msg in enumerate(messages[:10], 1):  # First 10 messages
                if isinstance(msg, dict):
                    msg_type = msg.get('type', 'unknown')
                    content = msg.get('content', '')
                    if isinstance(content, str):
                        content_preview = content[:100] + "..." if len(content) > 100 else content
                    else:
                        content_preview = str(content)[:100]
                    
                    print(f"  {i}. [{msg_type}] {content_preview}")
                    
                    # Show tool calls if present
                    if msg_type == 'ai' and msg.get('tool_calls'):
                        for tc in msg.get('tool_calls', []):
                            print(f"      → Tool: {tc.get('name')}")
        
    except Exception as e:
        print(f"✗ Error analyzing traces: {e}")
        import traceback
        traceback.print_exc()

def main():
    print("=== Forge Agent Behavior Analysis ===\n")
    
    client = connect_to_mongo()
    if not client:
        return
    
    try:
        # List all databases
        dbs = list_databases(client)
        
        # Try common database names
        possible_dbs = ['forge_search', 'forge', 'admin', 'test']
        target_db = None
        
        for db_name in possible_dbs:
            if db_name in dbs:
                target_db = db_name
                break
        
        if not target_db and dbs:
            # Use first non-system database
            system_dbs = ['admin', 'local', 'config']
            for db in dbs:
                if db not in system_dbs:
                    target_db = db
                    break
        
        if target_db:
            explore_collections(client, target_db)
            analyze_traces(client, target_db)
        else:
            print("\n✗ No suitable database found for analysis")
            
    finally:
        client.close()
        print("\n✓ Connection closed")

if __name__ == "__main__":
    main()
