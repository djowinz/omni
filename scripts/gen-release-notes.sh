#!/usr/bin/env bash
# Generate release notes from git log since last tag.
# Usage: ./scripts/gen-release-notes.sh <version> [output-file]
set -euo pipefail

VERSION="${1:?Usage: gen-release-notes.sh <version> [output-file]}"
OUTPUT="${2:-apps/desktop/dist/RELEASE_NOTES.md}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

PREV_TAG=$(git describe --tags --abbrev=0 HEAD^ 2>/dev/null || echo "")
if [ -z "$PREV_TAG" ]; then
    COMMIT_LOG=$(git log --oneline)
else
    COMMIT_LOG=$(git log "${PREV_TAG}..HEAD" --oneline)
fi

mkdir -p "$(dirname "$OUTPUT")"
{
    echo "## v${VERSION}"
    echo ""
    echo "### Changes"
    echo ""
    echo "$COMMIT_LOG" | grep -v '\[skip ci\]' | while IFS= read -r line; do
        MSG=$(echo "$line" | sed 's/^[a-f0-9]* //')
        echo "- $MSG"
    done
    echo ""
} > "$OUTPUT"

echo "Release notes written to $OUTPUT"
cat "$OUTPUT"
