# MCP Registry Scheduler

A registry and task scheduler for the Model Context Protocol (MCP). This project implements a modular network of composable MCP servers, each acting as a self-contained unit with a defined role.

## Project Structure

- `src/servers/` - MCP server implementations

  - `mcp_registrar.rs` - Central service directory
  - `tool_registry.rs` - Tool registration and invocation
  - `resource_registry.rs` - Resource registration and access
  - `prompt_registry.rs` - Prompt template storage
  - `task_scheduler.rs` - Task scheduling and execution

- `src/models/` - Data models

  - `task.rs` - Task model and status
  - `server.rs` - Server information model
  - `tool.rs` - Tool model
  - `resource.rs` - Resource model
  - `prompt.rs` - Prompt model

- `src/transport/` - Transport layer implementations

  - `stdio_transport.rs` - Stdio-based transport
  - `sse_transport.rs` - Server-Sent Events transport
  - `middleware.rs` - Transport middleware like CORS

- `src/cli/` - Command-line interface

  - `cli_parser.rs` - CLI argument parsing

- `src/utils/` - Utilities
  - `error.rs` - Error handling
  - `logging.rs` - Logging utilities
  - `config.rs` - Configuration management

## Getting Started

### Prerequisites

- Rust (edition 2021)
- Dependencies as specified in Cargo.toml

### Building the Project

```bash
cargo build
```

### Running a Server

```bash
# Start the MCP Registrar server over stdio only
cargo run --bin mcp-registrar

# Start the Task Scheduler server
cargo run -- start-task-scheduler

# Start the Tool Registry server
cargo run -- start-tool-registry

# Start the Resource Registry server
cargo run -- start-resource-registry

# Start the Prompt Registry server
cargo run -- start-prompt-registry
```

To expose the registrar over HTTP JSON-RPC in addition to stdio, pass `--http-addr` and optionally disable stdio:

```bash
# Stdio + HTTP (listens on 127.0.0.1:8080)
RUST_LOG=info cargo run --bin mcp-registrar -- --http-addr 127.0.0.1:8080

# HTTP only
RUST_LOG=info cargo run --bin mcp-registrar -- --http-addr 127.0.0.1:8080 --no-stdio
```

An accompanying CLI client, `mcp-registrar-client`, issues the registrar's JSON-RPC commands over HTTP:

```bash
# List servers (defaults to http://127.0.0.1:8080)
cargo run --bin mcp-registrar-client -- list-servers

# Register a server via HTTP
cargo run --bin mcp-registrar-client -- register-server \
  --name text-generator \
  --description "Text generator MCP server" \
  --version 0.1.0 \
  --capabilities tools \
  --endpoint http://127.0.0.1:9000
```

## Architecture

See the [ARCHITECTURE.md](ARCHITECTURE.md) document for detailed information on the architecture and design principles of this project.

## MCP Tool Behavior

- tools/list exposes each tool with `inputSchema` (JSON Schema) derived from the registry’s parameter schema.
- tools/call returns MCP-native results in the form:

```json
{
  "content": [
    { "type": "text", "text": "..." }
    // or { "type": "json", "json": { /* payload */ } }
  ],
  "isError": false
}
```

If a tool prints a single JSON line that is not already MCP content, the gateway wraps it as `{ type: "json" }` content.

## Tool Runtimes and Contract

The Tool Registry executes tools via process-based runtimes:

- `process` — run any executable with fixed command/args.
- `python-uv-script` — run a single-file Python script via `uv run` using PEP 723 metadata for dependencies.
- `binary` — run a native binary.

All runtimes use the same stdin/stdout contract:

- Stdin (one line): `{"arguments": <params>}`
- Stdout (one line): either MCP content `{ content: [...], isError }` or any JSON value (wrapped as `{type:"json"}`).

Policies (timeouts, memory, output bytes, network) are configurable per manifest.

## Scaffolding Modules

You can scaffold modules under `tools/<name>/` with the `registry-scheduler` binary:

