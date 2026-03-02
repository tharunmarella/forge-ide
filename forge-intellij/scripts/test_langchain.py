import asyncio
import os
from langchain_core.messages import HumanMessage, SystemMessage
from langchain_core.tools import tool
from langchain_litellm import ChatLiteLLM

@tool
def explore_codebase(question: str) -> str:
    """Fetch codebase context relevant to a question or task."""
    return "Dummy context"

async def test_langchain():
    print("Testing ChatLiteLLM with gemini-3-flash-preview and tools")
    api_key = os.getenv("GEMINI_API_KEY")
    if not api_key:
        print("Error: GEMINI_API_KEY not set")
        return

    model = ChatLiteLLM(
        model="gemini/gemini-3-flash-preview",
        temperature=0.1,
        streaming=True
    )
    model_with_tools = model.bind_tools([explore_codebase])

    messages = [
        SystemMessage(content="You are an AI assistant. You MUST use tools to explore the codebase before answering. You MUST write a sentence explaining what you are doing, AND ALSO call the tool in the same response. Do not emit bare tool calls with no text."),
        HumanMessage(content="What is this project about?")
    ]

    response = await model_with_tools.ainvoke(messages)
    print(f"Content: {response.content}")
    print(f"Tool calls: {response.tool_calls}")

if __name__ == "__main__":
    asyncio.run(test_langchain())
