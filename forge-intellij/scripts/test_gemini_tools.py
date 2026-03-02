import asyncio
import os
from langchain_core.messages import HumanMessage, SystemMessage
from langchain_core.tools import tool

# Set the api key to the workspace one if it's available
import sys
sys.path.append("/Users/tharun/Documents/projects/Forge/forge-search")
from app.core import llm as llm_provider

@tool
def explore_codebase(question: str) -> str:
    """Fetch codebase context relevant to a question or task."""
    return "Dummy context"

async def test_gemini():
    print("Testing gemini-3-flash-preview with tool calling")
    model = llm_provider.get_chat_model("gemini/gemini-3-flash-preview", temperature=0.1)
    model_with_tools = model.bind_tools([explore_codebase])
    
    messages = [
        SystemMessage(content="You are an AI assistant. You MUST use tools to explore the codebase before answering."),
        HumanMessage(content="What is this project about?")
    ]
    
    response = await model_with_tools.ainvoke(messages)
    print(f"Content: {response.content}")
    print(f"Tool calls: {response.tool_calls}")

if __name__ == "__main__":
    asyncio.run(test_gemini())