```bash
# Python single-file script (uv-managed)
cargo run -- scaffold-module \
  --name echo_python \
  --runtime python-uv-script \
  --version 0.1.0 \
  --description "Echo tool via Python+uv" \
  --categories example,python \
  --deps "orjson"

# Native binary
cargo run -- scaffold-module \
  --name echo_bin \
  --runtime binary \
  --version 0.1.0 \
  --description "Echo tool via native binary" \
  --categories example,binary \
  --command /usr/local/bin/echo \
  --args "-n"
```

This generates `tools/<name>/tool.json` and the appropriate script/entry. For `python-uv-script`, ensure [`uv`](https://github.com/astral-sh/uv) is installed and on PATH. The script’s PEP 723 header declares dependencies; you can edit it or use `uv add --script tools/<name>/<name>.py <dep>`.

## Example Tools

This repo includes examples:

- `tools/echo/` — simple process-based Python tool that returns MCP text content.
- `tools/echo_python/` — single-file Python script runnable via `uv` (not required for tests).

Start the Tool Registry and it will auto-load `tools/**/tool.json` on startup.

## Chain Module Resolution

Tools and WASM modules can be referenced via `chain://<module_id>`. The resolver supports multiple backends configured via environment variables (checked in this order):

- `CHAIN_INDEX_FILE`: Path to a local JSON index used in tests/offline.
  - Supported shapes:
    - Object map: `{ "<module_id>": { ModulePointer... }, ... }`
    - Wrapped map: `{ "modules": { "<module_id>": { ... } } }`
    - Array: `[ { ModulePointer... }, ... ]` (each item must include `module_id`)
- `CHAIN_INDEX_URL`: HTTP index base. The resolver performs `GET {CHAIN_INDEX_URL}/modules/{module_id}` and expects a JSON `ModulePointer`.

- `CHAIN_RPC_URL`: Substrate RPC (ws/wss). The resolver reads the pallet storage `Modules::Modules(ss58_key_bytes) -> cid`, treats `cid` as a signed metadata document (Metadata v1), fetches it from IPFS, then:
  - Fetches `artifact_uri` from the metadata
  - Verifies `digest` (sha256) over artifact bytes
  - Verifies `signature` with the SS58 key (sr25519, domain "module_digest")
  - Returns a verified pointer to the artifact. Mismatch errors abort resolution.

If neither is set, `chain://` resolution is disabled and calls will error with a clear message.

### Metadata v1 (signed)
Expected JSON at the on-chain CID when using `CHAIN_RPC_URL`:

```
{
  "module_id": "<SS58>",
  "artifact_uri": "ipfs://<cid>",
  "digest": "sha256:<hex>",
  "signature": "<sr25519 sig base64|hex>",
  "signature_scheme": "sr25519",        // optional, defaults to sr25519
  "version": "1.0.0"                    // optional
}
```

The `module_id` must match the SS58 key used as the storage map key on-chain. The signature is verified over the digest with domain "module_digest".

## Persistent Task Storage

The Task Scheduler now uses a pluggable `TaskStorage` trait for all task persistence. The default implementation, `FileTaskStorage`, stores tasks as JSON on disk. All task operations (create, update, delete) are immediately persisted, and tasks are loaded from disk at startup, ensuring durability across restarts.

- **Pluggable Storage:** The `TaskStorage` trait allows for alternative backends (e.g., database, remote service) in the future.
- **FileTaskStorage:** By default, tasks are stored in a JSON file. This is used in both production and tests (with temporary files).
- **Immediate Persistence:** Every change to a task is written to disk, minimizing risk of data loss.
- **Startup Loading:** On startup, all tasks are loaded from the storage file, so the scheduler resumes with the correct state.
- **Test Coverage:** Automated tests verify that tasks persist across restarts and that all CRUD operations are durable.

This design ensures robust, reliable task management and makes it easy to extend or swap out the storage backend as needed.

## License

TBD
