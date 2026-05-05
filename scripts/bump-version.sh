#!/usr/bin/env bash
# Usage: scripts/bump-version.sh <new-semver>
#
# Bumps every workspace crate + every apps/*/package.json + every
# packages/*/package.json to the same semver. Validates lockstep via
# `cargo metadata` and `node -p`. Exits non-zero on any mismatch.
# Doesn't commit (release.sh / release.yml does that downstream).
#
# Crates inherit via `version.workspace = true`, so this script only
# touches the root Cargo.toml's [workspace.package] version line. New
# crates that forget the inherit are caught by the post-bump cargo
# metadata check.
#
# JS packages are swept across BOTH apps/* and packages/* — packages/
# currently holds shared-types, but anything new dropped under either
# tree is bumped automatically.
#
# Uses `node` (already a project dependency) instead of `jq` to parse
# JSON, so the script runs on stock GitHub Actions runners without an
# extra apt/choco install.

set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <new-semver>" >&2
    exit 1
fi

NEW="$1"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Validate semver shape — major.minor.patch with optional -prerelease suffix.
if ! [[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$ ]]; then
    echo "error: '$NEW' is not a valid semver" >&2
    exit 1
fi

cd "$REPO_ROOT"

# 1. Bump root Cargo.toml's [workspace.package] version line.
ROOT_TOML="Cargo.toml"
if ! grep -q '^\[workspace\.package\]' "$ROOT_TOML"; then
    echo "error: $ROOT_TOML missing [workspace.package] block" >&2
    exit 1
fi
# Edit ONLY the version line within the [workspace.package] block.
# Awk pass: in-block flag = 1 between [workspace.package] and the next [section];
# replace 'version = "..."' on the first matching line.
awk -v new="$NEW" '
    /^\[workspace\.package\]$/ { in_block = 1; print; next }
    in_block && /^\[/         { in_block = 0; print; next }
    in_block && /^version[[:space:]]*=/ {
        print "version = \"" new "\""
        next
    }
    { print }
' "$ROOT_TOML" > "$ROOT_TOML.tmp"
mv "$ROOT_TOML.tmp" "$ROOT_TOML"
echo "  bumped [workspace.package] version -> $NEW"

# 2. Bump every JS workspace package.json (apps/* + packages/*).
bump_pkg() {
    local pkg="$1"
    # node -e is more portable than jq + handles BOM/encoding right.
    NEW="$NEW" PKG="$pkg" node -e "
        const fs = require('fs');
        const path = process.env.PKG;
        const data = JSON.parse(fs.readFileSync(path, 'utf8'));
        data.version = process.env.NEW;
        fs.writeFileSync(path, JSON.stringify(data, null, 2) + '\n');
    "
    echo "  bumped $pkg -> $NEW"
}

for pkg in apps/*/package.json; do
    [ -f "$pkg" ] && bump_pkg "$pkg"
done
for pkg in packages/*/package.json; do
    [ -f "$pkg" ] && bump_pkg "$pkg"
done

# 3. Validate every workspace member reports the new version.
echo ""
echo "Validating cargo metadata lockstep..."
MISMATCH=0
# Use node to parse cargo metadata JSON (avoids requiring jq on the runner).
while IFS=$'\t' read -r name ver; do
    if [ "$ver" != "$NEW" ]; then
        echo "  MISMATCH: cargo package '$name' = $ver (expected $NEW)" >&2
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

# 4. Validate every JS package.json reports the new version.
validate_pkg() {
    local pkg="$1"
    local ver
    ver=$(node -p "require('./$pkg').version")
    if [ "$ver" != "$NEW" ]; then
        echo "  MISMATCH: $pkg = $ver (expected $NEW)" >&2
        MISMATCH=1
    fi
}

echo "Validating apps/*/package.json lockstep..."
for pkg in apps/*/package.json; do
    [ -f "$pkg" ] && validate_pkg "$pkg"
done

echo "Validating packages/*/package.json lockstep..."
for pkg in packages/*/package.json; do
    [ -f "$pkg" ] && validate_pkg "$pkg"
done

if [ "$MISMATCH" -ne 0 ]; then
    echo ""
    echo "error: version drift detected — fix the offending manifest(s) and re-run" >&2
    exit 1
fi

echo ""
echo "OK: All workspace + apps + packages versions = $NEW"
