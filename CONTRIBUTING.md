# Contributing to Pulpo

Thanks for your interest in contributing to Pulpo! This is the short, human-facing
contributor workflow. For conventions, testing strategy, coverage rules, and the full
project layout, see [AGENTS.md](AGENTS.md) — it's the single development guide (for
human contributors and coding agents alike); this file just gets you from clone to a
running local setup and points into the right AGENTS.md sections from there.

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
make e2e            # end-to-end scenario suite (real daemon + tmux + fake harness; needs tmux)
make lint           # clippy + eslint + tsc
make all            # format + lint + test (same as pre-commit hook)
make ci             # canonical full quality gate
```

See AGENTS.md → "Makefile Targets" for the complete list (coverage, release, service
install, docs).

## Testing

Pulpo uses two kinds of test for two different jobs — unit tests (TDD, pure logic) and
scenario tests (`crates/pulpo-e2e/`, run with `make e2e`, for user-facing
session/watchdog/harness/schedule behavior against a real daemon and tmux server, not a
mock). If your change touches one of those behaviors, add or extend a scenario in
`crates/pulpo-e2e/tests/scenarios.rs` rather than a mock-backend flow test. See
AGENTS.md → "Testing — unit tests for logic, scenario tests for behavior" for the full
rationale (including why a proposal to lower the coverage bar was rejected — ADR
[0003](docs/adr/0003-scenario-tests-as-behavior-gate.md)) and AGENTS.md → "Linting" for
giving each concurrent worktree/branch its own `CARGO_TARGET_DIR`.

## Submitting Changes

1. Fork the repo and create a branch from `main`
2. Make your changes following the existing code style (see AGENTS.md → "Code
   Standards")
3. Ensure `make ci` passes locally
4. If your change touches session/watchdog/harness/schedule behavior, also run
   `make e2e` locally (not part of `make ci` — see "Testing" above)
5. If your change is a significant architectural or product decision, add an ADR under
   `docs/adr/` (see AGENTS.md → "Recording Decisions")
6. Open a pull request with a clear description of the change

## Code Style

Formatting, linting, and the `unsafe`/`.unwrap()` rules are documented once, in
AGENTS.md → "Code Standards" and "Security Rules" — this file doesn't repeat them.
Run `make fmt` and `make lint` before committing either way.

## License

By contributing, you agree that your contributions will be dual-licensed under MIT and Apache-2.0.
