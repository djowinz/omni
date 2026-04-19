# Omni — top-level build/test/release orchestration.
# See STRUCTURE.md for the monorepo layout.

.PHONY: build test lint format clean \
        rust test-rust lint-rust format-rust clean-rust \
        node test-node lint-node format-node clean-node \
        installer release release-notes \
        dev dev-desktop dev-worker dev-worker-seeded dev-seed dev-reset dev-reset-identity dev-kill dev-admin \
        deploy-worker \
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

# --- Shared types ---
types-gen:
	cargo test -p shared

types-check:
	cargo test -p shared
	git diff --exit-code packages/shared-types/src/generated

# --- Structure invariants ---
structure-check:
	./scripts/check-structure.sh
