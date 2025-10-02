---
description: Registry Scheduler Architecture Overview
---

# Registry Scheduler Architecture

## 1. Purpose and High-Level Topology
- **MCP registry stack**: Provides discovery, invocation, and scheduling services for Model Context Protocol tools. The crate exposes multiple servers that share a common transport contract and data model layer (`src/lib.rs`).
- **Key actors**:
  - **Registrar** (`src/servers/mcp_registrar.rs`): directory of MCP servers.
  - **Tool Registry** (`src/servers/tool_registry.rs`): catalogues tools, executes them via process/Wasm runtimes.
  - **Prompt Registry** (`src/servers/prompt_registry.rs`) and **Resource Registry** (`src/servers/resource_registry.rs`): manage prompt templates and external resources.
  - **Task Scheduler** (`src/servers/task_scheduler.rs`): orchestrates higher-level jobs using registered tools.
  - **MCP Gateway** (`src/bin/mcp_gateway.rs`): exposes Tool Registry functionality over MCP JSON-RPC.

## 2. Workspace Layout
- **Library modules** (`src/`):
  - `models/` – shared data types such as `Tool`, `ServerInfo`, `Resource`, `Prompt`, `Task`.
  - `servers/` – server implementations and supporting runtimes.
  - `transport/` – pluggable transports with the `McpServer` trait (`src/transport/mcpserver.rs`).
  - `utils/` – supporting utilities (configuration, storage, chain helpers).
  - `monitoring.rs` – execution metrics helpers.
- **Binaries** (`src/bin/`): entrypoints combining servers with transports. Examples:
  - `mcp_registrar.rs` (stdio/HTTP JSON-RPC).
  - `mcp_gateway.rs` (stdio MCP façade for tools/prompts/resources).
  - `tool_registry.rs`, `prompt_registry.rs`, `resource_registry.rs`, `task_scheduler.rs` (standalone servers for testing/dev flows).
  - `mcp_registrar_client.rs` (HTTP client for registrar JSON-RPC methods).
- **Scripts** (`scripts/`): Python stdio client (`mcp_client.py`) and helper SDK.
- **Submodules** (`submodules/`): optional example MCP servers such as `text-generator`.

## 3. Transport Layer
- **`McpServer` trait** (`src/transport/mcpserver.rs`): async handler signature used by all servers.
- **Stdio transport** (`src/transport/stdio_transport.rs`): newline-delimited JSON processing. Used by most binaries (e.g., `mcp_registrar.rs`, `mcp_gateway.rs`).
- **HTTP transport** (`src/transport/http_transport.rs`): Axum-based JSON-RPC endpoint at `/rpc` with health check. Added as optional layer for the registrar.
- **Other transports**: `sse_transport.rs` and `middleware.rs` exist for future expansion but are not wired into binaries by default.

## 4. Server Implementations
- **Registrar (`src/servers/mcp_registrar.rs`)**
  - Maintains in-memory `ServerInfo` catalog (`src/models/server.rs`).
  - Supports methods: `RegisterServer`, `ListServers`, `GetServer`, `UnregisterServer`, `UpdateServerStatus`, `Heartbeat`.
  - Optional autodetect path (`MCP_REGISTRAR_AUTODETECT`) invokes `server_loader::scan_and_load_servers("submodules")` to discover local projects. Currently populates placeholder endpoints; blockchain discovery is not yet wired.
  - HTTP/stdio transports configured in `src/bin/mcp_registrar.rs` via CLI flags.

- **Tool Registry (`src/servers/tool_registry.rs`)**
  - Persists tools using `utils::tool_storage::FileToolStorage` (`tools.json`).
  - Manages manifest-backed `StoredManifest` entries with runtime/policy metadata from `tool_runtime::manifest`.
  - Supports registering servers/tools, listing, invocation, and deletion.
  - Execution flow: resolves runtime config, validates schemas (via `jsonschema`), executes using `ProcessExecutor` (`src/servers/tool_runtime/executors/process.rs`) or `WasmExecutor` (`src/servers/tool_runtime/executors/wasm.rs`).
  - Integrates metrics via `monitoring::TOOL_METRICS` when invoked through `mcp_gateway`.

- **Prompt Registry (`src/servers/prompt_registry.rs`)**
  - Stores prompt templates keyed by server ID with optional variables schema.
  - Offers MCP methods for registration, retrieval, listing, rendering, and server registration for endpoint tracking.

- **Resource Registry (`src/servers/resource_registry.rs`)**
  - Catalogues resources associated with server IDs. Handles query validation and, if the stored endpoint is HTTP(S), forwards the query using a shared `reqwest::Client`.
  - On network failures, returns deterministic dummy data to keep tests stable.

- **Task Executor/Scheduler (`src/servers/task_executor.rs`, `src/servers/task_scheduler.rs`)**
  - `TaskExecutor` (not shown here) wraps tool invocations with metrics tracking.
  - `TaskSchedulerServer` persists `Task` objects via `utils::task_storage::FileTaskStorage` and can invoke tools through a pluggable `ToolInvoker` trait (`src/servers/tool_invoker.rs`).
  - Provides MCP methods for CRUD operations on tasks, status updates, and cancellation.

- **Text Generator (`src/servers/text_generator.rs` & `submodules/text-generator/`)**
  - Example MCP server demonstrating stdio framing. The submodule binary `mcp_server.rs` echoes OpenAI-compatible chat completion responses.

