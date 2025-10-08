#!/usr/bin/env bash
set -euo pipefail

# Load env hierarchy: local overrides -> repo root -> registrar-local
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REGISTRAR_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$REGISTRAR_ROOT/.." && pwd)"

PAYLOAD_FILE=""
MODULE_API_OVERRIDE=""
CHAIN_RPC_OVERRIDE=""
IPFS_BASE_OVERRIDE=""
TIMEOUT_OVERRIDE=""

# Allow optional --payload <path> to dump publish request body for debugging
while [[ $# -gt 0 ]]; do
  case "$1" in
    --payload)
      [[ $# -ge 2 ]] || { echo "ERROR: --payload requires a file path" >&2; exit 1; }
      PAYLOAD_FILE="$2"
      shift 2
      ;;
    --module-api)
      [[ $# -ge 2 ]] || { echo "ERROR: --module-api requires a URL" >&2; exit 1; }
      MODULE_API_OVERRIDE="$2"
      shift 2
      ;;
    --chain-rpc-url)
      [[ $# -ge 2 ]] || { echo "ERROR: --chain-rpc-url requires a URL" >&2; exit 1; }
      CHAIN_RPC_OVERRIDE="$2"
      shift 2
      ;;
    --ipfs-base)
      [[ $# -ge 2 ]] || { echo "ERROR: --ipfs-base requires a URL" >&2; exit 1; }
      IPFS_BASE_OVERRIDE="$2"
      shift 2
      ;;
    --timeout-secs)
      [[ $# -ge 2 ]] || { echo "ERROR: --timeout-secs requires a positive integer" >&2; exit 1; }
      TIMEOUT_OVERRIDE="$2"
      shift 2
      ;;
    --help|-h)
      cat <<'EOF'
Usage: scripts/register_module.sh [options]

Reads configuration from .env / .env.local (repo root and registrar directory)
and invokes the Rust registrar client. Options:
  --payload <file>        Write the JSON body sent to /modules/publish
  --module-api <url>      Override MODNET_MODULE_API_URL for this run
  --chain-rpc-url <url>   Override MODNET_CHAIN_RPC_URL for this run
  --ipfs-base <url>       Override MODNET_IPFS_BASE_URL for this run
  --timeout-secs <secs>   Override MODNET_REGISTER_TIMEOUT_SECS (default 10)
EOF
      exit 0
      ;;
    *)
      echo "ERROR: Unknown option $1" >&2
      exit 1
      ;;
  esac
done

ENV_FILES=(
  "$REPO_ROOT/.env.local"
  "$REPO_ROOT/.env"
  "$REGISTRAR_ROOT/.env.local"
  "$REGISTRAR_ROOT/.env"
)

for env_file in "${ENV_FILES[@]}"; do
  if [[ -f "$env_file" ]]; then
    set -a
    # shellcheck disable=SC1090
    source "$env_file"
    set +a
  fi
done

module_api_url=${MODULE_API_OVERRIDE:-${MODNET_MODULE_API_URL:?set MODNET_MODULE_API_URL}}
artifact_file=${MODNET_MODULE_ARTIFACT:?set MODNET_MODULE_ARTIFACT}
module_id=${MODNET_MODULE_ID:?set MODNET_MODULE_ID}
chain_rpc_url=${CHAIN_RPC_OVERRIDE:-${MODNET_CHAIN_RPC_URL:?set MODNET_CHAIN_RPC_URL}}
ipfs_base=${IPFS_BASE_OVERRIDE:-${MODNET_IPFS_BASE_URL-}}
timeout_secs=${TIMEOUT_OVERRIDE:-${MODNET_REGISTER_TIMEOUT_SECS:-30}}
key_name=${MODULE_API_KEY_NAME:?set MODULE_API_KEY_NAME}
key_password=${MODULE_API_KEY_PASSWORD:?set MODULE_API_KEY_PASSWORD}

echo "Using module-api: $module_api_url"
echo "Artifact: $artifact_file"
echo "Module ID: $module_id"
echo "Chain RPC: $chain_rpc_url"
echo "Key Name: $key_name"
if [[ -n "$ipfs_base" ]]; then echo "IPFS Base: $ipfs_base"; fi
echo "Timeout (s): $timeout_secs"

# Optionally write the publish request payload for debugging purposes
if [[ -n "$PAYLOAD_FILE" ]]; then
  cat >"$PAYLOAD_FILE" <<EOF
{
  "artifact_base64": "<set at runtime>",
  "module_id": "$module_id",
  "digest": "<set at runtime>",
  "signature": "<set at runtime>",
  "publish": false,
  "chain_rpc_url": "$chain_rpc_url"${ipfs_base:+,
  "ipfs_base": "$ipfs_base"}
}
EOF
  echo "Wrote publish payload template to $PAYLOAD_FILE"
fi

# Use the end-to-end client that signs using your keytools key (no mnemonic in env)
cargo run --bin mcp-registrar-client -- --timeout-secs "$timeout_secs" register-module \
  --module-api "$module_api_url" \
  --artifact-file "$artifact_file" \
  --module-id "$module_id" \
  --key-name "$key_name" \
  --key-password "$key_password" \
  --chain-rpc-url "$chain_rpc_url" \
  ${ipfs_base:+--ipfs-base "$ipfs_base"}