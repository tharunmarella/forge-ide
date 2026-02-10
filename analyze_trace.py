#!/usr/bin/env python3
"""
Analyze agent trace files to identify performance bottlenecks.
"""
import json
import sys
from pathlib import Path

def analyze_trace(trace_file):
    events = []
    with open(trace_file) as f:
        for line in f:
            if line.strip():
                events.append(json.loads(line))
    
    print(f"\n{'='*80}")
    print(f"TRACE ANALYSIS: {trace_file.name}")
    print(f"{'='*80}\n")
    
    # Calculate time spent in different phases
    api_calls = []
    tool_executions = []
    
    for i, event in enumerate(events):
        if event['event'] == 'completion_call':
            # Find the next event (tool_call or completion_response_finish)
            next_event_time = None
            for j in range(i + 1, len(events)):
                if events[j]['event'] in ['tool_call', 'completion_response_finish', 'error']:
                    next_event_time = events[j]['elapsed_s']
                    break
            
            if next_event_time:
                api_time = next_event_time - event['elapsed_s']
                api_calls.append({
                    'turn': event['turn'],
                    'time': api_time,
                    'prompt_len': event['data'].get('prompt_len', 0)
                })
        
        elif event['event'] == 'tool_result' and event.get('duration_s'):
            tool_executions.append({
                'turn': event['turn'],
                'tool': event['data'].get('tool', 'unknown'),
                'time': event['duration_s']
            })
    
    # Summary statistics
    total_time = events[-1]['elapsed_s'] if events else 0
    total_api_time = sum(call['time'] for call in api_calls)
    total_tool_time = sum(tool['time'] for tool in tool_executions)
    
    print(f"📊 SUMMARY")
    print(f"{'─'*80}")
    print(f"Total session time:        {total_time:.2f}s")
    print(f"Time in API calls:         {total_api_time:.2f}s ({total_api_time/total_time*100:.1f}%)")
    print(f"Time in tool execution:    {total_tool_time:.2f}s ({total_tool_time/total_time*100:.1f}%)")
    print(f"Other overhead:            {total_time - total_api_time - total_tool_time:.2f}s ({(total_time - total_api_time - total_tool_time)/total_time*100:.1f}%)")
    print(f"Number of turns:           {len(api_calls)}")
    print(f"Number of tool calls:      {len(tool_executions)}")
    
    print(f"\n⏱️  API CALL BREAKDOWN (slowest first)")
    print(f"{'─'*80}")
    print(f"{'Turn':<6} {'Time':<10} {'Prompt Length':<15} {'%'}")
    for call in sorted(api_calls, key=lambda x: x['time'], reverse=True):
        pct = call['time'] / total_time * 100
        print(f"{call['turn']:<6} {call['time']:<10.2f}s {call['prompt_len']:<15,} {pct:>5.1f}%")
    
    print(f"\n🔧 TOOL EXECUTION BREAKDOWN")
    print(f"{'─'*80}")
    tool_stats = {}
    for tool in tool_executions:
        name = tool['tool']
        if name not in tool_stats:
            tool_stats[name] = {'count': 0, 'total_time': 0}
        tool_stats[name]['count'] += 1
        tool_stats[name]['total_time'] += tool['time']
    
    print(f"{'Tool':<20} {'Count':<8} {'Total Time':<15} {'Avg Time'}")
    for tool, stats in sorted(tool_stats.items(), key=lambda x: x[1]['total_time'], reverse=True):
        avg = stats['total_time'] / stats['count']
        print(f"{tool:<20} {stats['count']:<8} {stats['total_time']:<15.3f}s {avg:.3f}s")
    
    print(f"\n🎯 KEY INSIGHTS")
    print(f"{'─'*80}")
    
    # Insight 1: API call overhead
    if total_api_time > total_tool_time * 5:
        print(f"⚠️  API calls are taking {total_api_time/total_tool_time:.1f}x more time than tools!")
        print(f"   This is the main bottleneck. Consider:")
        print(f"   - Using a faster model")
        print(f"   - Reducing prompt size")
        print(f"   - Caching context/schema information")
    
    # Insight 2: Turn count
    if len(api_calls) > 10:
        print(f"⚠️  High turn count ({len(api_calls)} turns) suggests the agent is exploring a lot")
        print(f"   Consider providing more focused prompts or better initial context")
    
    # Insight 3: Slow tools
    slow_tools = [t for t in tool_executions if t['time'] > 0.1]
    if slow_tools:
        print(f"⚠️  {len(slow_tools)} slow tool executions (>0.1s) detected")
        for tool in sorted(slow_tools, key=lambda x: x['time'], reverse=True)[:3]:
            print(f"   - {tool['tool']}: {tool['time']:.3f}s (turn {tool['turn']})")

if __name__ == '__main__':
    trace_dir = Path.home() / 'Library/Application Support/forge-ide/traces'
    
    if len(sys.argv) > 1:
        trace_file = Path(sys.argv[1])
    else:
        # Find most recent trace
        traces = sorted(trace_dir.glob('agent-*.jsonl'), key=lambda p: p.stat().st_mtime, reverse=True)
        if not traces:
            print(f"No trace files found in {trace_dir}")
            sys.exit(1)
        trace_file = traces[0]
    
    analyze_trace(trace_file)
