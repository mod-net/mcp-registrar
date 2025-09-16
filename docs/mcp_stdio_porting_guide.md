# MCP Stdio Server Porting Guide (Rust)

This guide documents the sharp edges and steps we took to get a robust, line-delimited stdio MCP server working in Rust. It’s intended as a repeatable playbook for future MCP servers.

## Summary
- We replaced the rmcp stdio service with a custom newline-delimited JSON-RPC loop in Rust to match the framing/behavior of our working Python implementation.
- We still use `rmcp::model` structs for rigid, spec-aligned response shapes, but we own the transport and lifecycle.
- The server now reliably handles `initialize`, `notifications/initialized`, `tools/list`, and `tools/call`.

## Symptoms We Saw
- Server responded to `initialize`, then the transport closed immediately on the next request (`tools/list` or `tools/call`).
- No server logs for `list_tools` or `call_tool` were observed after `initialize`.
- Client stack traces: `RuntimeError: transport closed` or `ConnectionResetError: Connection lost` on drain.

## Likely Root Cause
- Framing/lifecycle mismatch between our client and the rmcp stdio service:
  - The service terminated the session after `initialize` on the second inbound frame.
  - We never reached our handler logs for `tools/list` / `tools/call`, implying an upstream service close (pre-dispatch).

## Investigation Timeline
1. Verified stdout vs stderr separation. Ensured all logs go to stderr (and `/tmp/textgen_mcp.log`), stdout only for JSON-RPC frames.
2. Matched request shapes (camelCase, MCP version). Tried adding delays and explicit params (`{"cursor": null, "limit": null}`) after `initialize` to avoid races.
3. Added `on_initialized` hook/logging, panic hook, and error logging around `serve_server`.
4. Still closed on second frame. Conclusion: own the stdio loop to eliminate transport variability.
5. Implemented custom newline-delimited stdio loop. Success end-to-end.

## Final Architecture (Rust)
- File: `submodules/text-generator/src/bin/mcp_server.rs`
- Transport: custom loop using `tokio`:
  - Read `stdin` lines via `BufReader::lines()`.
  - For each non-empty line: `serde_json::from_str` → route by `method`.
  - Write exactly one JSON line to `stdout` (`write_all` + `flush`).
- Methods implemented:
  - `initialize` → returns `rmcp::model::ServerInfo` (serialized to `Value`).
  - `notifications/initialized` → no response, log only.
  - `tools/list` → returns `rmcp::model::ListToolsResult` with the tool `chat.completions.create` and a JSON schema (`chat_tool_schema()`).
  - `tools/call` → if `name == "chat.completions.create"`, forward to OpenAI-compatible backend; return `rmcp::model::CallToolResult::success|error`.
- Logging:
  - All diagnostics via `eprintln!` and `/tmp/textgen_mcp.log`.
  - Never write logs to stdout.

## Framing (Critical)
- One request or response per line.
- Newline terminator (`\n`) is required.
- Always flush stdout after writing a frame.
- Keep `stdin`/`stdout` open for the entire session.

## JSON-RPC Shapes (Minimum Set)
- initialize (request):
  - method: `"initialize"`
  - params: `{ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "...", "version": "..."} }`
- initialize (response):
  - result: `ServerInfo` from `rmcp::model`
- notifications/initialized:
  - method: `"notifications/initialized"` (no response)
- tools/list:
  - method: `"tools/list"`, params optional
  - result: `ListToolsResult` (tools array + `next_cursor`)
- tools/call:
  - method: `"tools/call"`, params: `{ "name": string, "arguments": object }`
  - result: `CallToolResult` (`content` array; `is_error` flag)

## Rigid Data Shapes via rmcp Models
- We construct results using `rmcp::model` (e.g., `ServerInfo`, `Tool`, `ListToolsResult`, `CallToolResult`) and serialize to JSON with `serde_json::to_value(...)` before writing.
- This enforces spec compliance while keeping control of transport.

## Client Considerations
- Use dotenv to load env vars (`OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`).
- Client must write exactly one line per JSON-RPC message and flush.
- Avoid accidental stdout logging—reserve it for protocol frames; logs should go to stderr.

## Gotchas / Sharp Edges Checklist
- __Framing__: newline-delimited JSON; flush after every write; single line per frame.
- __Streams__: keep `stdin`/`stdout` open; do not close `stdin` until process exit.
- __Logging separation__: never write logs to stdout; use stderr/file.
- __Protocol version__: match the server’s supported MCP version (`2024-11-05` in our case).
- __Request shapes__: exact method names (`tools/list`, `tools/call`) and param structures.
- __Initialize notification__: the client should send `notifications/initialized` after `initialize`.
- __Backpressure__: use async writes and flush; avoid buffering issues.
- __Schema__: provide valid JSON schema for tools; consider validating input.

## How to Run (Quick Test)
- Build:
  - `cargo build --release --manifest-path submodules/text-generator/Cargo.toml`
- Run client:
  - `uv run scripts/mcp_client.py --command ./submodules/text-generator/target/release/mcp_server`
- Set env (dotenv or shell):
  - `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`

Expected output (abridged):
```
initialize.result → protocolVersion 2024-11-05
tools.list.result → contains tool "chat.completions.create"
tools.call.result → content text contains OpenAI JSON
```

## Future Improvements
- __Schema validation__: validate `tools/call` `arguments` against `chat_tool_schema()`; return structured validation errors via `CallToolResult::error`.
- __More methods__: add prompts/resources if needed; follow same pattern.
- __Tracing__: optional feature gating to turn on structured tracing without polluting stdout.
- __Pagination__: support `cursor`/`limit` in `tools/list`.

## Files of Interest
- Server (Rust): `submodules/text-generator/src/bin/mcp_server.rs`
- Client (Python): `scripts/mcp_client.py`
- Python reference transport: `mcp-registry/mcp_servers/transport/stdio.py` (newline framing behavior)
- Registry HTTP server (reference): `mcp-registry/mcp_servers/registry_server.py`

---

This guide should let you reproduce a robust stdio MCP server in Rust with precise control over transport and rigid data shapes, avoiding the class of early-close issues we hit.
