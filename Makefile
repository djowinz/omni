# Omni — top-level build/test/release orchestration.
# See STRUCTURE.md for the monorepo layout.

.PHONY: build test lint format clean \
        rust test-rust lint-rust format-rust clean-rust \
        node test-node lint-node format-node clean-node \
        installer release release-notes \
        build-real-guard tree-guard tree-real-guard \
        dev dev-seed dev-reset dev-reset-identity dev-kill dev-admin \
        dev-desktop dev-worker dev-worker-seeded deploy-worker \
        dev-reset-ratelimit dev-check-limits dev-list-artifacts \
        types-gen types-check structure-check

# --- Top-level ---
build:    rust node
test:     test-rust test-node
lint:     lint-rust lint-node
format:   format-rust format-node
clean:    clean-rust clean-node

# --- Rust ---
rust:
	cargo build --release --workspace

test-rust:
	cargo test --workspace

lint-rust:
	cargo fmt --check
	cargo clippy --workspace -- -D warnings

format-rust:
	cargo fmt

clean-rust:
	cargo clean

# --- Node (pnpm workspace) ---
node:
	pnpm -r build

test-node:
	pnpm -r test

lint-node:
	pnpm -r lint

format-node:
	pnpm format

clean-node:
	pnpm -r clean || true
	rm -rf node_modules

# --- Desktop installer pipeline (wraps existing scripts) ---
installer:
	./scripts/build-installer.sh

release:
	./scripts/release.sh

release-notes:
	./scripts/gen-release-notes.sh

# --- Real omni-guard build (private crate override) ---
#
# The workspace Cargo.toml unconditionally patches `omni-guard` to
# `stubs/omni-guard/` so contributors without SSH access to the private
# djowinz/omni-guard repo can still compile. Cargo does not support
# conditional [patch] sections (see cargo#9227), so the only way to
# resolve the real private crate is to pass `--config` overrides that
# replace the workspace patch with a git source.
#
# Cargo's `--config` CLI flag rejects inline tables. The patch has to
# be expressed as multiple scalar dotted-key overrides, one per field,
# which cargo then merges into an equivalent table entry.
#
# Requires SSH access to ssh://git@github.com/djowinz/omni-guard.git.
# Without the key, `cargo fetch` will fail; fall back to a plain
# `make build` (which uses the stub) if you don't have access.
#
# Verify which source resolved with `make tree-guard` after building.
GUARD_URL := ssh://git@github.com/djowinz/omni-guard.git
# `net.git-fetch-with-cli=true` makes cargo shell out to the system git
# binary (which uses the OS SSH agent) instead of libgit2's built-in SSH
# client. libgit2's SSH is notoriously finicky on Windows — it can't
# prompt for passphrases, hangs on first-time host-key confirmation, and
# often just silently loops forever. Scoped to these real-guard targets
# so the default `make build` (stub path, no git fetch) is unaffected.
CARGO_GUARD_CONFIG := \
	--config 'net.git-fetch-with-cli=true' \
	--config 'patch."$(GUARD_URL)".omni-guard.git="$(GUARD_URL)"' \
	--config 'patch."$(GUARD_URL)".omni-guard.branch="main"'

build-real-guard:
	cargo build --release --package host --features guard $(CARGO_GUARD_CONFIG)

# Print which omni-guard source Cargo actually resolved for the host
# binary. `stubs/omni-guard` path = using the public no-op stub;
# `ssh://git@github.com/djowinz/omni-guard` = real private crate active.
tree-guard:
	cargo tree -p host --features guard -e normal | grep -iE 'omni-guard'

# Same as tree-guard but forces the real-guard config override, so you
# can confirm the patch resolves correctly WITHOUT spending a full
# release build. Requires SSH access to the private crate.
tree-real-guard:
	cargo tree -p host --features guard -e normal $(CARGO_GUARD_CONFIG) | grep -iE 'omni-guard'

