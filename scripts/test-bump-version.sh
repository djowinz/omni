#!/usr/bin/env bash
# Sanity test for scripts/bump-version.sh. Bumps to a sentinel version,
# verifies cargo metadata + node reports it everywhere (including
# packages/*), then restores via git. Run from CI to catch a forgotten
# version.workspace = true on a new crate AND to catch script regressions.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

SENTINEL="99.99.99"

# Refuse only when tracked files are modified — untracked build artifacts
# (target-*/, *.log) are common in dev shells and don't affect the bumper.
# CI checkouts have neither, so this stays strict there.
if [ -n "$(git status --porcelain | grep -v '^??')" ]; then
    echo "error: tracked files have uncommitted changes; refusing to run" >&2
    git status --porcelain | grep -v '^??' >&2
    exit 1
fi

# Restore every file the bumper touches: root Cargo.toml + all JS package
# manifests under apps/* and packages/*.
trap 'echo "restoring..."; git checkout -- Cargo.toml apps/*/package.json packages/*/package.json 2>/dev/null || true' EXIT

./scripts/bump-version.sh "$SENTINEL"

# Validate every workspace member reports the sentinel.
MISMATCH=0
while IFS=$'\t' read -r name ver; do
    if [ "$ver" != "$SENTINEL" ]; then
        echo "FAIL: cargo package '$name' = $ver (expected $SENTINEL)" >&2
        MISMATCH=1
    fi
done < <(cargo metadata --format-version 1 --no-deps | node -e "
    let raw = '';
    process.stdin.on('data', chunk => raw += chunk);
    process.stdin.on('end', () => {
        const meta = JSON.parse(raw);
        for (const p of meta.packages) {
            process.stdout.write(p.name + '\t' + p.version + '\n');
        }
    });
")

check_pkg() {
    local pkg="$1"
    local ver
    # Pass path via env-var so a path containing shell-meta chars can't break
    # out of the node script (matches bump-version.sh's bump_pkg pattern).
    ver=$(PKG="$pkg" node -e "console.log(JSON.parse(require('fs').readFileSync(process.env.PKG, 'utf8')).version)")
    if [ "$ver" != "$SENTINEL" ]; then
        echo "FAIL: $pkg = $ver (expected $SENTINEL)" >&2
        MISMATCH=1
    fi
}

for pkg in apps/*/package.json; do
    [ -f "$pkg" ] && check_pkg "$pkg"
done
for pkg in packages/*/package.json; do
    [ -f "$pkg" ] && check_pkg "$pkg"
done

if [ "$MISMATCH" -ne 0 ]; then
    echo ""
    echo "scripts/bump-version.sh failed to bump every manifest. \
A new crate likely missed version.workspace = true, or a new JS package \
was added under apps/ or packages/ but the bumper was not updated." >&2
    exit 1
fi

echo "OK: scripts/bump-version.sh sanity test passed"
