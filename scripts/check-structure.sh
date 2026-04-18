#!/usr/bin/env bash
# Structural invariants enforced in CI.
# Fail-fast on any regression that would contradict STRUCTURE.md.
set -euo pipefail

fail() {
  echo "STRUCTURE VIOLATION: $1" >&2
  exit 1
}

# 1. No package.json outside declared workspace members.
expected_manifests=(
  "./package.json"
  "./apps/desktop/package.json"
  "./apps/worker/package.json"
  "./packages/shared-types/package.json"
)

while IFS= read -r -d '' found; do
  match=0
  for e in "${expected_manifests[@]}"; do
    [[ "$found" == "$e" ]] && match=1 && break
  done
  [[ $match -eq 0 ]] && fail "unexpected package.json at $found (not a declared workspace member)"
done < <(find . -name package.json -not -path "./node_modules/*" -not -path "./**/node_modules/*" -not -path "./target/*" -not -path "./crates/*/pkg/*" -print0)

# 2. default/ must not exist.
[[ -d "./default" ]] && fail "default/ directory reappeared; it must stay deleted"

# 3. services/ must not exist.
[[ -d "./services" ]] && fail "services/ directory exists; worker should be at apps/worker/"

# 4. No .prettierrc outside the repo root.
while IFS= read -r -d '' found; do
  [[ "$found" == "./.prettierrc" ]] && continue
  fail "stray .prettierrc at $found (formatting config is root-only)"
done < <(find . -name ".prettierrc" -not -path "./node_modules/*" -not -path "./**/node_modules/*" -print0)

# 5. No pnpm-workspace.yaml outside the repo root.
while IFS= read -r -d '' found; do
  [[ "$found" == "./pnpm-workspace.yaml" ]] && continue
  fail "stray pnpm-workspace.yaml at $found (workspace decl is root-only)"
done < <(find . -name "pnpm-workspace.yaml" -not -path "./node_modules/*" -not -path "./**/node_modules/*" -print0)

# 6. No pnpm-lock.yaml outside the repo root.
while IFS= read -r -d '' found; do
  [[ "$found" == "./pnpm-lock.yaml" ]] && continue
  fail "stray pnpm-lock.yaml at $found (lockfile is root-only)"
done < <(find . -name "pnpm-lock.yaml" -not -path "./node_modules/*" -not -path "./**/node_modules/*" -print0)

# 7. docs/superpowers/ must be gitignored.
if ! git check-ignore -q docs/superpowers/ 2>/dev/null; then
  # Only fail if the directory exists — an absent dir is fine.
  [[ -d docs/superpowers ]] && fail "docs/superpowers/ is not gitignored"
fi

echo "Structure check: PASS"