## 5. Runtime Abstractions (`src/servers/tool_runtime/`)
- **`ToolRuntime` enum**: distinguishes process vs. Wasm execution with configuration structs.
- **`Policy` struct**: encapsulates resource limits (timeouts, memory, CPU, output size, network rules).
- **`Executor` trait**: executed by Process and Wasm backends. The current Wasm executor is a stub pending full wasmtime integration.
- **Manifest loader**: `manifest::load_manifests()` scans `tools/**/tool.json` to hydrate `LoadedTool`, turning manifests into `Tool` objects consumed by the registry.

## 6. Data Models (`src/models/`)
- **`tool.rs`**: Defines `Tool`, `ToolInvocation`, `ToolInvocationResult`, plus validation helpers.
- **`server.rs`**: `ServerInfo` with heartbeat tracking and `ServerStatus` enum.
- **`prompt.rs`, `resource.rs`, `task.rs`**: Provide structured models used by their respective servers, including validation logic for arguments and queries.

## 7. Storage and Utilities (`src/utils/`)
- **`tool_storage.rs`**: `ToolStorage` trait with file-backed implementation used by the Tool Registry.
- **`task_storage.rs`**: Supports persisted task state for the scheduler.
- **`chain.rs` & related binaries**: Provide helper functions for resolving `chain://` module IDs via local JSON files or on-chain RPC. These utilities are not yet integrated into the registrar auto-registration flow.
- **`config.rs`, `logging.rs`, `error.rs`**: Shared helpers for configuration management, logging setup, and error types.

## 8. Monitoring (`src/monitoring.rs`)
- Collects metrics around tool invocations (invocation count, error count, duration, byte counts). `mcp_gateway.rs` exposes `metrics/get` to surface snapshots to clients.

## 9. Entry Points and Clients
- **`mcp_gateway.rs`**: Runs `ToolRegistryServer`, `PromptRegistryServer`, `ResourceRegistryServer` under a stdio JSON-RPC loop. Implements MCP methods (`initialize`, `tools/list`, `tools/call`, `prompts/*`, `resources/*`, `metrics/get`).
- **`mcp_registrar.rs`**: CLI for the registrar with stdio/HTTP transport selection.
- **`tool_registry.rs`, `prompt_registry.rs`, `resource_registry.rs`, `task_scheduler.rs`**: Provide standalone binaries for integration tests or manual use.
- **`mcp_registrar_client.rs`**: HTTP client that maps CLI subcommands to registrar JSON-RPC methods and prints JSON results.
- **Python clients**:
  - `scripts/mcp_client.py` – Async stdio JSON-RPC client for smoke testing MCP servers.
  - `scripts/mcp_client_sdk.py` – higher-level helpers used by tests or automation.

## 10. Data Flows
- **Tool invocation via MCP Gateway**:
  1. Client calls `initialize`/`tools/list`/`tools/call` over stdio (`scripts/mcp_client.py`).
  2. `mcp_gateway.rs` delegates to `ToolRegistryServer::list_tools` or `handle("InvokeTool", ...)`.
  3. Tool Registry loads manifest runtime, executes via Process/Wasm executor, validates schemas, persists metrics.
  4. Results are wrapped into MCP `content`/`isError` shape before returning.

- **Registrar discovery**:
  1. Manual registration uses JSON-RPC (`RegisterServer`).
  2. Autodetect (if `MCP_REGISTRAR_AUTODETECT` set) scans `submodules/` via `server_loader.rs` for local binaries. No chain integration yet; endpoints are placeholders.
  3. HTTP transport exposes the same methods at `/rpc` for REST-like usage.

- **Prompt & Resource consumption**:
  - Registries enforce server registration before prompts/resources can be added.
  - Prompt rendering returns `PromptRenderResult` with textual output.
  - Resource queries optionally proxy to remote HTTP endpoints or return simulated data on failure.

- **Task scheduling**:
  - Tasks persisted to disk (`tasks.json`) with metrics integrated.
  - Scheduler uses `ToolInvoker` abstraction to trigger tool execution, allowing substitution with real Tool Registry clients.

## 11. Chain Integration Status
- `utils/chain.rs` and CLI tools (`src/bin/register_module.rs`, `src/bin/query_module.rs`, `src/bin/publish_module.rs`) implement resolution of `chain://` module IDs using local JSON indices or Substrate RPC endpoints (`CHAIN_INDEX_FILE`, `CHAIN_INDEX_URL`, `CHAIN_RPC_URL`).
- At present, the registrar does **not** consume these helpers automatically. The roadmap includes wiring chain discovery outputs (endpoint metadata, manifests) into `McpRegistrarServer::new()` and Tool Registry initialization.

## 12. Testing and Tooling
- **Tests** (`tests/`): cover transport framing, registry behavior, scheduler persistence, and resource/task execution.
- **Logs**: sample log files in repo root (`*_registry.log`) facilitate manual inspection when running binaries.

## 13. Future Considerations
- Align registrar autodetection with chain metadata ingestion to populate real endpoints.
- Finish Wasm executor implementation, including fuel/memory limits.
- Harden JSON schema validation and unify across modules (see TODOs in `tool_registry.rs`).
- Document authentication / security expectations for HTTP transports before production use.

This document supersedes ad-hoc notes in `MANIFEST.md` by focusing on runtime architecture, inter-module relationships, and current system behavior.
