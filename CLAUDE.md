# Norn Development Guide

Agent session orchestrator — manages coding agents across Tailscale-connected machines.

## Architecture

Rust workspace with three crates + a Svelte web UI:

- `crates/nornd/` — Daemon binary (`nornd`). Axum HTTP server, tmux backend, SQLite store.
- `crates/norn-cli/` — CLI binary (`norn`). Thin client that talks to `nornd`'s REST API.
- `crates/norn-common/` — Shared types (Session, Provider, NodeInfo, API request/response types).
- `web/` — Svelte 5 + SvelteKit + TypeScript. Static SPA built with `adapter-static`, embedded into the `nornd` binary via `rust-embed` for distribution.

See `SPEC.md` for the full architecture spec, session lifecycle, API design, and phase roadmap.

## Quick Start

```bash
# First-time setup (installs tools, git hooks, web dependencies)
make setup

# Run the daemon locally (port 7433)
cargo run -p nornd

# Run the web UI dev server (port 5173, proxies /api to nornd)
cd web && npm run dev

# Run all checks (what pre-commit runs)
make all
```

## Code Standards

### Formatting

- **Rust**: `cargo fmt` (config in `rustfmt.toml` — 100 char width, edition 2024)
- **Web**: `prettier` (config in `web/.prettierrc` — single quotes, trailing commas, 100 char width)
- Run `make fmt` to format everything. Run `make fmt-check` to verify without modifying.

### Linting

- **Rust**: `clippy` with strict settings — `deny(warnings)`, `warn(pedantic, nursery)`, `forbid(unsafe_code)`. Configured in workspace `Cargo.toml` under `[workspace.lints]`.
- **Web**: `eslint` with TypeScript and Svelte plugins (config in `web/eslint.config.js`), plus `svelte-check` for type checking.
- Run `make lint` to lint everything.

### Testing

- **Rust**: `cargo test --workspace`. Tests live alongside source code in `#[cfg(test)] mod tests` blocks.
- **Web**: `vitest` with jsdom environment. Test files use `*.test.ts` or `*.spec.ts` naming.
- Run `make test` to run all tests.

### Coverage

- **Target: 100% line coverage** for Rust code.
- Uses `cargo-llvm-cov`. Run `make coverage` to check (fails if under 100%).
- Run `make coverage-html` for an HTML report at `target/llvm-cov/html/index.html`.
- Every new function, branch, and error path must have a test. No exceptions.

### Pre-commit Hooks

Git hooks live in `.githooks/` and are activated via `git config core.hooksPath .githooks` (done by `make setup`). The pre-commit hook runs:

1. `cargo fmt --check` + `prettier --check`
2. `cargo clippy -- -D warnings`
3. `eslint` + `svelte-check`
4. `cargo test` + `vitest run`

**If the hook blocks your commit, fix the issue — do not bypass with `--no-verify`.**

## Development Workflow

### Adding a new API endpoint

1. Define request/response types in `crates/norn-common/src/api.rs`
2. Add the handler in `crates/nornd/src/api/` (e.g., `sessions.rs`)
3. Register the route in `crates/nornd/src/api/routes.rs`
4. Add the CLI subcommand in `crates/norn-cli/src/main.rs`
5. Add the API client function in `web/src/lib/api.ts`
6. Write tests for the handler (Rust integration test with `axum-test`)
7. Write tests for the web API client if non-trivial

### Adding a new backend feature (tmux/Docker)

1. Add the method to the `Backend` trait in `crates/nornd/src/backend/mod.rs`
2. Implement in `tmux.rs` (and `docker.rs` if applicable)
3. Write unit tests that mock tmux commands

### Writing tests

**Rust tests:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // ...
    }

    #[tokio::test]
    async fn test_async_thing() {
        // ...
    }
}
```

**Web tests** (`src/lib/api.test.ts`):
```typescript
import { describe, it, expect } from 'vitest';

