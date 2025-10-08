# Module API Reference

The `module-api` binary exposes REST and MCP interfaces for module publication, registry lookup, and tool orchestration. This document provides the authoritative API surface derived from `mcp-registrar/src/bin/module_api.rs`.

## Overview

- **Default bind address**: `MODULE_API_ADDR` (defaults to `127.0.0.1:8090`).
- **Public URL**: `MODULE_API_URL` for external clients.
- **Max upload size**: `MODULE_API_MAX_UPLOAD_MB` (megabytes, default from env).
- **Dependencies**:
  - Chain RPC endpoint (`CHAIN_RPC_URL`).
  - IPFS upload base (`IPFS_API_URL`) and optional `IPFS_API_KEY`.
  - Optional key storage via keytools JSON files (`MODNET_KEYS_DIR`).
- **Secrets**: On-chain publishing requires `MODULE_API_KEY_NAME` and `MODULE_API_KEY_PASSWORD` to unlock signing keys.

## REST API

### `POST /modules/publish`

Upload or reference a module artifact, produce metadata, optionally register on-chain.

**Request body** (`PublishRequest`):

```json
{
  "artifact_uri": "ipfs://<cid>" | "https://...",
  "artifact_base64": "<base64>",
  "module_id": "0x…",
  "digest": "sha256:<hex>",
  "signature": "<sr25519 signature>",
  "version": "<semver?>",
  "publish": false,
  "ipfs_base": "https://...",
  "ipfs_api_key": "<token>",
  "chain_rpc_url": "wss://..."
}
```

**Rules**

- Provide exactly one of `artifact_uri` or `artifact_base64`.
- `module_id` is the SS58/AccountId owner (hex string accepted).
- `digest` must match the artifact bytes (`sha256:` prefix enforced).
- `signature` must be sr25519 over the digest using context `module_digest` (base64 or 128-hex supported).
- When `publish=true`, the server signs and submits `Modules::register` using environment-provided credentials.
- IPFS upload uses overrides when provided; otherwise falls back to configured defaults.

**Response** (`PublishResponse`):

```json
{
  "metadata_cid": "bafy...",
  "artifact_uri": "ipfs://bafy...",
  "registered": true
}
```

**Errors**

- `400`: Missing artifact, bad digest/signature, unsupported URI.
- `500`: IPFS/HTTP failures, chain submission errors, JSON serialization issues.

### `POST /modules/publish/digest`

Compute a canonical sha256 digest for a supplied artifact.

**Request body** (`DigestRequest`): same artifact fields as `/modules/publish` (without signing metadata).

**Response**:

```json
{ "digest": "sha256:<hex>" }
```

**Notes**

- `artifact_uri` must be `ipfs://` or HTTP(S). Non-IPFS URIs fetched over HTTP and hashed locally.
- Returns `400` for missing/invalid sources, `500` for fetch errors.

### `POST /modules/register`

Registers metadata on-chain once the IPFS CID is available.

**Request body** (`RegisterRequest`):

```json
{
  "module_id": "0x…",
  "metadata_cid": "bafy...",
  "suri": "//Alice" | null,
  "chain_rpc_url": "wss://..." | null,
  "key_name": "module-api",
  "key_password": "<secret>"
}
```

Provide either:

- `suri` explicitly, or
- `key_name` + `key_password` to decrypt a keytools JSON file stored under `MODNET_KEYS_DIR`.

**Response** (`RegisterResponse`):

```json
{ "ok": true }
```

**Errors**

- `400`: signer material missing, bad CID/module ID.
- `500`: chain RPC / signing / storage errors.

### `POST /modules/register/build`

Returns `501 Not Implemented` with guidance to use `/modules/register` or submit signed extrinsic via `/modules/register/submit`.

### `POST /modules/register/submit`

Returns `501 Not Implemented`; clients should craft and submit extrinsics manually for now.

### `GET /modules/{module_id}`

Fetch metadata for `module_id` from chain storage and optionally verify digest.

**Query parameters** (`QueryParams`):

- `raw=true`: Return `{ "cid": "bafy..." }` without fetching metadata.
- `no_verify=true`: Skip artifact download/digest verification when returning metadata.

**Response variants** (`QueryResponse`):

```json
{ "cid": "bafy..." }
```

or

```json
{ "metadata": { /* metadata.json */ } }
```

**Validation**

