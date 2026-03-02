import asyncio
import os
from litellm import acompletion

async def test_litellm():
    print("Testing litellm directly with gemini-3-flash-preview and tools")
    api_key = os.getenv("GEMINI_API_KEY")
    if not api_key:
        print("Error: GEMINI_API_KEY not set")
        return

    tools = [
        {
            "type": "function",
            "function": {
                "name": "explore_codebase",
                "description": "Fetch codebase context relevant to a question or task.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "The question to explore"
                        }
                    },
                    "required": ["question"]
                }
            }
        }
    ]

    messages = [
        {"role": "system", "content": "You are an AI assistant. You MUST use tools to explore the codebase before answering."},
        {"role": "user", "content": "What is this project about?"}
    ]

    response = await acompletion(
        model="gemini/gemini-3-flash-preview",
        messages=messages,
        tools=tools,
        temperature=0.1,
    )
    
    msg = response.choices[0].message
    print(f"Content: {msg.content}")
    print(f"Tool calls: {msg.tool_calls}")

if __name__ == "__main__":
    asyncio.run(test_litellm())
