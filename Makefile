.PHONY: build test lint format clean rust desktop installer release release-notes

# Build everything
build: rust desktop

# Run all tests
test: test-rust test-desktop

# Lint everything
lint: lint-rust lint-desktop

# ── Rust ─────────────────────────────────────────────────────────────────

# Build Rust binaries (release) + test
rust:
	./scripts/build-rust.sh

# Build Rust without tests
rust-fast:
	./scripts/build-rust.sh --skip-tests

# Run Rust tests only
test-rust:
	cargo test --workspace

# Run clippy
lint-rust:
	cargo clippy --workspace -- -D warnings

# ── Desktop ──────────────────────────────────────────────────────────────

# Build desktop app + test
desktop:
	./scripts/build-desktop.sh

# Build desktop without tests
desktop-fast:
	./scripts/build-desktop.sh --skip-tests

# Run desktop tests only
test-desktop:
	cd apps/desktop && pnpm test

# Check TypeScript formatting
lint-desktop:
	cd apps/desktop && pnpm format:check

# ── Formatting ────────────────────────────────────────────────────────────

# Format everything
format: format-rust format-desktop

# Format Rust
format-rust:
	cargo fmt --all

# Format TypeScript
format-desktop:
	cd apps/desktop && pnpm format

# ── Packaging ────────────────────────────────────────────────────────────

# Build the NSIS installer (builds rust + desktop first)
installer: rust desktop
	./scripts/build-installer.sh

# Full release pipeline
release:
	./scripts/release.sh $(INCREMENT)

# Generate release notes
release-notes:
	@if [ -z "$(VERSION)" ]; then echo "Usage: make release-notes VERSION=1.2.3"; exit 1; fi
	./scripts/gen-release-notes.sh $(VERSION)

# ── Development ──────────────────────────────────────────────────────────

# Start desktop dev server
dev:
	cd apps/desktop && pnpm dev

# Install desktop dependencies
install:
	cd apps/desktop && pnpm install

# Clean all build artifacts
clean:
	cargo clean
	rm -rf apps/desktop/dist/win-unpacked
	rm -rf apps/desktop/dist/OmniSetup*
	rm -rf apps/desktop/dist/latest.yml
	rm -rf apps/desktop/app
	rm -rf apps/desktop/renderer/.next
	rm -rf apps/desktop/renderer/generated
