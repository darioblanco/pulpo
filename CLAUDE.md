# Pulpo Development Guide

Agent session orchestrator — manages coding agents across Tailscale-connected machines.

## Architecture

Rust workspace with three crates + a React web UI:

- `crates/pulpod/` — Daemon binary (`pulpod`). Axum HTTP server, tmux backend, SQLite store.
- `crates/pulpo-cli/` — CLI binary (`pulpo`). Thin client that talks to `pulpod`'s REST API.
- `crates/pulpo-common/` — Shared types (Session, NodeInfo, API request/response types).
- `web/` — React 19 + Vite + Tailwind CSS v4 + shadcn/ui. Static SPA embedded into the `pulpod` binary via `rust-embed` for distribution.

See `SPEC.md` for the full architecture spec, session lifecycle, API design, and phase roadmap.

## Quick Start

```bash
# First-time setup (installs tools, git hooks, web dependencies)
make setup

# Run pulpod from source (stops homebrew, uses .pulpo/config.toml)
make dev

# Run the web UI dev server (port 5173, proxies /api to pulpod)
make dev-web

# When done: Ctrl+C pulpod, then restore homebrew
make dev-stop

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
- **Web**: `eslint` with TypeScript and React plugins (config in `web/eslint.config.js`), plus `tsc --noEmit` for type checking.
- Run `make lint` to lint everything.

### Testing — Test-Driven Development (TDD)

This project follows **TDD**. Every feature and bug fix starts with a failing test:

1. **Write the test first** — define the expected behavior before writing implementation.
2. **Run the test** — confirm it fails for the right reason.
3. **Write the minimal implementation** to make the test pass.
4. **Refactor** — clean up while keeping tests green.
5. **Run the quality gates** — `make ci` must pass.

- **Rust**: `cargo test --workspace`. Tests live alongside source code in `#[cfg(test)] mod tests` blocks.
- **Web**: `vitest` with jsdom environment. Test files use `*.test.ts` or `*.spec.ts` naming.
- Run `make test` to run all tests.

### Coverage

- Rust coverage is enforced by the executable gate `make coverage-rust`.
- The full local quality gate is `make ci`.
- Uses `cargo-llvm-cov`. Run `make coverage` for Rust + web coverage, or `make coverage-html` for an HTML report.
- Run `make coverage-html` for an HTML report at `target/llvm-cov/html/index.html`.
- Every new function, branch, and error path must have a test. No exceptions.
- `main.rs` files are excluded from coverage — they are thin `#[cfg(not(coverage))]` wrappers. All logic lives in `lib.rs`.
- `embed.rs` is excluded from coverage — it contains only the `#[derive(Embed)]` macro for `rust-embed`, which generates uncoverable code.

#### Coverage exclusion patterns

