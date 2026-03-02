import asyncio
import os
import sys

sys.path.append("/Users/tharun/Documents/projects/Forge/forge-search")
from app.core import llm as llm_provider
from app.core.prompts import MASTER_PLANNING_PROMPT, AGENT_SYSTEM_PROMPT
from langchain_core.messages import HumanMessage, SystemMessage
from langchain_core.tools import tool

@tool
def explore_codebase(question: str) -> str:
    """Fetch codebase context."""
    return "ok"

async def test():
    model = llm_provider.get_chat_model("gemini/gemini-3-flash-preview", temperature=0.1)
    model_with_tools = model.bind_tools([explore_codebase])
    
    messages = [
        SystemMessage(content=AGENT_SYSTEM_PROMPT),
        SystemMessage(content=MASTER_PLANNING_PROMPT),
        SystemMessage(content="## Project Profile: Not Yet Available\n\nBefore making any changes, read the project's key config files to understand the tech stack:\n- `read_file(\"package.json\")` or `read_file(\"Cargo.toml\")` or `read_file(\"requirements.txt\")`\n- Check framework config, styling config (tailwind.config.*, globals.css)\n- Check for version conflicts between dependencies\n\nThis prevents writing code that's incompatible with the project's environment."),
        SystemMessage(content="## Getting Started\n\nCall `explore_codebase(question)` with the user's task to get full codebase context (project structure, relevant files, symbols, call chains) before making any changes."),
        HumanMessage(content="what is this project about ?")
    ]
    
    response = await model_with_tools.ainvoke(messages)
    print("--- CONTENT ---")
    print(response.content)
    print("--- TOOL CALLS ---")
    print(response.tool_calls)

if __name__ == "__main__":
    asyncio.run(test())
