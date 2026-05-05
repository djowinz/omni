#!/usr/bin/env bash
# Full release pipeline: bump → build → test → package → commit → publish.
# The version bump and tag are only pushed after all builds and tests pass.
#
# Usage:
#   ./scripts/release.sh              # Interactive prompt
#   ./scripts/release.sh patch|minor|major
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPTS="$REPO_ROOT/scripts"
cd "$REPO_ROOT"

echo "=== Omni Release ==="
echo ""

# ── Cleanup function for failures ────────────────────────────────────────
STASHED=0
NEW_VERSION=""
cleanup() {
    echo ""
    echo "!!! Release failed — rolling back..."
    # Revert workspace + every JS package.json bump (bump-version.sh sweeps
    # both apps/* and packages/*, so the rollback must too).
    if [ -n "$NEW_VERSION" ]; then
        cd "$REPO_ROOT"
        git checkout -- Cargo.toml apps/*/package.json packages/*/package.json 2>/dev/null || true
        # Delete local tag if it exists
        git tag -d "v${NEW_VERSION}" 2>/dev/null || true
    fi
    if [ "$STASHED" -eq 1 ]; then
        echo "Restoring stashed changes..."
        git stash pop 2>/dev/null || true
    fi
    echo "Rollback complete."
    exit 1
}
trap cleanup ERR

# ── 1. Ensure clean git state ────────────────────────────────────────────
echo "[1/8] Checking git state..."
if [ -n "$(git status --porcelain)" ]; then
    git stash push -m "release-script-auto-stash"
    STASHED=1
    echo "  ✓ Changes stashed"
else
    echo "  ✓ Working tree is clean"
fi
echo ""

# ── 2. Determine version increment ──────────────────────────────────────
echo "[2/8] Determining version increment..."
INCREMENT="${1:-}"

if [ -z "$INCREMENT" ]; then
    if [ ! -t 0 ]; then
        echo "ERROR: No increment type provided and running non-interactively."
        echo "Usage: ./scripts/release.sh patch|minor|major"
        exit 1
    fi
    echo "  Select version increment:"
    echo "    1) patch"
    echo "    2) minor"
    echo "    3) major"
    read -rp "  Choice [1/2/3]: " CHOICE
    case "$CHOICE" in
        1|patch)  INCREMENT="patch" ;;
        2|minor)  INCREMENT="minor" ;;
        3|major)  INCREMENT="major" ;;
        *)
            echo "ERROR: Invalid choice"
            if [ "$STASHED" -eq 1 ]; then git stash pop; fi
            exit 1
            ;;
    esac
fi

case "$INCREMENT" in
    patch|minor|major) ;;
    *)
        echo "ERROR: Invalid increment '$INCREMENT'. Use: patch, minor, or major"
        if [ "$STASHED" -eq 1 ]; then git stash pop; fi
        exit 1
        ;;
esac

echo "  ✓ Increment: $INCREMENT"
echo ""

# ── 3. Bump version locally (no commit, no push) ────────────────────────
echo "[3/8] Bumping version locally..."
# bump-version.sh wants an absolute target, so derive it from the current
# workspace version + INCREMENT. We do the semver math inline (a stock
# `node` is enough — no `semver` package needed) then hand the resolved
# version to bump-version.sh which sweeps every Cargo.toml + every
# apps/*/package.json + every packages/*/package.json in lockstep.
CURRENT=$(node -p "require('./apps/desktop/package.json').version")
NEW_VERSION=$(INCREMENT="$INCREMENT" CURRENT="$CURRENT" node -e "
    const cur = process.env.CURRENT;
    const inc = process.env.INCREMENT;
    const m = cur.match(/^(\d+)\.(\d+)\.(\d+)/);
    if (!m) { console.error('cannot parse current version: ' + cur); process.exit(1); }
    let [_, maj, min, pat] = m;
    maj = +maj; min = +min; pat = +pat;
    if      (inc === 'major') { maj += 1; min = 0; pat = 0; }
    else if (inc === 'minor') { min += 1; pat = 0; }
    else if (inc === 'patch') { pat += 1; }
    else { console.error('unknown increment: ' + inc); process.exit(1); }
    console.log(maj + '.' + min + '.' + pat);
")
"$SCRIPTS/bump-version.sh" "$NEW_VERSION"

# Create local tag so git describe picks it up for GitDate versioning
git tag "v${NEW_VERSION}"
echo "  ✓ v${NEW_VERSION} (local only — not pushed yet)"
echo ""

# ── 4. Build + test Rust ────────────────────────────────────────────────
echo "[4/8] Building and testing Rust..."
"$SCRIPTS/build-rust.sh"
echo ""

# ── 5. Build + test desktop ────────────────────────────────────────────
echo "[5/8] Building and testing desktop..."
"$SCRIPTS/build-desktop.sh"
echo ""

# ── 6. Package installer ───────────────────────────────────────────────
echo "[6/8] Packaging installer..."
"$SCRIPTS/build-installer.sh"
echo ""

# ── All builds passed — safe to commit and push ─────────────────────────

# ── 7. Commit, move tag, push ──────────────────────────────────────────
echo "[7/8] Committing and pushing..."
git add Cargo.toml apps/*/package.json packages/*/package.json
git commit -m "[skip ci] Bumping to v${NEW_VERSION}. Releasing..."

# Move tag from old HEAD to the new commit
git tag -f "v${NEW_VERSION}"

git push origin main
git push origin "v${NEW_VERSION}"
echo "  ✓ Pushed to main"
echo "  ✓ Tagged v${NEW_VERSION}"
echo ""

# ── 8. Release notes + GitHub release ──────────────────────────────────
echo "[8/8] Publishing GitHub release..."
NOTES_FILE="$REPO_ROOT/apps/desktop/dist/RELEASE_NOTES.md"
"$SCRIPTS/gen-release-notes.sh" "$NEW_VERSION" "$NOTES_FILE"

gh release create "v${NEW_VERSION}" \
    "$REPO_ROOT/apps/desktop/dist/OmniSetup.exe" \
    "$REPO_ROOT/apps/desktop/dist/latest.yml" \
    "$REPO_ROOT/apps/desktop/dist/OmniSetup.exe.blockmap" \
    --title "v${NEW_VERSION}" \
    --notes-file "$NOTES_FILE"

# Disable the error trap — we succeeded
trap - ERR

echo ""
echo "=== Release v${NEW_VERSION} complete ==="

if [ "$STASHED" -eq 1 ]; then
    echo ""
    echo "Restoring stashed changes..."
    git stash pop
fi