- Metadata fetched from IPFS gateway; digest check enforced unless `no_verify=true`.
- HTTP errors or digest mismatches yield `500` with detailed message.

## MCP API

The server also exposes MCP transports for tool orchestration.

### Transport Endpoints

- `GET /mcp/sse`: Establish a Server-Sent Events stream. The server emits JSON-RPC frames as `data:` payloads and sends keep-alives every 15 seconds.
- `POST /mcp/sse`: Submit MCP JSON-RPC frames. Supports two payload forms:
  - `{ "session_id": "uuid", "frame": { ... } }`
  - Raw JSON-RPC object (server will associate with latest session or reply directly).
- `GET /mcp/ws`: Upgrade to WebSocket. Bidirectional JSON messages handled by `handle_mcp_websocket()`.

### Content Negotiation

- Requests with `Accept` containing `text/event-stream` receive SSE responses even without a live session.
- When `Accept` contains `application/json` **and not** `text/event-stream`, the server returns a single JSON payload with `Content-Type: application/json`.
- All tool results are normalized by `wrap_tool_result_for_mcp()` into a `content` array of MCP-compatible blocks (`text` or original `content`).

### Supported JSON-RPC Methods

- `initialize`
- `tools/list`
- `tools/call`
- `resources/list`
- `resources/read`
- `prompts/list`
- `prompts/get`
- `metrics/get`

Unknown methods return JSON-RPC error `-32601` (`Method not found`). Parameter validation errors yield `-32602`. Transport failures map to `-32603` (`Internal error`).

### Example MCP Calls

**Initialize**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "clientInfo": { "name": "example", "version": "1.0" },
    "capabilities": {}
  }
}
```

**List Tools**

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/list",
  "params": {}
}
```

**Invoke Registry Tool**

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "registry",
    "arguments": { "action": "list" }
  }
}
```

## Operational Notes

- **Session Tracking**: SSE sessions stored in-memory (`ModuleApiState::sse_sessions`). Server logs when delivery fails and falls back to direct HTTP response.
- **Tool Runtime**: Uses `servers::tool_runtime::executors::process` to spawn or reuse tool processes according to manifest.
- **Metrics**: `metrics/get` returns snapshot counts from `monitoring::TOOL_METRICS` (invocations, errors, durations, bytes transferred).
- **Logging**: Structured tracing via `tracing` crate; restart with `pm2` or similar supervisors if port conflicts occur.

## Error Conventions

- REST handlers respond with HTTP status and descriptive message body (string).
- MCP responses use JSON-RPC error objects:
  - `-32600`: Invalid request frame.
  - `-32601`: Method not found.
  - `-32602`: Invalid params.
  - `-32603`: Internal error.

## Quick Test Commands

- **List tools over JSON**:

  ```bash
  curl -s -X POST \
    -H "Content-Type: application/json" \
    -H "Accept: application/json" \
    --data '{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"registry","arguments":{"action":"list"}}}' \
    "${MODULE_API_URL:-http://127.0.0.1:8090}/mcp/sse"
  ```

- **Open SSE stream**:

  ```bash
  curl -N "${MODULE_API_URL:-http://127.0.0.1:8090}/mcp/sse"
  ```

- **Publish artifact via base64**: see `/modules/publish` example; ensure `MODULE_API_MAX_UPLOAD_MB` covers file size.

## Environment Cheat Sheet

- `MODULE_API_ADDR` — server bind address (host:port).
- `MODULE_API_URL` — public-facing base URL.
- `MODULE_API_MAX_UPLOAD_MB` — maximum upload payload size.
- `CHAIN_RPC_URL` — default Substrate node endpoint.
- `IPFS_API_URL` / `IPFS_API_KEY` — upload target and credentials.
- `MODULE_API_KEY_NAME` / `MODULE_API_KEY_PASSWORD` — keytools secrets for auto-registration.
- `MODNET_KEYS_DIR` — directory containing encrypted key JSON files.

## Troubleshooting

- **Port already in use**: Stop conflicting process or adjust `MODULE_API_ADDR`.
- **IPFS upload failures**: Verify `IPFS_API_URL` and `IPFS_API_KEY`; server logs include upstream status codes.
- **Chain RPC errors**: Ensure node reachable; override with `chain_rpc_url` in requests if necessary.
- **JSON-RPC invalid request**: Confirm payload matches examples; keys must be strings and object fields properly quoted.

Maintain this document alongside updates to `module_api.rs` to keep the reference accurate.
