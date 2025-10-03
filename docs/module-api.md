# Module API — Developer Guide

Base URL: `https://module-api-modnet.ngrok.dev` (set via `MODULE_API_URL`)

## Environment
=== Canonical environment variables ===

Keys
Directory for keytools JSON key files
MODNET_KEYS_DIR=$HOME/.modnet/keys

Module API (HTTP server)
Public URL for clients and scripts
MODULE_API_URL=https://module-api-modnet.ngrok.dev
Bind address for the server process
MODULE_API_ADDR=https://module-api-modnet.ngrok.dev
Max JSON body size for artifact_base64 uploads (in MB)
MODULE_API_MAX_UPLOAD_MB=64

Chain RPC (ws/wss)
CHAIN_RPC_URL=wss://chain-rpc-modnet.ngrok.dev

IPFS
Base API URL for uploads (FastAPI style /files/upload or Kubo /api/v0)
IPFS_API_URL=https://ipfs-api-modnet.ngrok.dev
Optional API key header (sent as X-API-Key)
IPFS_API_KEY=
HTTP gateway for reads (must end with /ipfs/)
IPFS_GATEWAY_URL=https://ipfs-gateway-modnet.ngrok.dev/ipfs/

Registrar/Cache
MCP_REGISTRAR_AUTODETECT=false
REGISTRY_CACHE_DIR=$HOME/.cache/registry-scheduler

Example variables for scripts
FILE=/path/to/artifact
SS58_ADDRESS=your-ss58-address-here  
MODULE_API_KEY_NAME=my-key
MODULE_API_KEY_PASSWORD=your-password

---

- Module API
  - `MODULE_API_ADDR` (bind), `MODULE_API_URL` (public)
  - `MODULE_API_MAX_UPLOAD_MB` (default 64)
- Chain
  - `CHAIN_RPC_URL` (ws/wss)
- IPFS
  - `IPFS_API_URL` (upload), `IPFS_API_KEY` (optional header X-API-Key)
  - `IPFS_GATEWAY_URL` (reads; must end with `/ipfs/`)
- Keys (server-side optional auto-register)
  - `MODULE_API_KEY_NAME`, `MODULE_API_KEY_PASSWORD`
  - Key files live in `MODNET_KEYS_DIR` (default `$HOME/.modnet/keys`)

Start server:

```bash
export MODULE_API_MAX_UPLOAD_MB=256
export CHAIN_RPC_URL=<wss-url>
export IPFS_API_URL=<https-url>
cargo run --bin module-api
```

---

## Cryptography inputs

- `digest`: sha256 over artifact bytes, formatted as `sha256:<hex>`.
- `signature`: sr25519 signature over the digest using signing context `"module_digest"`.
  - Accepted formats: base64 or 128-char hex.

Sign with subkey:

```bash
subkey sign --scheme sr25519 --suri "<your SURI>" "sha256:<hex>"
```

---

## 1) POST /modules/publish

Uploads an artifact (or references an IPFS/HTTP URI), builds metadata, optionally registers on-chain.

Request JSON fields:
- Provide exactly one of:
  - `artifact_base64`: Base64 of artifact bytes
  - `artifact_uri`: `ipfs://<cid>` or `https://...` (HTTP pulled by server)
- Required:
  - `module_id`: SS58 address of module owner
  - `digest`: `"sha256:<hex>"`
  - `signature`: sr25519 signature over `digest` (base64 or 128-hex)
- Optional:
  - `version`: string tag
  - `publish`: boolean (if true and server has `MODULE_API_KEY_NAME/PASSWORD`, server will register on-chain)
  - Overrides: `ipfs_base`, `ipfs_api_key`, `chain_rpc_url`

Response 200 OK:

```json
{
  "metadata_cid": "<cid>",
  "artifact_uri": "ipfs://<cid>",
  "registered": false
}
```

Example: local file upload (base64)

```bash
FILE=/path/artifact.wasm
DIGEST_HEX=$(sha256sum "$FILE" | awk '{print $1}')
DIGEST="sha256:${DIGEST_HEX}"
SIGN=$(subkey sign --scheme sr25519 --suri "$SURI" "$DIGEST")
ART_B64=$(base64 -w0 "$FILE")

curl -s "${MODULE_API_URL}/modules/publish" \
  -H 'Content-Type: application/json' \
  -X POST -d "{
    \"artifact_base64\": \"${ART_B64}\",
    \"module_id\": \"${SS58_ADDRESS}\",
    \"digest\": \"${DIGEST}\",
    \"signature\": \"${SIGN}\",
    \"publish\": false,
    \"ipfs_base\": \"${IPFS_API_URL}\"
  }" | jq .
```

Example: hosted artifact (HTTP)

