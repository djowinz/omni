#!/usr/bin/env bash
# Build the Electron desktop app and run tests.
# Usage: ./scripts/build-desktop.sh [--skip-tests]
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT/apps/desktop"

echo "Installing desktop dependencies..."
pnpm install --frozen-lockfile 2>/dev/null || pnpm install

if [ "${1:-}" != "--skip-tests" ]; then
    echo ""
    echo "Running desktop tests..."
    pnpm test
    echo "  ✓ Desktop tests passed"
fi

echo ""
echo "Building Electron app..."
pnpm run build
echo "  ✓ Next.js + Electron compiled"