# --- Dev shortcuts ---
# `dev`, `dev-seed`, etc. invoke the omni-dev Rust orchestrator directly; see
# tools/dev-orchestrator/src/main.rs for subcommand details. `dev-admin` forwards
# all args after `--` to the omni-admin CLI against the local worker, e.g.:
#   make dev-admin ARGS='review'
#   make dev-admin ARGS='stats --json'
dev:
	cargo run --quiet -p dev-orchestrator -- run $(ARGS)

dev-seed:
	cargo run --quiet -p dev-orchestrator -- seed

dev-reset:
	cargo run --quiet -p dev-orchestrator -- reset $(ARGS)

dev-reset-identity:
	cargo run --quiet -p dev-orchestrator -- reset-identity $(ARGS)

dev-kill:
	cargo run --quiet -p dev-orchestrator -- kill

dev-admin:
	cargo run --quiet -p dev-orchestrator -- admin -- $(ARGS)

dev-desktop:
	pnpm dev:desktop

# Bare wrangler dev — no admin pubkey injection, no seed. Use when you just
# want to hit public worker routes without dev-orchestrator setup.
dev-worker:
	pnpm dev:worker

# wrangler dev + admin pubkey injected + seed, but NO Electron/host. Useful
# for iterating on worker code or hitting the local API from curl/Postman.
dev-worker-seeded:
	cargo run --quiet -p dev-orchestrator -- worker $(ARGS)

deploy-worker:
	./scripts/deploy-worker.sh

# --- Dev-state reset / inspection --------------------------------------
#
# Rate limits live in the worker's STATE KV as `quota:device:<df_hex>:<date>`
# and `quota:pubkey:<pk_hex>:<date>` counters (see apps/worker/src/lib/rate_limit.ts).
# Burning through 5 upload_new attempts/day/device is easy during smoke
# testing — especially when host auto-retries a failed 5xx (client.rs §9).
# This target lists every `quota:*` key in the local miniflare KV and
# deletes each one; leaves `config:*`, D1, R2, and identity keys intact.

dev-reset-ratelimit:
	@cd apps/worker && node -e "const {execSync}=require('child_process');const prefixes=['quota:','denylist:','df_pubkey_velocity:','flags:vm:'];let total=0;for(const p of prefixes){const raw=execSync('pnpm exec wrangler kv key list --binding=STATE --local --prefix='+p,{encoding:'utf8'});const keys=JSON.parse(raw);if(!keys.length){console.log('['+p+'] no keys');continue;}console.log('['+p+'] deleting '+keys.length+' keys:');for(const k of keys){console.log('  '+k.name);execSync('pnpm exec wrangler kv key delete --binding=STATE --local '+JSON.stringify(k.name),{stdio:'inherit'});total++;}}console.log('total deleted: '+total);"

# Print the current value of `config:limits` in the local STATE KV.
# Empty/missing = the cause of upload /v1/upload 500 errors per
# `getLimits()` in apps/worker/src/routes/upload.ts.
dev-check-limits:
	@cd apps/worker && pnpm exec wrangler kv key get --binding=STATE --local config:limits || echo "(not seeded — run your worker boot script or make dev-reset)"

# Print every row in D1's `artifacts` table — useful for confirming the seed
# populated (should show 4 rows: Neon Alley, HWMon Compact, Solarize Lite,
# Full Telemetry) and for verifying that a just-completed upload landed in
# the table. Empty result = the explore list will be empty too.
dev-list-artifacts:
	@cd apps/worker && pnpm exec wrangler d1 execute META --local --command "SELECT id, name, kind, hex(author_pubkey) AS author_pubkey_hex, install_count, is_removed, datetime(created_at, 'unixepoch') AS created FROM artifacts ORDER BY created_at DESC;"

# --- Shared types ---
types-gen:
	cargo test -p shared

types-check:
	cargo test -p shared
	git diff --exit-code packages/shared-types/src/generated

# --- Structure invariants ---
structure-check:
	./scripts/check-structure.sh
