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
# Start the MCP Registrar server
cargo run -- start-registrar

# Start the Task Scheduler server
cargo run -- start-task-scheduler

# Start the Tool Registry server
cargo run -- start-tool-registry

# Start the Resource Registry server
cargo run -- start-resource-registry

# Start the Prompt Registry server
cargo run -- start-prompt-registry
```

## Architecture

See the [ARCHITECTURE.md](ARCHITECTURE.md) document for detailed information on the architecture and design principles of this project.

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
