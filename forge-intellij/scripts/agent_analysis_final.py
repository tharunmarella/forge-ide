#!/usr/bin/env python3
"""
Comprehensive Forge Agent Behavior Analysis
"""

from pymongo import MongoClient
from datetime import datetime, timedelta
from collections import Counter, defaultdict
import json

MONGO_URI = "mongodb://mongo:IYXQEyLlmzsXejgVDPcSQRwlYkzRcCXO@mainline.proxy.rlwy.net:20298"
DB_NAME = "forge_traces"

def analyze():
    client = MongoClient(MONGO_URI, serverSelectionTimeoutMS=5000)
    db = client[DB_NAME]
    
    print("=" * 100)
    print(" " * 30 + "FORGE AGENT BEHAVIOR ANALYSIS")
    print("=" * 100)
    
    # ============ OVERVIEW ============
    traces = db['traces']
    llm_calls = db['llm_calls']
    
    total_traces = traces.count_documents({})
    total_llm_calls = llm_calls.count_documents({})
    
    print(f"\n📊 OVERVIEW")
    print(f"  • Total conversations: {total_traces}")
    print(f"  • Total LLM calls: {total_llm_calls}")
    
    # Time-based analysis
    now = datetime.utcnow()
    day_ago = now - timedelta(days=1)
    week_ago = now - timedelta(days=7)
    month_ago = now - timedelta(days=30)
    
    traces_24h = traces.count_documents({"timestamp": {"$gte": day_ago}})
    traces_7d = traces.count_documents({"timestamp": {"$gte": week_ago}})
    traces_30d = traces.count_documents({"timestamp": {"$gte": month_ago}})
    
    print(f"\n📅 ACTIVITY")
    print(f"  • Last 24 hours: {traces_24h} conversations")
    print(f"  • Last 7 days: {traces_7d} conversations")
    print(f"  • Last 30 days: {traces_30d} conversations")
    
    # Get recent traces for detailed analysis
    recent_traces = list(traces.find().sort("timestamp", -1).limit(500))
    
    # ============ PERFORMANCE METRICS ============
    print(f"\n" + "=" * 100)
    print("⚡ PERFORMANCE METRICS")
    print("=" * 100)
    
    execution_times = [t.get('execution_time_ms', 0) for t in recent_traces if t.get('execution_time_ms')]
    if execution_times:
        avg_time = sum(execution_times) / len(execution_times)
        min_time = min(execution_times)
        max_time = max(execution_times)
        median_time = sorted(execution_times)[len(execution_times)//2]
        
        print(f"  • Average response time: {avg_time:.0f}ms ({avg_time/1000:.1f}s)")
        print(f"  • Median response time: {median_time:.0f}ms ({median_time/1000:.1f}s)")
        print(f"  • Fastest response: {min_time:.0f}ms ({min_time/1000:.1f}s)")
        print(f"  • Slowest response: {max_time:.0f}ms ({max_time/1000:.1f}s)")
        
        # Response time distribution
        fast = sum(1 for t in execution_times if t < 5000)
        medium = sum(1 for t in execution_times if 5000 <= t < 15000)
        slow = sum(1 for t in execution_times if t >= 15000)
        
        print(f"\n  Response Time Distribution:")
        print(f"    < 5s  (Fast):   {fast:4} ({fast/len(execution_times)*100:.1f}%)")
        print(f"    5-15s (Medium): {medium:4} ({medium/len(execution_times)*100:.1f}%)")
        print(f"    > 15s (Slow):   {slow:4} ({slow/len(execution_times)*100:.1f}%)")
    
    # ============ TOOL USAGE ANALYSIS ============
    print(f"\n" + "=" * 100)
    print("🔧 TOOL USAGE ANALYSIS")
    print("=" * 100)
    
    tool_counts = Counter()
    total_tool_calls = 0
    traces_with_tools = 0
    
    for trace in recent_traces:
        tool_count = trace.get('tool_call_count', 0)
        if tool_count > 0:
            total_tool_calls += tool_count
            traces_with_tools += 1
    
    print(f"  • Conversations using tools: {traces_with_tools} / {len(recent_traces)} ({traces_with_tools/len(recent_traces)*100:.1f}%)")
    print(f"  • Total tool calls: {total_tool_calls}")
    if traces_with_tools > 0:
        print(f"  • Average tools per conversation (when used): {total_tool_calls/traces_with_tools:.1f}")
    
    # Tool call distribution
    tool_call_dist = Counter()
    for trace in recent_traces:
        tool_count = trace.get('tool_call_count', 0)
        tool_call_dist[tool_count] += 1
    
    print(f"\n  Tool Call Distribution:")
    for count in sorted(tool_call_dist.keys())[:10]:
        num_traces = tool_call_dist[count]
        print(f"    {count:2} tools: {num_traces:4} conversations ({num_traces/len(recent_traces)*100:.1f}%)")
    
    # ============ CONVERSATION PATTERNS ============
    print(f"\n" + "=" * 100)
    print("💬 CONVERSATION PATTERNS")
    print("=" * 100)
    
    message_counts = [t.get('message_count', 0) for t in recent_traces if t.get('message_count')]
    if message_counts:
        avg_msgs = sum(message_counts) / len(message_counts)
        print(f"  • Average messages per conversation: {avg_msgs:.1f}")
        print(f"  • Shortest conversation: {min(message_counts)} messages")
        print(f"  • Longest conversation: {max(message_counts)} messages")
    
    # Plan usage
    traces_with_plan = sum(1 for t in recent_traces if t.get('has_plan', False))
    print(f"\n  • Conversations using planning: {traces_with_plan} / {len(recent_traces)} ({traces_with_plan/len(recent_traces)*100:.1f}%)")
    
    # Status distribution
    status_counts = Counter(t.get('status', 'unknown') for t in recent_traces)
    print(f"\n  Status Distribution:")
    for status, count in status_counts.most_common():
        print(f"    {status:10}: {count:4} ({count/len(recent_traces)*100:.1f}%)")
    
    # Error analysis
    traces_with_errors = sum(1 for t in recent_traces if t.get('error'))
    if traces_with_errors > 0:
        print(f"\n  ⚠️  Conversations with errors: {traces_with_errors} ({traces_with_errors/len(recent_traces)*100:.1f}%)")
    
    # ============ REQUEST ANALYSIS ============
    print(f"\n" + "=" * 100)
    print("📝 REQUEST ANALYSIS")
    print("=" * 100)
    
    continuation_count = sum(1 for t in recent_traces if t.get('request', {}).get('is_continuation', False))
    with_files = sum(1 for t in recent_traces if t.get('request', {}).get('attached_files', 0) > 0)
    with_images = sum(1 for t in recent_traces if t.get('request', {}).get('attached_images', 0) > 0)
    needs_think = sum(1 for t in recent_traces if t.get('request', {}).get('needs_think', False))
    
    print(f"  • Continuation requests: {continuation_count} ({continuation_count/len(recent_traces)*100:.1f}%)")
    print(f"  • With attached files: {with_files} ({with_files/len(recent_traces)*100:.1f}%)")
    print(f"  • With attached images: {with_images} ({with_images/len(recent_traces)*100:.1f}%)")
    print(f"  • Requiring deep thinking: {needs_think} ({needs_think/len(recent_traces)*100:.1f}%)")
    
    # ============ LLM CALLS ANALYSIS ============
    print(f"\n" + "=" * 100)
    print("🧠 LLM CALLS ANALYSIS")
    print("=" * 100)
    
    recent_llm = list(llm_calls.find().sort("timestamp", -1).limit(500))
    
    if recent_llm:
        model_counts = Counter(c.get('model', 'unknown') for c in recent_llm)
        print(f"\n  Model Usage:")
        for model, count in model_counts.most_common():
            print(f"    {model}: {count} ({count/len(recent_llm)*100:.1f}%)")
        
        # Token analysis
        total_tokens = sum(c.get('usage', {}).get('total_tokens', 0) for c in recent_llm)
        input_tokens = sum(c.get('usage', {}).get('prompt_tokens', 0) for c in recent_llm)
        output_tokens = sum(c.get('usage', {}).get('completion_tokens', 0) for c in recent_llm)
        
        if total_tokens > 0:
            print(f"\n  Token Usage (last {len(recent_llm)} calls):")
            print(f"    Total: {total_tokens:,} tokens")
            print(f"    Input: {input_tokens:,} tokens ({input_tokens/total_tokens*100:.1f}%)")
            print(f"    Output: {output_tokens:,} tokens ({output_tokens/total_tokens*100:.1f}%)")
            print(f"    Average per call: {total_tokens/len(recent_llm):.0f} tokens")
        
        # Latency analysis
        latencies = [c.get('latency_ms', 0) for c in recent_llm if c.get('latency_ms')]
        if latencies:
            avg_latency = sum(latencies) / len(latencies)
            print(f"\n  LLM Latency:")
            print(f"    Average: {avg_latency:.0f}ms ({avg_latency/1000:.1f}s)")
            print(f"    Min: {min(latencies):.0f}ms")
            print(f"    Max: {max(latencies):.0f}ms")
    
    # ============ USER ACTIVITY ============
    print(f"\n" + "=" * 100)
    print("👥 USER ACTIVITY")
    print("=" * 100)
    
    workspace_counts = Counter(t.get('workspace_id', 'unknown') for t in recent_traces)
    user_counts = Counter(t.get('user_email', 'anonymous') for t in recent_traces)
    
    print(f"\n  Active Workspaces: {len(workspace_counts)}")
    print(f"  Top Workspaces:")
    for workspace, count in workspace_counts.most_common(10):
        print(f"    {workspace[:40]:40}: {count:4} conversations")
    
    print(f"\n  Active Users: {len(user_counts)}")
    print(f"  Top Users:")
    for user, count in user_counts.most_common(10):
        print(f"    {user[:40]:40}: {count:4} conversations")
    
    # ============ SAMPLE CONVERSATIONS ============
    print(f"\n" + "=" * 100)
    print("📖 SAMPLE RECENT CONVERSATIONS")
    print("=" * 100)
    
    for i, trace in enumerate(recent_traces[:5], 1):
        print(f"\n{i}. Thread: {trace.get('thread_id', 'N/A')}")
        print(f"   Workspace: {trace.get('workspace_id', 'N/A')}")
        print(f"   Time: {trace.get('timestamp', 'N/A')}")
        print(f"   Duration: {trace.get('execution_time_ms', 0)/1000:.1f}s")
        print(f"   Messages: {trace.get('message_count', 0)}")
        print(f"   Tool calls: {trace.get('tool_call_count', 0)}")
        print(f"   Status: {trace.get('status', 'N/A')}")
        
        question = trace.get('request', {}).get('question', 'N/A')
        question_preview = question[:100] + "..." if len(question) > 100 else question
        print(f"   Question: {question_preview}")
        
        answer = trace.get('response', {}).get('answer', 'N/A')
        answer_preview = answer[:100] + "..." if len(answer) > 100 else answer
        print(f"   Answer: {answer_preview}")
    
    print(f"\n" + "=" * 100)
    print("Analysis Complete!")
    print("=" * 100)
    
    client.close()

if __name__ == "__main__":
    try:
        analyze()
    except Exception as e:
        print(f"\n❌ Error: {e}")
        import traceback
        traceback.print_exc()
