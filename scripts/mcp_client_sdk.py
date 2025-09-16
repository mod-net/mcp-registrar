#!/usr/bin/env python3
import asyncio
import json
import os
import sys
from typing import Any

try:
    from mcp import ClientSession, StdioServerParameters
    from mcp.client.stdio import stdio_client
except Exception as e:
    print("mcp SDK not installed. Install with: uv add 'mcp[cli]'", file=sys.stderr)
    raise

async def main() -> None:
    # Build stdio server parameters
    cmd = os.environ.get("MCP_CMD") or sys.argv[1]
    args = sys.argv[2:]

    server = StdioServerParameters(command=cmd, args=args)

    async with stdio_client(server) as (read, write):
        async with ClientSession(read, write) as session:
            info = await session.initialize()
            print(json.dumps({"initialize.result": info.model_dump(mode="json")}, indent=2))

            tools = await session.list_tools()
            print(json.dumps({"tools.list.result": tools.model_dump(mode="json")}, indent=2))

            # Try calling our chat tool if present
            names = {t.name for t in tools.tools}
            if "chat.completions.create" in names:
                res = await session.call_tool(
                    name="chat.completions.create",
                    arguments={
                        "messages": [
                            {"role": "system", "content": "You are helpful."},
                            {"role": "user", "content": "Say hello in one short sentence."},
                        ]
                    },
                )
                print(json.dumps({"tools.call.result": res.model_dump(mode="json")}, indent=2))

if __name__ == "__main__":
    asyncio.run(main())
