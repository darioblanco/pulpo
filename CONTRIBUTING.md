# Contributing to Pulpo

Thanks for your interest in contributing to Pulpo!

## Getting Started

**Requirements:** Rust — pinned via `rust-toolchain.toml` (currently 1.93.1; `rustup` picks
it up automatically — edition 2024 and let-chains need at least 1.88), Node.js 22+, tmux 3.2+

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
cargo run -p pulpo-cli -- spawn some-repo --workdir ~/repos/some-repo -- claude -p "Do something"
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
make test           # run all unit tests (Rust + web)
make test-rust      # Rust unit tests only (excludes crates/pulpo-e2e)
make test-web       # web tests only
make test-web-watch # web tests in watch mode
make e2e            # end-to-end scenario suite (real daemon + tmux + fake harness; needs tmux)
make lint           # clippy + eslint + tsc
make all            # format + lint + test (same as pre-commit hook)
make coverage       # coverage checks (Rust + web)
make coverage-rust  # Rust coverage gate
make ci             # canonical full quality gate
make clean          # remove all build artifacts + dev data (.pulpo/)
```

## Testing

Two kinds of test, for two different jobs — see CLAUDE.md's "Testing" section for
the full rationale:

- **Unit tests** for pure logic (parsing, cron math, state-transition functions, ...).
  This project follows **TDD** for these: write the test, confirm it fails, write
  the minimal implementation, refactor while keeping it green, verify `make ci`
  passes. A handful of unit tests still talk to a real tmux server
  (`backend::tmux`, `session::manager`) instead of `MockBackend`; they take a
  process-wide lock (`crate::test_serial::lock()`) so they never run concurrently
  with each other.
- **Scenario tests** (`crates/pulpo-e2e/`, run with `make e2e`) for every
  user-facing session/watchdog/harness/schedule behavior, against a real `pulpod`
  daemon and a real tmux server — not a `MockBackend`. If your change affects one
  of those, add or extend a scenario (there are 12 today, `s1_*`–`s11_*` in
  `crates/pulpo-e2e/tests/scenarios.rs`, e.g. `s6_budget_breaker_stops_session_and_delivers_webhook`
  and `s9_worktrees_distinct_survive_stop_and_removed_by_cleanup`), not a
  mock-backend flow test. Scenarios drive a real `pulpod` against `fake-claude`
  (`crates/pulpo-e2e/src/bin/fake-claude.rs`), a small binary that plays Claude
  Code's part — reading the same `--session-id`/`--settings`/`--resume` flags and
  firing the same hook commands a real session would, scripted by the
  `FAKE_AGENT_SCENARIO` env var. `pulpo-e2e` runs in its own CI job (`e2e`,
  separate from `coverage`) and is excluded from both coverage instrumentation
  and release builds/artifacts — it's dev-only tooling, never shipped.
- **Build cache**: if you're running tests from more than one worktree/branch at
  once (e.g. parallel agent sessions), give each its own `CARGO_TARGET_DIR`
  (`$HOME/.cache/pulpo-target/<branch-or-worktree-name>`) — sharing one across
  concurrent builds corrupts cargo's fingerprints. See CLAUDE.md's "Linting"
  section.

See [CLAUDE.md](CLAUDE.md) for detailed conventions, project structure, and code standards.

## Submitting Changes

1. Fork the repo and create a branch from `main`
2. Make your changes following the existing code style
3. Ensure `make ci` passes locally
4. If your change touches session/watchdog/harness/schedule behavior, also run
   `make e2e` locally (not part of `make ci` — see "Testing" above)
5. Open a pull request with a clear description of the change

## Code Style

- **Rust**: `cargo fmt` (100 char width, edition 2024). Clippy with `deny(clippy::all)`; `pedantic`/`nursery` are intentionally off.
- **Web**: Prettier (single quotes, trailing commas, 100 char width). ESLint + `tsc --noEmit`.
- **No `unsafe` code** — `forbid(unsafe_code)` is set workspace-wide.
- **No `.unwrap()`** in production code — use `?` or handle errors explicitly. `.unwrap()` is fine in tests.

## License

By contributing, you agree that your contributions will be dual-licensed under MIT and Apache-2.0.
