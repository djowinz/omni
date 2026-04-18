#!/usr/bin/env bash
# Build Rust binaries and run tests.
# Usage: ./scripts/build-rust.sh [--skip-tests]
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

echo "Building Rust binaries (release)..."
cargo build --release --package host --package overlay
echo "  ✓ omni-host.exe"
echo "  ✓ omni-overlay.exe"

if [ "${1:-}" != "--skip-tests" ]; then
    echo ""
    echo "Running Rust tests..."
    cargo test --workspace
    echo "  ✓ Rust tests passed"
    echo "  ✓ TypeScript bindings regenerated (packages/shared-types/src/generated/)"
fi
