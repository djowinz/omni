# Omni — top-level build/test/release orchestration.
# See STRUCTURE.md for the monorepo layout.

.PHONY: build test lint format clean \
        rust test-rust lint-rust format-rust clean-rust \
        node test-node lint-node format-node clean-node \
        fetch-models \
        installer release release-notes \
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

# --- Moderation models ---
#
# Download + verify the bundled NSFW classifier ONNX models (NudeNet +
# Falconsai) referenced in `crates/moderation/resources/MODELS.toml`.
# Idempotent — skips files already present and matching the manifest hash;
# only the first run pays the ~96 MB download cost. Required before:
#   - `cargo test -p moderation` / `-p host` (otherwise inference tests skip)
#   - `cargo run -p host` (otherwise moderation gate degrades to NotInitialized)
#   - `make installer` / `make release` (electron-builder needs the .onnx
#     files on disk to bundle them via extraResources)
fetch-models:
	cargo run --release -p fetch-models

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
#
# Both targets depend on `fetch-models` because electron-builder packages the
# moderation .onnx files into the installer via `extraResources`. Without the
# files on disk at packaging time the installer ships a broken moderation
# pipeline (host starts but every check returns Moderation:NotInitialized).
# `fetch-models` is idempotent — when models are already present it costs
# milliseconds, so making it a hard prerequisite is cheap.
installer: fetch-models
	./scripts/build-installer.sh

release: fetch-models
	./scripts/release.sh

release-notes:
	./scripts/gen-release-notes.sh

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