We use `#[cfg(coverage)]` / `#[cfg(not(coverage))]` to exclude genuinely untestable code. The `#[coverage(off)]` attribute would be cleaner (code still compiles and runs, just isn't measured), but its stabilization was reverted (rust-lang/rust#134672) and it remains unstable. Track rust-lang/rust#84605 for status — when it stabilizes on stable Rust, migrate the ~47 occurrences across ~14 files.

**Three patterns in use:**

1. **Binary entry points** — `main.rs` files use dual `cfg` to provide a no-op main under coverage:
```rust
#[cfg(not(coverage))]
#[tokio::main]
async fn main() -> anyhow::Result<()> { /* ... */ }

#[cfg(coverage)]
fn main() {}
```

2. **Untestable I/O** — functions that require real infrastructure (PTY spawn) are gated with `#[cfg(not(coverage))]` on the function itself.

3. **Dead code under coverage** — helpers that become unused when their callers are excluded use `#[cfg_attr(coverage, allow(dead_code))]` to suppress warnings.

**Enforced threshold:**
- Local and CI Rust coverage gate: **98%** line coverage via `make coverage-rust`
- `main.rs` and `embed.rs` excluded via `cargo-llvm-cov` filename regex

> **Note:** `cargo-llvm-cov 0.8+` counts `?` error-path regions as "missed lines" even when the line itself executes, and `cfg(coverage)` exclusions for I/O code further reduce the measurable surface. The 98% threshold accounts for this.

**When to exclude:** Only for genuinely untestable I/O (process spawning, network listeners, real hardware). All business logic must be testable and tested. Do not use `cfg(coverage)` to skip testable code.

### Pre-commit Hooks

Git hooks live in `.githooks/` and are activated via `git config core.hooksPath .githooks` (done by `make setup`). The pre-commit hook runs:

1. `cargo fmt --check` + `prettier --check`
2. `cargo clippy -- -D warnings`
3. `eslint` + `tsc --noEmit`
4. `cargo test` + `vitest run`
5. `make coverage-rust` (Rust coverage gate)

**If the hook blocks your commit, fix the issue — do not bypass with `--no-verify`.**

## Development Workflow

### Adding a new API endpoint

1. **Write tests first** for the handler and types (TDD).
2. Define request/response types in `crates/pulpo-common/src/api.rs`
3. Add the handler in `crates/pulpod/src/api/` (e.g., `sessions.rs`)
4. Register the route in `crates/pulpod/src/api/routes.rs`
5. Add integration tests in `routes.rs` using `axum-test::TestServer`
6. Add the CLI subcommand in `crates/pulpo-cli/src/lib.rs`
7. Add the API client function in `web/src/api/client.ts`
8. Verify `make ci` passes before committing.

### Adding a new backend feature (tmux)

1. **Write tests first** for command construction (TDD).
2. Add the method to the `Backend` trait in `crates/pulpod/src/backend/mod.rs`
3. Implement in `tmux.rs`
4. Test command building by inspecting `Command::get_args()` — do not execute tmux in tests.

> The Docker **session runtime** was removed (`--runtime docker`, `backend/docker.rs`).
> Historical sessions stored with `runtime = "docker"` remain readable; spawning,
> resuming, or scheduling with the docker runtime is rejected server-side.
> Deploying pulpod itself in a container (`bind = "container"`, `docker/`) is unaffected.

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

**Integration tests with axum-test:**
```rust
#[cfg(test)]
mod tests {
    use axum_test::TestServer;

    async fn test_server() -> TestServer {
        let app = build_test_router().await;
        TestServer::new(app).unwrap()
    }

    #[tokio::test]
    async fn test_endpoint() {
        let server = test_server().await;
        server.get("/api/v1/endpoint").await.assert_status_ok();
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

- **Error handling**: Use `anyhow::Result` for application errors; API errors use hand-rolled response types (e.g. `ErrorResponse` in `pulpo-common`).
- **Async**: All I/O is async via `tokio`. Backend trait methods are sync (tmux commands are fast) but called from async context via `tokio::task::spawn_blocking` when needed.
- **Naming**: Session names are kebab-case, **validated server-side** by `validate_session_name()` in `session/manager.rs` (`[a-z0-9-]`, max 128 chars). This is security-critical — session names are interpolated into shell commands in `wrap_command`. Schedule names follow the same rules. Any new code path that accepts session/schedule names MUST validate them.
- **Session IDs**: `backend_session_id` stores the tmux `$N` session ID (monotonically increasing, never reused while tmux server runs). At startup, name-based IDs are upgraded to `$N` IDs.
- **Database**: SQLite via `sqlx`. Versioned schema migrations live in `crates/pulpod/migrations/`; `store/mod.rs` contains the runtime store API only. Use `sqlx::query!` macro for compile-time checked queries when possible.
- **Config**: TOML config at `~/.pulpo/config.toml`. All fields have sensible defaults — pulpod runs with zero config. Key watchdog config fields: `idle_threshold_secs` (seconds of unchanged output before Active→Idle, default 60), `waiting_patterns` (extra user-defined patterns appended to the 29 built-in waiting-for-input patterns).
- **Per-session idle**: Sessions accept `idle_threshold_secs: Option<u32>` — `None` = use global, `Some(0)` = never idle, `Some(N)` = N seconds. CLI: `pulpo spawn <name> --idle-threshold <secs>`.
- **Logging**: Use `tracing` macros (`info!`, `warn!`, `error!`, `debug!`). Set level via `RUST_LOG` env var.
- **No `unsafe` code** — `forbid(unsafe_code)` is set workspace-wide.
- **No `.unwrap()`** in production code — use `?` or handle the error. `.unwrap()` is fine in tests.

## Makefile Targets

| Target | Description |
|--------|-------------|
| `make setup` | First-time setup: install tools, git hooks, web deps |
| `make dev` | Run pulpod from source (stops homebrew, uses .pulpo/config.toml) |
| `make dev-stop` | Stop local dev and restore the homebrew service |
| `make dev-web` | Run the web UI dev server (port 5173, proxies to pulpod) |
| `make all` | Format + lint + test (what pre-commit runs) |
| `make fmt` | Format all code (Rust + web) |
| `make fmt-check` | Check formatting without modifying |
| `make lint` | Run all linters (clippy + eslint + tsc) |
| `make test` | Run all tests (Rust + web) |
| `make test-web-watch` | Run web tests in watch mode |
| `make coverage` | Run coverage checks (Rust + web) |
| `make coverage-rust` | Run the Rust coverage gate |
| `make coverage-html` | Generate HTML coverage report |
| `make build` | Build release binary with embedded web UI |
| `make release` | Build release binaries to `dist/` |
| `make install` | Install binaries to `/usr/local/bin` |
| `make service-install` | Install and start launchd service (macOS) |
| `make service-uninstall` | Stop and remove launchd service (macOS) |
| `make service-install-linux` | Install and enable systemd user service (Linux) |
| `make service-uninstall-linux` | Disable and remove systemd user service (Linux) |
| `make deploy-server` | SCP pulpod to `DEPLOY_HOST` + restart systemd service |
| `make check` | Quick compile check (fastest feedback loop) |
| `make ci` | Full CI pipeline: fmt-check + lint + test + coverage |
| `make clean` | Remove all build artifacts + dev data (.pulpo/data/) |
| `make sweep` | Prune build artifacts older than 7 days (cargo-sweep) |

## Project Layout

```
pulpo/
├── CLAUDE.md                     # This file
├── AGENTS.md                     # Agent guardrails (concise, for any coding agent)
├── SPEC.md                       # Architecture spec and lifecycle design
├── ROADMAP.md                    # Project sequencing and next steps
├── Cargo.toml                    # Workspace root + shared deps + lints
├── Makefile                      # All development commands
├── rustfmt.toml                  # Rust formatter config
├── clippy.toml                   # Clippy config
├── .githooks/pre-commit          # Pre-commit hook (activated by make setup)
├── .gitignore
├── LICENSE-MIT
├── LICENSE-APACHE
├── contrib/
│   ├── com.pulpo.daemon.plist    # macOS launchd service definition
│   ├── pulpo.service             # Linux systemd user service
├── crates/
│   ├── pulpod/src/
│   │   ├── main.rs               # Thin entry point (cfg(coverage) excluded)
│   │   ├── lib.rs                # Daemon logic: Cli, init_tracing, build_app
│   │   ├── config.rs             # TOML config loading
│   │   ├── platform.rs           # OS detection (macOS/Linux/WSL2)
│   │   ├── api/                  # Axum REST API
│   │   │   ├── mod.rs            # AppState, router setup
│   │   │   ├── routes.rs         # Route definitions + auth middleware
│   │   │   ├── auth.rs           # Auth token endpoint
│   │   │   ├── config.rs         # Config API endpoint
│   │   │   ├── health.rs         # Health check endpoint
│   │   │   ├── sessions.rs       # Session CRUD handlers
│   │   │   ├── node.rs           # Node info + memory detection
│   │   │   ├── peers.rs          # Peers endpoint
│   │   │   ├── ws.rs             # WebSocket terminal streaming
│   │   │   ├── events.rs         # SSE event stream endpoint
│   │   │   ├── static_files.rs   # rust-embed static file serving
│   │   │   └── embed.rs          # rust-embed derive (excluded from coverage)
│   │   ├── backend/              # Terminal backends
│   │   │   ├── mod.rs            # Backend trait
│   │   │   └── tmux.rs           # tmux backend (macOS/Linux)
│   │   ├── session/              # Session lifecycle
│   │   │   ├── mod.rs            # Session module
│   │   │   ├── manager.rs        # Orchestration (spawn, stop, resume)
│   │   │   └── pty_bridge.rs     # PTY bridge for WebSocket streaming
│   │   ├── store/                # Persistence
│   │   │   └── mod.rs            # SQLite store API
│   │   ├── notifications/        # Push notifications
│   │   │   ├── mod.rs            # Module declaration
│   │   │   └── discord.rs        # Discord webhook notifier + loop
│   │   ├── peers/                # Peer discovery
│   │   │   ├── mod.rs            # PeerRegistry
│   │   │   └── health.rs         # Peer health probing (cached on-demand)
│   │   ├── watchdog/             # Resource monitoring
│   │   │   ├── mod.rs            # Watchdog loop (memory + idle detection)
│   │   │   └── memory.rs         # System memory probing
│   │   └── discovery/            # Peer discovery (Tailscale)
│   │       ├── mod.rs            # Discovery types + constants
│   │       └── tailscale.rs      # Tailscale API peer discovery
│   ├── pulpo-cli/src/
│   │   ├── main.rs               # Thin entry point (cfg(coverage) excluded)
│   │   └── lib.rs                # CLI logic: Cli, Commands, execute
│   └── pulpo-common/src/
│       ├── lib.rs
│       ├── session.rs            # Session, SessionStatus types
│       ├── node.rs               # NodeInfo type
│       ├── peer.rs               # PeerInfo, PeerStatus types
│       ├── event.rs              # SessionEvent for SSE + notifications
│       └── api.rs                # API request/response types
└── web/                          # React 19 + Vite + Tailwind v4 + shadcn/ui
    ├── src/
    │   ├── index.css             # Tailwind imports + dark theme CSS vars
    │   ├── main.tsx              # Entry point
    │   ├── App.tsx               # React Router setup
    │   ├── api/
    │   │   ├── types.ts          # Shared TypeScript interfaces
    │   │   ├── client.ts         # API fetch functions (20+)
    │   │   └── connection.ts     # testConnection, discoverPeers
    │   ├── hooks/
    │   │   ├── use-connection.tsx # Connection context (baseUrl, token, saved)
    │   │   └── use-sse.tsx       # SSE event stream + session state
    │   ├── lib/
    │   │   ├── utils.ts          # cn() helper, formatDuration
    │   │   └── notifications.ts  # Desktop notification helpers
    │   ├── components/
    │   │   ├── ui/               # shadcn generated components
    │   │   ├── layout/           # Sidebar, header, app shell
    │   │   ├── dashboard/        # Status summary, node/session cards, new session
    │   │   ├── session/          # Chat view, terminal view (ghostty-web)
    │   │   ├── history/          # Session filter (reused by dashboard)
    │   │   ├── settings/         # Node, peer settings
    │   │   └── connect/          # Connect form, saved connections
    │   └── pages/
    │       ├── dashboard.tsx     # Sessions dashboard with status filters
    │       ├── schedules.tsx     # Schedule management
    │       ├── settings.tsx      # Node, peers config
    │       └── connect.tsx       # Connection screen (standalone)
    ├── eslint.config.js
    ├── .prettierrc
    ├── vite.config.ts            # Vite config + Tailwind plugin + API proxy
    └── vitest.config.ts          # Vitest config
```
