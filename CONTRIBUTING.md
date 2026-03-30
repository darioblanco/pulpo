# Contributing to Pulpo

Thanks for your interest in contributing to Pulpo!

## Getting Started

**Requirements:** Rust 1.82+, Node.js 22+, tmux 3.2+

```bash
git clone https://github.com/darioblanco/pulpo.git
cd pulpo
make setup    # installs tools, git hooks, web dependencies
```

## Local Development Workflow

You need three terminals: the daemon, the web UI dev server, and a terminal for CLI/API interaction.

### 1. Run the daemon from source

```bash
# Terminal A — start pulpod (port 7433, repo-local config at .pulpo/config.toml)
make dev
```

`make dev` creates `.pulpo/config.toml` from `.pulpo/config.toml.example` if missing,
so your local development config stays in the repo directory (gitignored).

To run with an explicit config path:

```bash
# .pulpo/ is gitignored
mkdir -p .pulpo
cat > .pulpo/config.toml <<'EOF'
[node]
name = "local-dev"
EOF

cargo run -p pulpod -- --config .pulpo/config.toml
```

### 2. Run the web UI dev server

```bash
# Terminal B — Vite dev server on :5173, proxies /api to :7433
make dev-web
```

Open http://localhost:5173 for the dashboard with hot reload.

### 3. Drive with CLI or curl

```bash
# Terminal C — use the CLI from source
cargo run -p pulpo-cli -- list
cargo run -p pulpo-cli -- spawn --workdir ~/repos/some-repo "Do something"
cargo run -p pulpo-cli -- logs some-repo

# Or hit the API directly
curl http://localhost:7433/api/v1/health
curl http://localhost:7433/api/v1/sessions
curl -N http://localhost:7433/api/v1/events   # SSE stream
```

Note: spawning a real session requires Claude Code or Codex installed and authenticated.

### 4. Dev/test loop

```bash
make check          # fast compile check (fastest feedback)
make test           # run all tests (Rust + web)
make test-rust      # Rust tests only
make test-web       # web tests only
make test-web-watch # web tests in watch mode
make lint           # clippy + eslint + tsc
make all            # format + lint + test (same as pre-commit hook)
make coverage       # coverage checks (Rust + web)
make coverage-rust  # Rust coverage gate
make ci             # canonical full quality gate
make clean          # remove all build artifacts + dev data (.pulpo/)
```

## Test-Driven Development

This project follows **TDD**. Every change starts with a failing test:

1. Write the test
2. Confirm it fails
3. Write the minimal implementation
4. Refactor while keeping tests green
5. Verify `make ci` passes

See [CLAUDE.md](CLAUDE.md) for detailed conventions, project structure, and code standards.

## Submitting Changes

1. Fork the repo and create a branch from `main`
2. Make your changes following the existing code style
3. Ensure `make ci` passes locally
4. Open a pull request with a clear description of the change

## Code Style

- **Rust**: `cargo fmt` (100 char width, edition 2024). Clippy with `deny(warnings)`, `warn(pedantic, nursery)`.
- **Web**: Prettier (single quotes, trailing commas, 100 char width). ESLint + svelte-check.
- **No `unsafe` code** — `forbid(unsafe_code)` is set workspace-wide.
- **No `.unwrap()`** in production code — use `?` or handle errors explicitly. `.unwrap()` is fine in tests.

## License

By contributing, you agree that your contributions will be dual-licensed under MIT and Apache-2.0.
