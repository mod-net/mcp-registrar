#!/usr/bin/env python3
import argparse
import asyncio
import json
import os
import sys
from typing import Any, Dict, Optional
from dotenv import load_dotenv

# Load variables from a .env file if present
load_dotenv()

JSON = Dict[str, Any]

class MCPClient:
    def __init__(self, command: str, args: list[str] | None = None, cwd: Optional[str] = None) -> None:
        self.command = command
        self.args = args or []
        self.cwd = cwd
        # Align with Rust server expectations: OPENAI_API_KEY, OPENAI_BASE_URL, OPENAI_MODEL
        # Maintain backward compatibility by reading OPENAI_API_BASE/OPENAI_API_MODEL if set
        self.env = {
            "OPENAI_API_KEY": os.getenv("OPENAI_API_KEY"),
            "OPENAI_BASE_URL": os.getenv("OPENAI_BASE_URL") or os.getenv("OPENAI_API_BASE"),
            "OPENAI_MODEL": os.getenv("OPENAI_MODEL") or os.getenv("OPENAI_API_MODEL"),
        }
        self.proc: Optional[asyncio.subprocess.Process] = None
        self._id = 0
        self._pending: dict[int, asyncio.Future] = {}

    def _next_id(self) -> int:
        self._id += 1
        return self._id

    async def start(self) -> None:
        # Build environment for the child process. Only pass variables that are set
        merged_env = dict(os.environ)
        for key, value in (self.env or {}).items():
            if value:
                merged_env[key] = value

        self.proc = await asyncio.create_subprocess_exec(
            self.command,
            *self.args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            cwd=self.cwd,
            env=merged_env,
        )
        asyncio.create_task(self._read_stdout())
        asyncio.create_task(self._read_stderr())

    async def _read_stdout(self) -> None:
        assert self.proc and self.proc.stdout
        while True:
            raw_line = await self.proc.stdout.readline()
            if not raw_line:
                # process ended
                # complete all pending futures with error
                for future in self._pending.values():
                    if not future.done():
                        future.set_exception(RuntimeError("transport closed"))
                self._pending.clear()
                return
            try:
                message = json.loads(raw_line.decode("utf-8"))
            except Exception as e:
                # ignore malformed lines but log to stderr
                print(f"[client] failed to parse line: {raw_line!r}: {e}", file=sys.stderr)
                continue
            # handle JSON-RPC shapes: response or error
            if "id" in message and ("result" in message or "error" in message):
                request_id = message.get("id")
                future = self._pending.pop(request_id, None)
                if future is not None and not future.done():
                    if "error" in message:
                        future.set_exception(RuntimeError(json.dumps(message["error"])) )
                    else:
                        future.set_result(message["result"]) 
            else:
                # notifications or other messages; just print
                print(json.dumps(message))

    async def _read_stderr(self) -> None:
        assert self.proc and self.proc.stderr
        log_path = "/tmp/mcp_server_stderr.log"
        try:
            log_file = open(log_path, "a", buffering=1)
        except Exception:
            log_file = None
        while True:
            raw_line = await self.proc.stderr.readline()
            if not raw_line:
                if log_file:
                    log_file.close()
                return
            text_line = raw_line.decode("utf-8")
            sys.stderr.write(f"[server-stderr] {text_line}")
            if log_file:
                try:
                    log_file.write(text_line)
                except Exception:
                    pass

    async def request(self, method: str, params: Any) -> Any:
        assert self.proc and self.proc.stdin
        request_id = self._next_id()
        request_payload: JSON = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }
        future: asyncio.Future = asyncio.get_event_loop().create_future()
        self._pending[request_id] = future
        encoded_line = (json.dumps(request_payload) + "\n").encode("utf-8")
        self.proc.stdin.write(encoded_line)
        await self.proc.stdin.drain()
        return await future

    async def notify(self, method: str, params: Any) -> None:
        assert self.proc and self.proc.stdin
        notification_payload: JSON = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }
        encoded_line = (json.dumps(notification_payload) + "\n").encode("utf-8")
        self.proc.stdin.write(encoded_line)
        await self.proc.stdin.drain()

    async def initialize(self) -> JSON:
        # Use a protocol version compatible with the rmcp server's response
        init_params: JSON = {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "mcp-test-client", "version": "0.1.0"},
        }
        init_result = await self.request("initialize", init_params)
        # Send initialized notification as per MCP spec recommendation
        await self.notify("notifications/initialized", {})
        return init_result

    async def list_tools(self) -> JSON:
        # tools/list has optional params; omit params to maximize compatibility
        return await self.request("tools/list", {})

    async def call_tool(self, name: str, arguments: Optional[Dict[str, Any]] = None) -> JSON:
        call_params: JSON = {
            "name": name,
            "arguments": arguments or {},
        }
        return await self.request("tools/call", call_params)

    async def aclose(self) -> None:
        if self.proc:
            if self.proc.stdin:
                self.proc.stdin.close()
            try:
                await asyncio.wait_for(self.proc.wait(), timeout=1.0)
            except asyncio.TimeoutError:
                self.proc.kill()


async def main() -> None:
    parser = argparse.ArgumentParser(description="Minimal MCP JSON-RPC client over stdio (dotenv-enabled)")
    parser.add_argument("--command", required=True, help="Path to MCP server executable")
    parser.add_argument("server_args", nargs=argparse.REMAINDER, help="Arguments to pass to the server after --")
    parser.add_argument("--cwd", default=None, help="Working directory for the server")
    args = parser.parse_args()

    # Split separator -- from remaining args if present
    server_args_list: list[str] = []
    if args.server_args:
        server_args_list = args.server_args
        if server_args_list and server_args_list[0] == "--":
            server_args_list = server_args_list[1:]

    # Basic validation for readability
    if not os.getenv("OPENAI_API_KEY") and not os.getenv("OPENAI_API_BASE") and not os.getenv("OPENAI_BASE_URL"):
        print("[client] Note: OPENAI_* environment variables are not set in dotenv or environment.", file=sys.stderr)

    client = MCPClient(args.command, server_args_list, cwd=args.cwd)
    await client.start()

    try:
        init = await client.initialize()
        print(json.dumps({"initialize.result": init}, indent=2))

        # Small delay to avoid race with server post-initialize handling on some transports
        await asyncio.sleep(0.05)

        # First, list tools to verify dispatch after initialize; send explicit optional params
        tools = await client.request("tools/list", {"cursor": None, "limit": None})
        print(json.dumps({"tools.list.result": tools}, indent=2))

        # Then, call chat tool
        call_res = await client.call_tool(
            "chat.completions.create",
            {
                "messages": [
                    {"role": "system", "content": "You are helpful."},
                    {"role": "user", "content": "Say hello in one short sentence."},
                ]
            },
        )
        print(json.dumps({"tools.call.result": call_res}, indent=2))
    finally:
        await client.aclose()


if __name__ == "__main__":
    asyncio.run(main())