describe('api', () => {
  it('should fetch sessions', async () => {
    // ...
  });
});
```

## Key Conventions

- **Error handling**: Use `anyhow::Result` for application errors, `thiserror` for library errors in `norn-common`.
- **Async**: All I/O is async via `tokio`. Backend trait methods are sync (tmux commands are fast) but called from async context via `tokio::task::spawn_blocking` when needed.
- **Naming**: Session names are kebab-case. tmux sessions are prefixed with `norn-` (e.g., `norn-my-api`).
- **Database**: SQLite via `sqlx`. Migrations are inline in `store/mod.rs` for now. Use `sqlx::query!` macro for compile-time checked queries when possible.
- **Config**: TOML config at `~/.norn/config.toml`. All fields have sensible defaults — nornd runs with zero config.
- **Logging**: Use `tracing` macros (`info!`, `warn!`, `error!`, `debug!`). Set level via `RUST_LOG` env var.
- **No `unsafe` code** — `forbid(unsafe_code)` is set workspace-wide.
- **No `.unwrap()`** in production code — use `?` or handle the error. `.unwrap()` is fine in tests.

## Makefile Targets

| Target | Description |
|--------|-------------|
| `make setup` | First-time setup: install tools, git hooks, web deps |
| `make all` | Format + lint + test (what pre-commit runs) |
| `make fmt` | Format all code (Rust + web) |
| `make fmt-check` | Check formatting without modifying |
| `make lint` | Run all linters (clippy + eslint + svelte-check) |
| `make test` | Run all tests (Rust + web) |
| `make coverage` | Run Rust tests with 100% coverage check |
| `make coverage-html` | Generate HTML coverage report |
| `make build` | Build release binary with embedded web UI |
| `make check` | Quick compile check (fastest feedback loop) |
| `make ci` | Full CI pipeline: fmt-check + lint + test + coverage |
| `make clean` | Remove all build artifacts |

## Project Layout

```
norn/
├── CLAUDE.md                     # This file
├── SPEC.md                       # Architecture spec and roadmap
├── Cargo.toml                    # Workspace root + shared deps + lints
├── Makefile                      # All development commands
├── rustfmt.toml                  # Rust formatter config
├── clippy.toml                   # Clippy config
├── .githooks/pre-commit          # Pre-commit hook (activated by make setup)
├── .gitignore
├── LICENSE-MIT
├── LICENSE-APACHE
├── crates/
│   ├── nornd/src/
│   │   ├── main.rs               # Daemon entry point
│   │   ├── config.rs             # TOML config loading
│   │   ├── platform.rs           # OS detection (macOS/Linux/WSL2)
│   │   ├── api/                  # Axum REST API
│   │   │   ├── mod.rs            # AppState, router setup
│   │   │   ├── routes.rs         # Route definitions
│   │   │   ├── sessions.rs       # Session CRUD handlers
│   │   │   └── node.rs           # Node info handler
│   │   ├── backend/              # Terminal backends
│   │   │   ├── mod.rs            # Backend trait
│   │   │   ├── tmux.rs           # tmux backend (macOS/Linux)
│   │   │   └── docker.rs         # Docker+tmux backend (Windows, Phase 4)
│   │   ├── session/              # Session lifecycle
│   │   │   ├── manager.rs        # Orchestration (spawn, kill, resume)
│   │   │   ├── output.rs         # Output capture
│   │   │   └── state.rs          # State machine
│   │   ├── store/                # Persistence
│   │   │   └── mod.rs            # SQLite store + migrations
│   │   └── peers/                # Peer discovery (Phase 3)
│   │       └── mod.rs
│   ├── norn-cli/src/
│   │   └── main.rs               # CLI subcommands
│   └── norn-common/src/
│       ├── lib.rs
│       ├── session.rs            # Session, Provider, SessionStatus types
│       ├── node.rs               # NodeInfo type
│       └── api.rs                # API request/response types
└── web/                          # Svelte 5 + SvelteKit + TypeScript
    ├── src/
    │   ├── routes/               # SvelteKit pages
    │   │   ├── +layout.svelte    # Root layout
    │   │   └── +page.svelte      # Dashboard
    │   └── lib/
    │       ├── api.ts            # API client
    │       └── components/       # Svelte components
    ├── eslint.config.js
    ├── .prettierrc
    └── vite.config.ts            # Vite config + vitest + API proxy
```
