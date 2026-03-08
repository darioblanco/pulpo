# Pulpo Development Guide

Agent session orchestrator — manages coding agents across Tailscale-connected machines.

## Architecture

Rust workspace with three crates + a React web UI:

- `crates/pulpod/` — Daemon binary (`pulpod`). Axum HTTP server, tmux backend, SQLite store.
- `crates/pulpo-cli/` — CLI binary (`pulpo`). Thin client that talks to `pulpod`'s REST API.
- `crates/pulpo-common/` — Shared types (Session, Provider, NodeInfo, API request/response types).
- `web/` — React 19 + Vite + Tailwind CSS v4 + shadcn/ui. Static SPA embedded into the `pulpod` binary via `rust-embed` for distribution.

See `SPEC.md` for the full architecture spec, session lifecycle, API design, and phase roadmap.

## Quick Start

```bash
# First-time setup (installs tools, git hooks, web dependencies)
make setup

# Run the daemon locally (port 7433)
cargo run -p pulpod

# Run the web UI dev server (port 5173, proxies /api to pulpod)
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
- **Web**: `eslint` with TypeScript and React plugins (config in `web/eslint.config.js`), plus `tsc --noEmit` for type checking.
- Run `make lint` to lint everything.

### Testing — Test-Driven Development (TDD)

This project follows **TDD**. Every feature and bug fix starts with a failing test:

1. **Write the test first** — define the expected behavior before writing implementation.
2. **Run the test** — confirm it fails for the right reason.
3. **Write the minimal implementation** to make the test pass.
4. **Refactor** — clean up while keeping tests green.
5. **Check coverage** — `make coverage` must pass (100% line coverage).

- **Rust**: `cargo test --workspace`. Tests live alongside source code in `#[cfg(test)] mod tests` blocks.
- **Web**: `vitest` with jsdom environment. Test files use `*.test.ts` or `*.spec.ts` naming.
- Run `make test` to run all tests.

### Coverage

- **Target: 100% line coverage** for Rust code. Enforced in pre-commit hook and CI.
- Uses `cargo-llvm-cov`. Run `make coverage` to check (fails if under 100%).
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

2. **Untestable I/O** — functions that require real infrastructure (mDNS, PTY spawn, MCP stdio) are gated with `#[cfg(not(coverage))]` on the function itself.

3. **Dead code under coverage** — helpers that become unused when their callers are excluded use `#[cfg_attr(coverage, allow(dead_code))]` to suppress warnings.

**Tiered enforcement:**
- Local pre-commit: **99%** line coverage (strict gate via `cargo-llvm-cov`)
- CI (Linux): **99%** threshold (macOS-specific paths unreachable on Linux)
- `main.rs` and `embed.rs` excluded via `cargo-llvm-cov` filename regex

> **Note:** `cargo-llvm-cov 0.8+` counts `?` error-path regions as "missed lines" even when the line itself executes, making true 100% impossible without testing every `Result::Err` branch. The 99% threshold accounts for this.

**When to exclude:** Only for genuinely untestable I/O (process spawning, network listeners, real hardware). All business logic must be testable and tested. Do not use `cfg(coverage)` to skip testable code.

### Pre-commit Hooks

Git hooks live in `.githooks/` and are activated via `git config core.hooksPath .githooks` (done by `make setup`). The pre-commit hook runs:

1. `cargo fmt --check` + `prettier --check`
2. `cargo clippy -- -D warnings`
3. `eslint` + `tsc --noEmit`
4. `cargo test` + `vitest run`
5. `cargo llvm-cov --fail-under-lines 99` (line coverage gate)

**If the hook blocks your commit, fix the issue — do not bypass with `--no-verify`.**

## Development Workflow

### Adding a new API endpoint

1. **Write tests first** for the handler and types (TDD).
2. Define request/response types in `crates/pulpo-common/src/api.rs`
3. Add the handler in `crates/pulpod/src/api/` (e.g., `sessions.rs`)
4. Register the route in `crates/pulpod/src/api/routes.rs`
5. Add integration tests in `routes.rs` using `axum-test::TestServer`
6. Add the CLI subcommand in `crates/pulpo-cli/src/lib.rs`
7. Add the API client function in `web/src/lib/api.ts`
8. Verify `make coverage` passes before committing.

### Adding a new backend feature (tmux/Docker)

1. **Write tests first** for command construction (TDD).
2. Add the method to the `Backend` trait in `crates/pulpod/src/backend/mod.rs`
3. Implement in `tmux.rs` (and `docker.rs` if applicable)
4. Test command building by inspecting `Command::get_args()` — do not execute tmux in tests.

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

- **Error handling**: Use `anyhow::Result` for application errors, `thiserror` for library errors in `pulpo-common`.
- **Async**: All I/O is async via `tokio`. Backend trait methods are sync (tmux commands are fast) but called from async context via `tokio::task::spawn_blocking` when needed.
- **Naming**: Session names are kebab-case. tmux sessions are prefixed with `pulpo-` (e.g., `pulpo-my-api`).
- **Database**: SQLite via `sqlx`. Migrations are inline in `store/mod.rs` for now. Use `sqlx::query!` macro for compile-time checked queries when possible.
- **Config**: TOML config at `~/.pulpo/config.toml`. All fields have sensible defaults — pulpod runs with zero config.
- **Logging**: Use `tracing` macros (`info!`, `warn!`, `error!`, `debug!`). Set level via `RUST_LOG` env var.
- **No `unsafe` code** — `forbid(unsafe_code)` is set workspace-wide.
- **No `.unwrap()`** in production code — use `?` or handle the error. `.unwrap()` is fine in tests.

