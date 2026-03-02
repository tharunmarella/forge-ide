import asyncio
import os
import sys

sys.path.append("/Users/tharun/Documents/projects/Forge/forge-search")
from app.core import llm as llm_provider
from app.core.agent import ALL_TOOLS, AGENT_SYSTEM_PROMPT, MASTER_PLANNING_PROMPT
from langchain_core.messages import HumanMessage, SystemMessage

async def test_full_agent_prompt():
    print(f"Testing gemini-3-flash-preview with {len(ALL_TOOLS)} tools")
    api_key = os.getenv("GEMINI_API_KEY")
    
    model = llm_provider.get_chat_model("gemini/gemini-3-flash-preview", temperature=0.1)
    model_with_tools = model.bind_tools(ALL_TOOLS)

    messages = [
        SystemMessage(content=AGENT_SYSTEM_PROMPT),
        SystemMessage(content=MASTER_PLANNING_PROMPT),
        SystemMessage(content="## Getting Started\nCall `explore_codebase(question)` with the user's task to get full codebase context before making any changes."),
        HumanMessage(content="what is this project about ?")
    ]

    print("Invoking model...")
    response = await model_with_tools.ainvoke(messages)
    print(f"\nContent: {response.content}")
    print(f"Tool calls count: {len(response.tool_calls)}")
    if response.tool_calls:
        for tc in response.tool_calls:
            print(f"  - {tc['name']}")

if __name__ == "__main__":
    asyncio.run(test_full_agent_prompt())
