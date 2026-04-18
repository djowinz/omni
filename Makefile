# Omni — top-level build/test/release orchestration.
# See STRUCTURE.md for the monorepo layout.

.PHONY: build test lint format clean \
        rust test-rust lint-rust format-rust clean-rust \
        node test-node lint-node format-node clean-node \
        installer release release-notes \
        dev-desktop dev-worker deploy-worker \
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
dev-desktop:
	pnpm dev:desktop

dev-worker:
	pnpm dev:worker

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
