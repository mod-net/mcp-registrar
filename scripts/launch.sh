#!/usr/bin/env bash

set -e

source .venv/bin/activate

pm2 start './target/release/registry-scheduler start-tool-registry' --name tool-registry
pm2 start './target/release/registry-scheduler start-resource-registry' --name resource-registry
pm2 start './target/release/registry-scheduler start-prompt-registry' --name prompt-registry
pm2 start './target/release/registry-scheduler start-task-scheduler' --name task-scheduler
pm2 start './target/release/registry-scheduler start-registrar' --name registrar
pm2 start './target/release/mcp_gateway' --name mcp-gateway
