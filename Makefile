.PHONY: all check fmt lint test coverage coverage-rust coverage-web build clean setup hooks install release release-tarball service-install service-uninstall service-install-linux service-uninstall-linux deploy-server dev dev-web test-web-watch

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

# ─── Dev ──────────────────────────────────────────────────────────────────────

# Run the daemon from source (port 7433) using repo-local config
dev:
	@if [ ! -f .pulpo/config.toml ]; then \
		cp .pulpo/config.toml.example .pulpo/config.toml; \
		echo "Created .pulpo/config.toml from example"; \
	fi
	@mkdir -p .pulpo/data
	cargo run -p pulpod -- --config $(PWD)/.pulpo/config.toml

# Run the web UI dev server (port 5173, proxies /api to pulpod)
dev-web:
	cd web && npm run dev

# Run web tests in watch mode
test-web-watch:
	cd web && npx vitest

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
	cd web && npx tsc --noEmit

# ─── Test ────────────────────────────────────────────────────────────────────

# Run all tests
test: test-rust test-web

test-rust:
	cargo test --workspace

test-web:
	cd web && npx vitest run

# ─── Coverage ────────────────────────────────────────────────────────────────

# Generate test coverage reports (requires cargo-llvm-cov)
# Excludes main.rs files (thin cfg(coverage) wrappers that cargo test never invokes)
coverage: coverage-rust coverage-web

coverage-rust:
	cargo llvm-cov --workspace --ignore-filename-regex "(main|embed|build)\.rs$$" --fail-under-lines 100

coverage-web:
	cd web && npx vitest run --coverage

# Generate HTML coverage report
coverage-html:
	cargo llvm-cov --workspace --ignore-filename-regex "(main|embed|build)\.rs$$" --html
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

# ─── Release ────────────────────────────────────────────────────────────────

VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
ARCH := $(shell uname -m)
OS := $(shell uname -s | tr '[:upper:]' '[:lower:]')
TARBALL := pulpo-$(VERSION)-$(OS)-$(ARCH).tar.gz

# Build release binaries for current platform → dist/
release: build
	@mkdir -p dist
	cp target/release/pulpod dist/
	cp target/release/pulpo dist/
	@echo "Release binaries in dist/"
	@ls -lh dist/pulpod dist/pulpo

# Build release tarball for distribution → dist/pulpo-VERSION-OS-ARCH.tar.gz
release-tarball: release
	cd dist && tar czf $(TARBALL) pulpo pulpod
	@echo ""
	@echo "Tarball: dist/$(TARBALL)"
	@echo "SHA256:  $$(shasum -a 256 dist/$(TARBALL) | cut -d' ' -f1)"
	@ls -lh dist/$(TARBALL)

# Bump workspace version. Usage: make bump-version NEW_VERSION=0.2.0
bump-version:
ifndef NEW_VERSION
	$(error NEW_VERSION is required. Usage: make bump-version NEW_VERSION=0.2.0)
endif
	sed -i '' 's/^version = "$(VERSION)"/version = "$(NEW_VERSION)"/' Cargo.toml
	cargo generate-lockfile
	git add Cargo.toml Cargo.lock
	git commit -m "chore: bump to v$(NEW_VERSION)"
	@echo "Version bumped to $(NEW_VERSION). Push to main, then publish the draft release on GitHub."

# ─── Install & Deploy ────────────────────────────────────────────────────────

# Install binaries to /usr/local/bin
install: build
	cp target/release/pulpod /usr/local/bin/
	cp target/release/pulpo /usr/local/bin/

# Install and load launchd service (macOS)
service-install:
	cp contrib/com.pulpo.daemon.plist ~/Library/LaunchAgents/
	launchctl load ~/Library/LaunchAgents/com.pulpo.daemon.plist

# Unload and remove launchd service (macOS)
service-uninstall:
	launchctl unload ~/Library/LaunchAgents/com.pulpo.daemon.plist
	rm ~/Library/LaunchAgents/com.pulpo.daemon.plist

# Install and enable systemd user service (Linux)
service-install-linux:
	mkdir -p ~/.config/systemd/user
	cp contrib/pulpo.service ~/.config/systemd/user/
	systemctl --user daemon-reload
	systemctl --user enable --now pulpo

# Disable and remove systemd user service (Linux)
service-uninstall-linux:
	systemctl --user disable --now pulpo
	rm ~/.config/systemd/user/pulpo.service
	systemctl --user daemon-reload

# Deploy pulpod to a remote Linux server
DEPLOY_HOST ?= deploy@your-server
deploy-server:
	scp target/release/pulpod $(DEPLOY_HOST):/usr/local/bin/pulpod
	scp contrib/pulpo.service $(DEPLOY_HOST):~/.config/systemd/user/pulpo.service
	ssh $(DEPLOY_HOST) "systemctl --user daemon-reload && systemctl --user restart pulpo"
	@echo "Deployed pulpod to $(DEPLOY_HOST)"

# ─── Check ───────────────────────────────────────────────────────────────────

# Quick check: compiles but doesn't produce binaries (fastest feedback)
check:
	cargo check --workspace --all-targets

# Full CI check: format + lint + test + coverage
ci: fmt-check lint test coverage

# ─── Clean ───────────────────────────────────────────────────────────────────

clean:
	cargo clean
	rm -rf web/build web/node_modules .pulpo/data
