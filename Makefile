.PHONY: all check fmt lint test coverage build clean setup hooks

# Run all checks (what pre-commit runs)
all: fmt lint test

# ─── Setup ──────────────────────────────────────────────────────────────────

# First-time setup: install tools and git hooks
setup: hooks
	rustup component add rustfmt clippy llvm-tools-preview
	cargo install cargo-llvm-cov
	cd web && npm install
	@echo "Setup complete."

# Install git pre-commit hooks
hooks:
	git config core.hooksPath .githooks
	@echo "Git hooks installed."

# ─── Format ─────────────────────────────────────────────────────────────────

# Format all code
fmt:
	cargo fmt --all
	cd web && npx prettier --write .

# Check formatting without modifying files
fmt-check:
	cargo fmt --all -- --check
	cd web && npx prettier --check .

# ─── Lint ────────────────────────────────────────────────────────────────────

# Run all linters
lint: lint-rust lint-web

lint-rust:
	cargo clippy --workspace --all-targets -- -D warnings

lint-web:
	cd web && npx eslint .
	cd web && npx svelte-check --tsconfig ./tsconfig.json

# ─── Test ────────────────────────────────────────────────────────────────────

# Run all tests
test: test-rust test-web

test-rust:
	cargo test --workspace

test-web:
	cd web && npx vitest run --passWithNoTests

# ─── Coverage ────────────────────────────────────────────────────────────────

# Generate test coverage report (requires cargo-llvm-cov)
coverage:
	cargo llvm-cov --workspace --fail-under-lines 100

# Generate HTML coverage report
coverage-html:
	cargo llvm-cov --workspace --html
	@echo "Coverage report: target/llvm-cov/html/index.html"

# ─── Build ───────────────────────────────────────────────────────────────────

# Build release binary with embedded web UI
build: build-web
	cargo build --release

build-web:
	cd web && npm run build

# Development build (no web embedding)
build-dev:
	cargo build

# ─── Check ───────────────────────────────────────────────────────────────────

# Quick check: compiles but doesn't produce binaries (fastest feedback)
check:
	cargo check --workspace --all-targets

# Full CI check: format + lint + test + coverage
ci: fmt-check lint test coverage

# ─── Clean ───────────────────────────────────────────────────────────────────

clean:
	cargo clean
	rm -rf web/build web/.svelte-kit web/node_modules