```bash
curl -s "${MODULE_API_URL}/modules/publish" \
  -H 'Content-Type: application/json' \
  -X POST -d "{
    \"artifact_uri\": \"https://your-host/artifacts/artifact.wasm\",
    \"module_id\": \"${SS58_ADDRESS}\",
    \"digest\": \"${DIGEST}\",
    \"signature\": \"${SIGN}\",
    \"publish\": false
  }" | jq .
```

Example: auto-register on-chain (server holds key)

```bash
curl -s "${MODULE_API_URL}/modules/publish" \
  -H 'Content-Type: application/json' \
  -X POST -d "{
    \"artifact_base64\": \"${ART_B64}\",
    \"module_id\": \"${SS58_ADDRESS}\",
    \"digest\": \"${DIGEST}\",
    \"signature\": \"${SIGN}\",
    \"publish\": true,
    \"chain_rpc_url\": \"${CHAIN_RPC_URL}\"
  }" | jq .
```

Notes:
- For large artifacts, increase `MODULE_API_MAX_UPLOAD_MB`.
- If supplying `artifact_uri` http(s), ensure the server can reach it.

---

## 2) POST /modules/publish/digest

Server computes the artifact digest for you (useful to confirm your local digest).

Request JSON:
- Provide one of:
  - `artifact_base64`
  - `artifact_uri` (must be `ipfs://` or http(s))
- Optional overrides: `ipfs_base`, `ipfs_api_key`

Response 200 OK:

```json
{ "digest": "sha256:<hex>" }
```

Example:

```bash
ART_B64=$(base64 -w0 "$FILE")
curl -s "${MODULE_API_URL}/modules/publish/digest" \
  -H 'Content-Type: application/json' \
  -X POST -d "{
    \"artifact_base64\": \"${ART_B64}\"
  }" | jq .
```

---

## 3) POST /modules/register

Registers an existing `metadata_cid` on-chain, binding it to `module_id`.

Request JSON:
- Required:
  - `module_id`: SS58 address
  - `metadata_cid`: CID string (metadata JSON stored on IPFS)
- One of:
  - `key_name` + `key_password` (server loads SURI from encrypted key file)
  - `suri` (explicit signer SURI)
- Optional:
  - `chain_rpc_url` (override server default)

Response 200 OK:

```json
{ "ok": true }
```

Example: use server’s keytools key

```bash
curl -s "${MODULE_API_URL}/modules/register" \
  -H 'Content-Type: application/json' \
  -X POST -d "{
    \"module_id\": \"${SS58_ADDRESS}\",
    \"metadata_cid\": \"${METADATA_CID}\",
    \"key_name\": \"${MODULE_API_KEY_NAME}\",
    \"key_password\": \"${MODULE_API_KEY_PASSWORD}\",
    \"chain_rpc_url\": \"${CHAIN_RPC_URL}\"
  }" | jq .
```

Example: explicit SURI signer

```bash
curl -s "${MODULE_API_URL}/modules/register" \
  -H 'Content-Type: application/json' \
  -X POST -d "{
    \"module_id\": \"${SS58_ADDRESS}\",
    \"metadata_cid\": \"${METADATA_CID}\",
    \"suri\": \"${SURI}\",
    \"chain_rpc_url\": \"${CHAIN_RPC_URL}\"
  }" | jq .
```

---

## 4) GET /modules/{module_id}

Fetches the current on-chain pointer and returns either raw CID or full metadata.

Query params:
- `raw=true` → returns only the CID string
- `no_verify=true` → skips signature verification of metadata/artifact

Responses:
- 200 OK (raw): `"bafy..."`
- 200 OK (default): metadata JSON contents

Example (raw CID):

```bash
curl -s "${MODULE_API_URL}/modules/${SS58_ADDRESS}?raw=true"
```

Example (verified metadata):

```bash
curl -s "${MODULE_API_URL}/modules/${SS58_ADDRESS}" | jq .
```

---

## Workflow summary

1) Compute digest + signature

```bash
DIGEST_HEX=$(sha256sum "$FILE" | awk '{print $1}')
DIGEST="sha256:${DIGEST_HEX}"
SIGN=$(subkey sign --scheme sr25519 --suri "$SURI" "$DIGEST")
```

2) Publish to IPFS (get `metadata_cid`)

```bash
# Use artifact_base64 or artifact_uri; see examples above
```

3) Register on-chain

```bash
# Call /modules/register with key_name/password (server key) or explicit suri
```

---

## Errors

- 400 Bad Request: Missing fields, invalid URI scheme, invalid digest/signature format.
- 500 Internal Server Error: Upstream failures (IPFS upload, RPC connectivity, JSON errors).
- Error body is a plain string; inspect for details.
