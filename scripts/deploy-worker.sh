#!/usr/bin/env bash
# Deploy the Cloudflare Worker from apps/worker/.
# Thin wrapper around `wrangler deploy` so CI and the Makefile have
# a single entry point independent of the worker's internal scripts.
set -euo pipefail

cd "$(dirname "$0")/.."
pnpm --filter @omni/worker install --frozen-lockfile
pnpm --filter @omni/worker build:wasm
pnpm --filter @omni/worker exec wrangler deploy "$@"