## Makefile Targets

| Target | Description |
|--------|-------------|
| `make setup` | First-time setup: install tools, git hooks, web deps |
| `make dev` | Run the daemon from source (port 7433) with repo-local config |
| `make dev-web` | Run the web UI dev server (port 5173, proxies to pulpod) |
| `make all` | Format + lint + test (what pre-commit runs) |
| `make fmt` | Format all code (Rust + web) |
| `make fmt-check` | Check formatting without modifying |
| `make lint` | Run all linters (clippy + eslint + tsc) |
| `make test` | Run all tests (Rust + web) |
| `make test-web-watch` | Run web tests in watch mode |
| `make coverage` | Run Rust tests with 100% coverage check |
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
│   └── discord-bot/              # Discord bot for controlling pulpod
│       ├── src/
│       │   ├── index.ts          # Entry: init client, register commands, autocomplete
│       │   ├── config.ts         # Env-based config
│       │   ├── commands/         # Slash commands
│       │   │   ├── spawn.ts      # /spawn — create a new session
│       │   │   ├── status.ts     # /status — show session(s)
│       │   │   ├── logs.ts       # /logs — recent output
│       │   │   ├── kill.ts       # /kill — terminate session
│       │   │   ├── resume.ts     # /resume — resume stale session
│       │   │   ├── inks.ts       # /inks — list inks
│       │   │   └── input.ts      # /input — send text to session
│       │   ├── listeners/sse.ts  # EventSource → Discord channel messages
│       │   ├── api/pulpod.ts     # HTTP client for pulpod REST API
│       │   └── formatters/embed.ts # Discord embed builders
│       ├── package.json
│       └── tsconfig.json
├── crates/
│   ├── pulpod/src/
│   │   ├── main.rs               # Thin entry point (cfg(coverage) excluded)
│   │   ├── lib.rs                # Daemon logic: Cli, init_tracing, build_app
│   │   ├── config.rs             # TOML config loading
│   │   ├── guard.rs              # Guard config resolution + flag building
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
│   │   │   ├── inks.rs           # Inks endpoint
│   │   │   ├── events.rs         # SSE event stream endpoint
│   │   │   ├── static_files.rs   # rust-embed static file serving
│   │   │   └── embed.rs          # rust-embed derive (excluded from coverage)
│   │   ├── backend/              # Terminal backends
│   │   │   ├── mod.rs            # Backend trait
│   │   │   └── tmux.rs           # tmux backend (macOS/Linux)
│   │   ├── session/              # Session lifecycle
│   │   │   ├── mod.rs            # Session module
│   │   │   ├── manager.rs        # Orchestration (spawn, kill, resume)
│   │   │   └── pty_bridge.rs     # PTY bridge for WebSocket streaming
│   │   ├── store/                # Persistence
│   │   │   └── mod.rs            # SQLite store + migrations
│   │   ├── notifications/        # Push notifications
│   │   │   ├── mod.rs            # Module declaration
│   │   │   └── discord.rs        # Discord webhook notifier + loop
│   │   ├── peers/                # Peer discovery
│   │   │   ├── mod.rs            # PeerRegistry
│   │   │   └── health.rs         # Peer health probing (cached on-demand)
│   │   ├── watchdog/             # Resource monitoring
│   │   │   ├── mod.rs            # Watchdog loop (memory + idle detection)
│   │   │   └── memory.rs         # System memory probing
│   │   ├── mcp/                  # MCP server
│   │   │   ├── mod.rs            # MCP tool handlers
│   │   │   └── resources.rs      # MCP resource definitions
│   │   └── discovery/            # Peer discovery (mDNS, Tailscale, seed)
│   │       ├── mod.rs            # ServiceRegistration types + constants
│   │       ├── mdns.rs           # mDNS register, browse, shutdown
│   │       ├── tailscale.rs      # Tailscale API peer discovery
│   │       └── seed.rs           # Seed-based gossip peer discovery
│   ├── pulpo-cli/src/
│   │   ├── main.rs               # Thin entry point (cfg(coverage) excluded)
│   │   └── lib.rs                # CLI logic: Cli, Commands, execute
│   └── pulpo-common/src/
│       ├── lib.rs
│       ├── session.rs            # Session, Provider, SessionStatus types
│       ├── node.rs               # NodeInfo type
│       ├── peer.rs               # PeerInfo, PeerStatus types
│       ├── guard.rs              # GuardConfig type (binary unrestricted toggle)
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
    │   │   ├── session/          # Chat view, terminal view (xterm.js)
    │   │   ├── history/          # Session filter, session list
    │   │   ├── settings/         # Node, guard, peer settings
    │   │   └── connect/          # Connect form, saved connections
    │   └── pages/
    │       ├── dashboard.tsx     # Real-time session dashboard
    │       ├── history.tsx       # Session history with search/filter
    │       ├── settings.tsx      # Node, guards, peers config
    │       └── connect.tsx       # Connection screen (standalone)
    ├── eslint.config.js
    ├── .prettierrc
    ├── vite.config.ts            # Vite config + Tailwind plugin + API proxy
    └── vitest.config.ts          # Vitest config
```
