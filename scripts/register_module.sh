#!/usr/bin/env bash
set -euo pipefail

# Load env (override with your own as needed)
if [[ -f "$HOME/repos/comai/mod-net/modsdk/mcp-registrar/.env" ]]; then
  # shellcheck disable=SC1090
  source "$HOME/repos/comai/mod-net/modsdk/mcp-registrar/.env"
fi

# MODULE_API_URL should be an HTTP(S) URL for the module-api, e.g. https://module-api-modnet.ngrok.dev
# MODULE_API_ADDR is the server bind address and not suitable as a client URL (often tcp or ws)
module_api_url=${MODULE_API_URL:-http://127.0.0.1:8090}
# IPFS base used by the server to upload artifacts
ipfs_base=${IPFS_API_URL:-}

# Required inputs
artifact_file=${FILE:?set FILE to the path of the artifact}
module_id=${SS58_ADDRESS:?set SS58_ADDRESS to your owner address}
chain_rpc_url=${CHAIN_RPC_URL:?set CHAIN_RPC_URL (e.g., wss://chain-rpc-modnet.ngrok.dev)}

# Signer via keytools
key_name=${MODULE_API_KEY_NAME:?set MODULE_API_KEY_NAME}
key_password=${MODULE_API_KEY_PASSWORD:?set MODULE_API_KEY_PASSWORD}

echo "Using module-api: $module_api_url"
echo "Artifact: $artifact_file"
echo "Module ID: $module_id"
echo "Chain RPC: $chain_rpc_url"
echo "Key Name: $key_name"
if [[ -n "$ipfs_base" ]]; then echo "IPFS Base: $ipfs_base"; fi

# Use the end-to-end client that signs using your keytools key (no mnemonic in env)
cargo run --bin mcp-registrar-client -- register-module \
  --module-api "$module_api_url" \
  --artifact-file "$artifact_file" \
  --module-id "$module_id" \
  --key-name "$key_name" \
  --key-password "$key_password" \
  --chain-rpc-url "$chain_rpc_url" \
  ${ipfs_base:+--ipfs-base "$ipfs_base"}