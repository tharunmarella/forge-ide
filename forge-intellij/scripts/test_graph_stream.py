import asyncio
import os
import sys

sys.path.append("/Users/tharun/Documents/projects/Forge/forge-search")
from app.core import agent
from langchain_core.messages import HumanMessage

async def test_stream():
    messages = [HumanMessage(content="what is this project about? please use explore_codebase")]
    state = {
        "messages": messages,
        "workspace_id": "test",
        "attached_files": {},
        "attached_images": [],
        "project_profile": "",
        "plan_steps": [],
        "current_step": 0,
        "changed_files": [],
        "changed_symbols": [],
        "verify_retry_count": 0,
        "current_phase": "",
        "verify_error_context": ""
    }
    
    config = {"configurable": {"thread_id": "123"}}
    
    print("Testing astream with stream_mode='messages'")
    final_messages = []
    
    async for chunk in agent.forge_agent.astream(state, config=config, stream_mode="messages"):
        msg, metadata = chunk
        node_name = metadata.get("langgraph_node", "")
        if node_name == "agent":
            if msg not in final_messages:
                final_messages.append(msg)
                
    last_msg = final_messages[-1]
    print(f"Content: {last_msg.content}")
    print(f"Tool calls: {getattr(last_msg, 'tool_calls', 'No tool calls attr')}")

if __name__ == "__main__":
    asyncio.run(test_stream())
