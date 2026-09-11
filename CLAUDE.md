# Pulpo Development Guide

Self-hosted meter and breaker box for coding agents — runs them as durable background
workers on a machine you own, meters exactly what they cost, enforces budgets before you
blow a limit, and stays reachable remotely over Tailscale.

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
> Deploying `pulpod` itself in a container (`bind = "container"`, `docker/`) was also
> removed — `pulpod` is installed via Homebrew or systemd on the machines it
> supervises, and a containerized `pulpod` can't see the agents' own session files
> that exact usage metering depends on.

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
- **Naming**: Session names are kebab-case, **validated server-side** by `validate_session_name()` in `session/utils.rs` (`[a-z0-9-]`, max 128 chars). This is security-critical — session names are interpolated into shell commands in `wrap_command`. Schedule names follow the same rules. Any new code path that accepts session/schedule names MUST validate them.
- **Exit markers**: `wrap_command` writes `{data_dir}/exit/{id}.code` (agent exit code) and `{id}.clean` (shell ended normally). A dead tmux session WITH a marker resolves to `Stopped` (clean end); without → `Lost` (crash). Markers are purged with the session and swept by `pulpo cleanup`.
- **Harness adapters**: `session/manager.rs` resolves a `HarnessAdapter` (`harness/`) at spawn/resume time to rewrite the command so the harness (Claude Code, Codex, pi) reports lifecycle events to `pulpo hook <harness>` → `POST /api/v1/sessions/{id}/harness-events`. Shipped for Claude Code (hook mechanics verified against v2.1.266), Codex and pi (implemented from their docs, unverified in the field). Once a session's `harness_last_event_at` is set, the watchdog stops applying scrollback-heuristic *detection* to it — but per-signal (`watchdog::owned_signals`/`HarnessAdapter::owned_signals`), not all-or-nothing: an adapter missing a signal (Codex has no error/rate-limit hook) keeps that one heuristic running from scrollback even while its lifecycle events flow. Memory intervention, `idle_timeout`, and the budget/burn fields always apply regardless of harness ownership. See `docs/architecture/harness-adapters.md`.
- **Session IDs**: `backend_session_id` stores the tmux `$N` session ID (monotonically increasing, never reused while tmux server runs). At startup, name-based IDs are upgraded to `$N` IDs.
- **Database**: SQLite via `sqlx`. Versioned schema migrations live in `crates/pulpod/migrations/`; `store/mod.rs` contains the runtime store API only. Use `sqlx::query!` macro for compile-time checked queries when possible.
- **Config**: TOML config at `~/.pulpo/config.toml`. All fields have sensible defaults — pulpod runs with zero config. Key watchdog config fields: `idle_threshold_secs` (seconds of unchanged output before Active→Idle, default 60), `waiting_patterns` (extra user-defined patterns appended to the built-in waiting-for-input patterns).
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
├── POSITIONING.md                # Positioning memo (ICPs, wedge, messaging)
├── MISSION.md                    # One-page mission and non-goals
├── CONTRIBUTING.md               # Contributor workflow
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
│   └── examples/webhook-discord/ # Reference webhook consumer (see docs/reference/config.md)
├── docs/                         # VuePress docs site (getting-started/guides/reference/architecture/operations)
├── examples/                     # Runnable CLI/API/config examples
├── scripts/                      # demo.sh, e2e.sh, install-pulpo.sh
├── .pulpo/config.toml.example    # `make dev` local-dev config template
├── crates/
│   ├── pulpod/src/
│   │   ├── main.rs               # Thin entry point (cfg(coverage) excluded)
│   │   ├── lib.rs                # Daemon logic: Cli, init_tracing, build_app
│   │   ├── config.rs             # TOML config loading
│   │   ├── platform.rs           # OS detection (macOS/Linux/WSL2)
│   │   ├── auth_info.rs          # Credential/plan extraction from agent config files
│   │   ├── coverage_macros.rs    # coverage_warn!/coverage_info! (keep rare log-only branches out of coverage)
│   │   ├── api/                  # Axum REST API
│   │   │   ├── mod.rs            # AppState, router setup
│   │   │   ├── routes.rs         # Route definitions + auth middleware
│   │   │   ├── auth.rs           # Bearer-token middleware + GET /auth/token endpoint
│   │   │   ├── config.rs         # Config API endpoint
│   │   │   ├── health.rs         # Health check endpoint
│   │   │   ├── sessions.rs       # Session CRUD + input/stop/resume/handoff/harness-events handlers
│   │   │   ├── sessions_tests.rs # Session handler tests (split out of sessions.rs)
│   │   │   ├── test_support.rs   # Shared API test fixtures/helpers
│   │   │   ├── node.rs           # Node info endpoint
│   │   │   ├── schedules.rs      # Schedule CRUD + run-history handlers
│   │   │   ├── notifications.rs  # Notification config endpoint
│   │   │   ├── push.rs           # Web Push subscribe/unsubscribe/action endpoints
│   │   │   ├── usage.rs          # Usage projection + scan endpoints
│   │   │   ├── metrics.rs        # Prometheus /metrics endpoint
│   │   │   ├── watchdog.rs       # Watchdog config endpoint
│   │   │   ├── ws.rs             # WebSocket terminal streaming
│   │   │   ├── events.rs         # SSE event stream endpoint
│   │   │   ├── error.rs          # Shared API handler error type
│   │   │   ├── static_files.rs   # rust-embed static file serving
│   │   │   └── embed.rs          # rust-embed derive (excluded from coverage)
│   │   ├── backend/              # Terminal backends
│   │   │   ├── mod.rs            # Backend trait
│   │   │   └── tmux.rs           # tmux backend (macOS/Linux)
│   │   ├── session/              # Session lifecycle
│   │   │   ├── mod.rs            # Session module
│   │   │   ├── manager.rs        # Orchestration (spawn, stop, resume, handoff)
│   │   │   ├── pty_bridge.rs     # PTY bridge for WebSocket streaming
│   │   │   └── utils.rs          # Session/schedule name validation, workdir checks
│   │   ├── store/                # Persistence (SQLite via sqlx)
│   │   │   ├── mod.rs            # Public store API + module wiring
│   │   │   ├── core.rs           # Store, migrations, test_store helper
│   │   │   ├── rows.rs           # SQLite row → domain type mapping
│   │   │   ├── sessions.rs       # Session CRUD queries
│   │   │   ├── schedules.rs      # Schedule CRUD queries
│   │   │   ├── session_metadata.rs      # Session metadata key/value queries
│   │   │   ├── session_interventions.rs # Intervention event queries
│   │   │   ├── outbox.rs         # Webhook outbox queries
│   │   │   ├── push.rs           # Push subscription queries
│   │   │   └── tests.rs          # Integration tests exercising the store API end-to-end
│   │   ├── notifications/        # Push + webhook notifications
│   │   │   ├── mod.rs            # Module declaration + dispatcher
│   │   │   ├── webhook.rs        # Signed webhook delivery (lifecycle/intervention/usage_alert/fleet)
│   │   │   ├── web_push.rs       # Web Push notifications (VAPID)
│   │   │   ├── outbox.rs         # Retry/backoff queue for webhook delivery
│   │   │   └── action_token.rs   # Signed action tokens (push "Stop" action)
│   │   ├── watchdog/             # Resource monitoring
│   │   │   ├── mod.rs            # Watchdog loop (memory + idle detection)
│   │   │   ├── idle.rs           # Idle detection + status transitions
│   │   │   ├── metadata.rs       # PR/branch/rate-limit/error/usage scraping from output
│   │   │   ├── output_patterns.rs # Waiting-for-input/rate-limit/error/PR-URL pattern matching
│   │   │   ├── git.rs            # Branch/commit detection for sessions
│   │   │   ├── budget.rs         # Per-session cost budget alerts + auto-stop
│   │   │   ├── burn.rs           # Burn-rate ceiling governor (cost/token per hour)
│   │   │   ├── intervention.rs   # Shared stop-and-record path for forced session stops
│   │   │   ├── memory.rs         # System memory probing
│   │   │   └── tests.rs          # Integration tests exercising the watchdog loop end-to-end
│   │   ├── harness/              # Harness adapters (agent lifecycle events)
│   │   │   ├── mod.rs            # HarnessAdapter trait, HarnessEvent, state transitions
│   │   │   ├── registry.rs       # HarnessRegistry: resolve a command line to an adapter
│   │   │   ├── generic.rs        # Fallback adapter: no rewrite, no events
│   │   │   ├── claude.rs         # Claude Code adapter (hooks, --session-id/--resume)
│   │   │   ├── codex.rs          # Codex adapter (isolated CODEX_HOME + hooks/notify)
│   │   │   ├── pi.rs             # pi adapter (pulpo.ts extension, --session-id)
│   │   │   └── pulpo.ts.tmpl     # pi extension file template (embedded via include_str!)
│   │   ├── scheduler/mod.rs      # Cron schedule loop (60s tick, fires sessions)
│   │   └── usage/                # Structured usage readers (exact tokens/cost from agent files)
│   │       ├── mod.rs            # UsageReader plumbing, rate table, RateOverrides
│   │       ├── claude.rs         # Claude Code transcript reader
│   │       ├── codex.rs          # Codex rollout-file reader
│   │       ├── pi.rs             # pi session-file reader (scan only)
│   │       ├── pool.rs           # Billing-pool attribution (subscription vs headless)
│   │       ├── projection.rs     # Burn-rate/time-to-cap projection
│   │       └── scan.rs           # Read-only scan of all local agent history
│   ├── pulpo-cli/src/
│   │   ├── main.rs               # Thin entry point (cfg(coverage) excluded)
│   │   ├── lib.rs                # CLI logic: Cli, Commands, execute
│   │   ├── hook.rs               # `pulpo hook <harness>` internal subcommand (lifecycle events → daemon)
│   │   ├── format.rs             # Terminal output rendering (tables/reports)
│   │   └── http.rs               # HTTP client helpers (auth, base-URL/token resolution)
│   └── pulpo-common/src/
│       ├── lib.rs
│       ├── session.rs            # Session, SessionStatus, InterventionCode types
│       ├── node.rs               # NodeInfo type
│       ├── event.rs              # SessionEvent for SSE + notifications
│       ├── auth.rs               # BindMode (local/tailscale/public)
│       └── api.rs                # API request/response types
└── web/                          # React 19 + Vite + Tailwind v4 + shadcn/ui
    ├── src/
    │   ├── index.css             # Tailwind imports + dark theme CSS vars
    │   ├── main.tsx              # Entry point
    │   ├── App.tsx                # React Router setup
    │   ├── sw.ts                  # Service worker (push notifications)
    │   ├── api/
    │   │   ├── types.ts          # Shared TypeScript interfaces
    │   │   ├── client.ts         # API fetch functions
    │   │   └── connection.ts     # testConnection
    │   ├── hooks/
    │   │   ├── use-connection.tsx      # Connection context (baseUrl, token, saved)
    │   │   ├── use-sse.tsx             # SSE event stream + session state
    │   │   ├── use-push-notifications.tsx # Push subscribe/unsubscribe
    │   │   ├── use-schedules-filter.ts # Schedule list filtering
    │   │   └── use-mobile.ts           # Mobile breakpoint detection
    │   ├── lib/
    │   │   ├── utils.ts          # cn() helper, formatDuration, formatSessionStatus
    │   │   ├── notifications.ts  # Desktop notification helpers
    │   │   ├── cron.ts           # Cron expression parsing/formatting
    │   │   └── push-sw.ts        # Service-worker-side push payload handling
    │   ├── components/
    │   │   ├── ui/               # shadcn generated components
    │   │   ├── layout/           # Sidebar, header, app shell, disconnected banner
    │   │   ├── dashboard/        # Status summary, node/session cards, new session dialog
    │   │   ├── session/          # Output view, terminal view (ghostty-web)
    │   │   ├── schedules/        # Schedule dialog, run-history panel, schedule row
    │   │   ├── history/          # Session filter (reused by dashboard)
    │   │   ├── settings/         # Node, watchdog, notifications settings
    │   │   └── connect/          # Connect form, saved connections
    │   └── pages/
    │       ├── dashboard.tsx     # Sessions list (the landing page)
    │       ├── session-detail.tsx # Single-session detail view
    │       ├── schedules.tsx     # Schedule management
    │       ├── usage.tsx         # Usage/cost gauge
    │       ├── settings.tsx      # Node, watchdog, notifications config
    │       └── connect.tsx       # Connection screen (standalone)
    ├── eslint.config.js
    ├── .prettierrc
    ├── vite.config.ts            # Vite config + Tailwind plugin + API proxy
    └── vitest.config.ts          # Vitest config
```
