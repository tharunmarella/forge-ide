#!/usr/bin/env python3
"""
Detailed analysis of Forge agent behavior from MongoDB
"""

from pymongo import MongoClient
from datetime import datetime, timedelta
import json
from collections import Counter, defaultdict

MONGO_URI = "mongodb://mongo:IYXQEyLlmzsXejgVDPcSQRwlYkzRcCXO@mainline.proxy.rlwy.net:20298"
DB_NAME = "forge_traces"

def analyze_traces():
    """Analyze agent traces in detail"""
    client = MongoClient(MONGO_URI, serverSelectionTimeoutMS=5000)
    db = client[DB_NAME]
    
    print("=" * 80)
    print("FORGE AGENT BEHAVIOR ANALYSIS")
    print("=" * 80)
    
    # ============ TRACES COLLECTION ============
    traces = db['traces']
    total_traces = traces.count_documents({})
    print(f"\n📊 Total traces: {total_traces}")
    
    # Time-based analysis
    yesterday = datetime.utcnow() - timedelta(days=1)
    week_ago = datetime.utcnow() - timedelta(days=7)
    
    recent_24h = traces.count_documents({"created_at": {"$gte": yesterday}})
    recent_7d = traces.count_documents({"created_at": {"$gte": week_ago}})
    
    print(f"  • Last 24 hours: {recent_24h} traces")
    print(f"  • Last 7 days: {recent_7d} traces")
    
    # Get recent traces for detailed analysis
    recent_traces = list(traces.find().sort("created_at", -1).limit(100))
    
    # ============ MODEL USAGE ============
    print("\n" + "=" * 80)
    print("🤖 MODEL USAGE")
    print("=" * 80)
    
    model_counts = Counter()
    for trace in recent_traces:
        model = trace.get('model', 'unknown')
        model_counts[model] += 1
    
    for model, count in model_counts.most_common():
        percentage = (count / len(recent_traces)) * 100
        print(f"  {model}: {count} ({percentage:.1f}%)")
    
    # ============ TOOL USAGE ============
    print("\n" + "=" * 80)
    print("🔧 TOOL USAGE STATISTICS")
    print("=" * 80)
    
    tool_counts = Counter()
    tool_success = defaultdict(lambda: {"success": 0, "failed": 0})
    total_tool_calls = 0
    
    for trace in recent_traces:
        messages = trace.get('messages', [])
        for msg in messages:
            if not isinstance(msg, dict):
                continue
            
            # AI messages with tool calls
            if msg.get('type') == 'ai' and msg.get('tool_calls'):
                for tc in msg.get('tool_calls', []):
                    tool_name = tc.get('name', 'unknown')
                    tool_counts[tool_name] += 1
                    total_tool_calls += 1
            
            # Tool result messages
            if msg.get('type') == 'tool':
                tool_name = msg.get('name', 'unknown')
                status = msg.get('status', 'success')
                if status == 'error' or 'error' in str(msg.get('content', '')).lower()[:100]:
                    tool_success[tool_name]['failed'] += 1
                else:
                    tool_success[tool_name]['success'] += 1
    
    print(f"\n📈 Total tool calls: {total_tool_calls}")
    print(f"\n🏆 Most Used Tools (Top 20):")
    for tool, count in tool_counts.most_common(20):
        percentage = (count / total_tool_calls) * 100 if total_tool_calls > 0 else 0
        success_count = tool_success[tool]['success']
        failed_count = tool_success[tool]['failed']
        total = success_count + failed_count
        success_rate = (success_count / total * 100) if total > 0 else 0
        
        print(f"  {tool:30} {count:4} calls ({percentage:5.1f}%) | Success rate: {success_rate:5.1f}%")
    
    # ============ ERROR ANALYSIS ============
    print("\n" + "=" * 80)
    print("❌ ERROR ANALYSIS")
    print("=" * 80)
    
    error_types = Counter()
    error_tools = Counter()
    
    for trace in recent_traces:
        messages = trace.get('messages', [])
        for msg in messages:
            if not isinstance(msg, dict):
                continue
            
            if msg.get('type') == 'tool' and msg.get('status') == 'error':
                tool_name = msg.get('name', 'unknown')
                error_tools[tool_name] += 1
                
                content = str(msg.get('content', ''))
                # Categorize errors
                if 'timeout' in content.lower():
                    error_types['Timeout'] += 1
                elif 'not found' in content.lower() or '404' in content:
                    error_types['Not Found'] += 1
                elif 'permission' in content.lower() or 'denied' in content.lower():
                    error_types['Permission Denied'] += 1
                elif 'connection' in content.lower():
                    error_types['Connection Error'] += 1
                else:
                    error_types['Other'] += 1
    
    if error_types:
        print("\n📊 Error Types:")
        for error_type, count in error_types.most_common():
            print(f"  {error_type}: {count}")
        
        print("\n🔧 Tools with Most Errors:")
        for tool, count in error_tools.most_common(10):
            print(f"  {tool}: {count} errors")
    else:
        print("  ✓ No errors found in recent traces")
    
    # ============ CONVERSATION PATTERNS ============
    print("\n" + "=" * 80)
    print("💬 CONVERSATION PATTERNS")
    print("=" * 80)
    
    msg_lengths = []
    turns_per_conversation = []
    
    for trace in recent_traces:
        messages = trace.get('messages', [])
        msg_lengths.append(len(messages))
        
        # Count user turns
        user_turns = sum(1 for msg in messages if isinstance(msg, dict) and msg.get('type') == 'human')
        turns_per_conversation.append(user_turns)
    
    if msg_lengths:
        avg_msgs = sum(msg_lengths) / len(msg_lengths)
        avg_turns = sum(turns_per_conversation) / len(turns_per_conversation)
        print(f"  • Average messages per conversation: {avg_msgs:.1f}")
        print(f"  • Average user turns per conversation: {avg_turns:.1f}")
        print(f"  • Shortest conversation: {min(msg_lengths)} messages")
        print(f"  • Longest conversation: {max(msg_lengths)} messages")
    
    # ============ LLM CALLS ANALYSIS ============
    print("\n" + "=" * 80)
    print("🧠 LLM CALLS ANALYSIS")
    print("=" * 80)
    
    llm_calls = db['llm_calls']
    total_llm_calls = llm_calls.count_documents({})
    print(f"\n📊 Total LLM calls: {total_llm_calls}")
    
    recent_llm = list(llm_calls.find().sort("timestamp", -1).limit(100))
    
    if recent_llm:
        # Model distribution
        llm_models = Counter()
        total_tokens = 0
        total_latency = 0
        latency_count = 0
        
        for call in recent_llm:
            model = call.get('model', 'unknown')
            llm_models[model] += 1
            
            # Token usage
            usage = call.get('usage', {})
            if isinstance(usage, dict):
                total_tokens += usage.get('total_tokens', 0)
            
            # Latency
            if 'latency_ms' in call:
                total_latency += call['latency_ms']
                latency_count += 1
        
        print(f"\n🤖 LLM Model Distribution:")
        for model, count in llm_models.most_common():
            percentage = (count / len(recent_llm)) * 100
            print(f"  {model}: {count} ({percentage:.1f}%)")
        
        if total_tokens > 0:
            avg_tokens = total_tokens / len(recent_llm)
            print(f"\n💰 Average tokens per call: {avg_tokens:.0f}")
        
        if latency_count > 0:
            avg_latency = total_latency / latency_count
            print(f"⚡ Average latency: {avg_latency:.0f}ms")
    
    # ============ SAMPLE RECENT CONVERSATION ============
    print("\n" + "=" * 80)
    print("📝 SAMPLE RECENT CONVERSATION")
    print("=" * 80)
    
    if recent_traces:
        latest = recent_traces[0]
        print(f"\nThread ID: {latest.get('thread_id', 'N/A')}")
        print(f"Created: {latest.get('created_at', 'N/A')}")
        print(f"Status: {latest.get('status', 'N/A')}")
        print(f"Model: {latest.get('model', 'N/A')}")
        
        messages = latest.get('messages', [])
        print(f"\nMessage flow ({len(messages)} total messages):\n")
        
        for i, msg in enumerate(messages[:20], 1):  # First 20 messages
            if not isinstance(msg, dict):
                continue
            
            msg_type = msg.get('type', 'unknown')
            
            if msg_type == 'human':
                content = msg.get('content', '')
                preview = content[:80] + "..." if len(content) > 80 else content
                print(f"  {i}. 👤 USER: {preview}")
            
            elif msg_type == 'ai':
                content = msg.get('content', '')
                tool_calls = msg.get('tool_calls', [])
                
                if tool_calls:
                    print(f"  {i}. 🤖 AI: [Calling {len(tool_calls)} tool(s)]")
                    for tc in tool_calls:
                        tool_name = tc.get('name', 'unknown')
                        print(f"       └─ {tool_name}")
                else:
                    preview = content[:80] + "..." if len(content) > 80 else content
                    print(f"  {i}. 🤖 AI: {preview}")
            
            elif msg_type == 'tool':
                tool_name = msg.get('name', 'unknown')
                status = msg.get('status', 'success')
                icon = "✓" if status == 'success' else "✗"
                print(f"  {i}. 🔧 TOOL ({icon}): {tool_name}")
    
    print("\n" + "=" * 80)
    print("Analysis complete!")
    print("=" * 80)
    
    client.close()

if __name__ == "__main__":
    try:
        analyze_traces()
    except Exception as e:
        print(f"\n❌ Error during analysis: {e}")
        import traceback
        traceback.print_exc()
