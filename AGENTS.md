# Pulpo Development Guide

Instructions for coding agents — Claude Code, Codex, pi, and any other compatible
tool. This is the single development guide for the project: `CLAUDE.md` is a one-line
pointer into this file (Claude Code's `@`-import syntax), and `CONTRIBUTING.md` is the
short, human-facing version that links into the sections below rather than repeating
them.

## Product Focus

Pulpo is a **self-hosted meter and breaker box for coding agents** — agent-agnostic
infrastructure you own. It runs any CLI agent (Claude Code, Codex, pi, Aider, Gemini
CLI, etc.) as a durable session on your machines, with a flat budget-cost cap (alert
80%, stop 100%), durable session lifecycle, hook-driven watchdog supervision, and
scheduling. Claude Code, Codex, and pi additionally get **exact usage metering**
(tokens/cost read from each agent's own session files, no scraping) and hook-driven
state instead of a scrollback guess; any other command still runs, but shows no usage.
Each machine's `pulpod` is standalone (no controller/fleet control plane); reach any of
them directly over Tailscale.

**Positioning:** infrastructure layer, not an agent. Sovereign by architecture — code
never leaves your infrastructure. Key differentiators: exact cross-agent cost metering,
a budget cap that actually intervenes, direct multi-machine access over Tailscale, EU
sovereignty compliance. See [ROADMAP.md](ROADMAP.md) for the full strategic narrative
and [docs/adr/](docs/adr/README.md) for the individual decisions behind it.

Current core scope: `pulpod` daemon, `pulpo` CLI, embedded web UI. Do not expand scope
into desktop/mobile clients unless explicitly requested.

## Architecture

Rust workspace with three crates + a React web UI:

- `crates/pulpod/` — Daemon binary (`pulpod`). Axum HTTP server, tmux backend, SQLite store.
- `crates/pulpo-cli/` — CLI binary (`pulpo`). Thin client that talks to `pulpod`'s REST API.
- `crates/pulpo-common/` — Shared types (Session, NodeInfo, API request/response types).
- `crates/pulpo-e2e/` — Dev-only end-to-end scenario suite (never released); see "Testing" below.
- `web/` — React 19 + Vite + Tailwind CSS v4 + shadcn/ui. Static SPA embedded into the `pulpod` binary via `rust-embed` for distribution.

For the architecture/session-lifecycle/API deep dive, see
[docs/architecture/overview.md](docs/architecture/overview.md),
[docs/architecture/core-concepts.md](docs/architecture/core-concepts.md),
[docs/architecture/harness-adapters.md](docs/architecture/harness-adapters.md),
[docs/operations/session-lifecycle.md](docs/operations/session-lifecycle.md), and
[docs/reference/api.md](docs/reference/api.md) — this file covers conventions and
workflow, not the full spec.

## What to Build Next

See [ROADMAP.md](ROADMAP.md) "Shipped" for what's already live and "Parked" for what's
deliberately not planned. In priority order for live work:

1. **Landing page + demo video** — the one distribution item still open; the
   README/docs reposition around the meter/breaker vision already shipped.
2. Nothing else is currently prioritized above what ROADMAP.md "Parked" lists — check
   there before proposing new scope, and check [docs/adr/](docs/adr/README.md) for
   whether a related decision has already been made (e.g. the five-state session model
   in ADR [0009](docs/adr/0009-five-state-session-model.md) is accepted but its
   implementation is still open work).

**Do NOT build:** mDNS/seed discovery, Tailscale peer discovery or a peer registry
(`bind = "tailscale"` stays for transport only), an MCP server, a Kubernetes backend,
team/multi-user features, or a cross-node control plane (see ADR
[0002](docs/adr/0002-meter-and-breaker-box-positioning.md)) — first parties won that
race. Do not re-add a cost-rate ceiling or forecasting math on top of the flat budget
cap (see ADR [0006](docs/adr/0006-exact-metering-and-flat-budget-cap-only.md)). Do not
propose a Go rewrite without reading ADR
[0007](docs/adr/0007-rust-stays-go-decision-deferred.md) first. The ocean gamification
UI was extracted to a separate `pulpo-ocean` repo (with history) — do not re-add
canvas/game code to this repo.

## Recording Decisions

Record a significant architectural or product decision (a removal, a positioning
shift, a test-strategy or config-contract change) as an ADR in `docs/adr/`, using the
MADR-style template in [docs/adr/0000-template.md](docs/adr/0000-template.md): Title,
Status, Context, Decision, Consequences, Date. Add it to the index in
[docs/adr/README.md](docs/adr/README.md). Not every change needs one — a bug fix or a
routine feature doesn't — but a decision that would otherwise only live in a commit
message or a PR description, and that a future contributor would need to un-archaeology
from `git log`, should get one instead.

## Quick Reference

Development: `make setup` | `make fmt` | `make lint` | `make test` | `make e2e` |
`make coverage-rust` | `make ci` | `make docs`

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

## Security Rules

These are mandatory for all code changes:

- **Session names are validated** via `validate_session_name()` in `session/utils.rs`
  — kebab-case only (`[a-z0-9-]`), max 128 chars. Any new code path that creates
  sessions MUST go through this validation. Session names are interpolated into shell
  commands (`wrap_command`); invalid names enable shell injection.
- **Schedule names** are validated with the same rules in `api/schedules.rs`. Any new
  code path that accepts session/schedule names MUST validate them.
- **Auth tokens** are compared using constant-time comparison (`constant_time_eq` in
  `auth.rs`). Do not use `==` for token comparison.
- **ConnectInfo** absence is treated as remote (fail-closed). Do not change this to
  fail-open.
- **CORS** is restricted in Public bind mode. Do not set `allow_origin(Any)` for
  Public mode.
- **No `unsafe` code.** `forbid(unsafe_code)` is set workspace-wide.
- **No `.unwrap()` in production code.** Use `?` or handle the error. `.unwrap()` is
  fine in tests.

## Code Standards

### Formatting

- **Rust**: `cargo fmt` (config in `rustfmt.toml` — 100 char width, edition 2024)
- **Web**: `prettier` (config in `web/.prettierrc` — single quotes, trailing commas, 100 char width)
- Run `make fmt` to format everything. Run `make fmt-check` to verify without modifying.

### Linting

- **Rust**: `clippy` with `deny(clippy::all)` and `forbid(unsafe_code)`, configured in workspace `Cargo.toml` under `[workspace.lints]`. `pedantic`/`nursery` were dropped in September 2026 (style churn, no bugs caught); do not re-add them or add per-file `#[allow]`s for lints that are no longer enabled.
- **Build cache for agent worktrees**: do NOT share one `CARGO_TARGET_DIR` between checkouts that build concurrently — cargo's fingerprints thrash and the September 2026 agent fleet saw corrupted test binaries mid-run. Use one cache dir per branch/worktree so it survives worktree removal without being shared: `export CARGO_TARGET_DIR=$HOME/.cache/pulpo-target/$(git branch --show-current)`. Prune old ones with `cargo sweep` or `rm -rf`. The Makefile and CI do not depend on this.
- **Web**: `eslint` with TypeScript and React plugins (config in `web/eslint.config.js`), plus `tsc --noEmit` for type checking.
- Run `make lint` to lint everything.

### Testing — unit tests for logic, scenario tests for behavior

2,300+ unit tests at 98% coverage still missed four real user-facing bugs (idle
threshold never read, hooks only reaching the default port, interventions deleting
a *shared* worktree, a Ready session bouncing back on resume) — every one of them a
gap *between* units (config → watchdog, harness → session manager, intervention →
worktree cleanup), the kind a `MockBackend`-based "flow" test asserts past because
the mock never disagrees with the code driving it. The fix isn't more unit tests;
it's a second, different kind of test for a different job. See ADR
[0003](docs/adr/0003-scenario-tests-as-behavior-gate.md) for the full rationale,
including why a proposal to lower the coverage bar was rejected.

- **Unit tests** — pure logic: parsing, cron math, rate tables, command-string
  rewriting, state-transition functions, request validation. TDD still applies
  here: write the test first, watch it fail, implement, refactor, keep it green.
  Fast, deterministic, `#[cfg(test)] mod tests` alongside the code as always.
- **Scenario tests** — every *documented user-facing behavior* (session lifecycle
  transitions, harness adapters, the watchdog, schedules, worktrees), proven
  against a **real `pulpod` daemon and a real (private) tmux server**, driven
  through the real `pulpo` CLI and HTTP API, with only the *agent* faked (a small
  binary imitating Claude Code's CLI/hook surface — see below). No mock backend,
  no mock store, no mock harness adapter: the daemon, tmux, and the CLI are all the
  real thing, so a scenario test can't pass just because a mock agreed with the
  code under test.
- **Mock-backend flow tests for new work are not accepted.** A PR adding or
  changing user-facing session/watchdog/harness/schedule behavior needs a scenario
  test (new or extended), not a `MockBackend`-driven integration test asserting the
  same call sequence the code just made. Existing `MockBackend` unit tests for
  pure branch coverage (a handler's error paths, a store query's edge cases) are
  still fine — this rule is about *flow* tests standing in for the real stack.
- Coverage stays as a **decay guard**, not the primary correctness signal: the 98%
  floor (`make coverage-rust`) still catches an untested branch, but it never
  catches a wrong *interaction* between two correctly-tested units — that's what
  the scenario suite is for.

**The scenario suite** (`crates/pulpo-e2e/`): a dev-only workspace member, never
part of a release build. `src/bin/fake-claude.rs`/`fake-codex.rs`/`fake-pi.rs` are
small binaries that imitate each harness's CLI/hook surface exactly as far as
pulpo's adapter uses it, scripted entirely by the `FAKE_AGENT_SCENARIO` env var (or
a `pulpo-fake-scenario.txt` file in the session's workdir, read fresh by every
process so a test can change behavior across a resume) — the shared step
vocabulary: `start`, `prompt`, `needs_input`, `wait` (blocks on stdin — `pulpo
input` unblocks it), `stop`, `exit`/`exit:<code>`, `hang`; `fake-claude` also has
`spend:<usd>` (writes a real Claude-shaped transcript file so the budget breaker has
real data to read). Each fake reads its own generated hook config exactly the way
its real harness would and fires the same hook commands the daemon's adapter wired
up, piped the same JSON shape:
- `fake-claude` (`--session-id`, `--settings`, `-p`, `--resume`, `--model`; unknown
  flags ignored): reads its `--settings` JSON and fires `pulpo hook claude`.
- `fake-codex` (`--dangerously-bypass-hook-trust`, `resume <id>`/`resume --last`,
  `exec`, `-m`/`--model`, a positional prompt): honors `CODEX_HOME`, reads its
  `config.toml`'s `notify`/`[[hooks.<Event>]]` entries, verifies `auth.json` and the
  adapter's symlinked real-home entries actually resolve (written to
  `pulpo-fake-codex-env.json`), fires `pulpo hook codex --event <Name>` on stdin and
  the `notify` program with its JSON as the *last argv element* (not stdin, matching
  Codex's real delivery), and writes a real rollout file under
  `$CODEX_HOME/sessions/YYYY/MM/DD/` so `pulpo usage` has real Codex data to read.
- `fake-pi` (`--session-id`, `-e`, `-p`, `-c`, `--model`): can't run the generated
  TypeScript extension, so it reads the file to confirm it exists and parses the
  `PULPO_BIN` path out of it, then emits the same events the extension would by
  spawning `pulpo hook pi --event <name>` itself; `--session-id` is idempotent
  create-or-open (reopens the session file already on disk under
  `$HOME/.pi/agent/sessions/<cwd-mangled>/` for that id, or creates one), and it
  exits 1 when `--session-id` is combined with `-c`/`-r`/`--session`/`--fork`,
  matching real pi.

`src/lib.rs` is the harness: boots an isolated `pulpod` (its own `HOME`, data dir,
config, free port, private tmux server via `TMUX_TMPDIR`), stages every fake binary
under a temp `bin/` dir named exactly like the real CLI it imitates (`claude`/
`codex`/`pi` — `HarnessRegistry` matches by argv0 basename), and exposes
`spawn`/`wait_status`/`session`/`stop`/`resume`/`input`/`restart_daemon`/
`kill_tmux_server`/`cleanup`/`fake_bin(harness)` (plus `fake_claude_bin`/
`fake_codex_bin`/`fake_pi_bin` shorthands).

**Scenarios** (`crates/pulpo-e2e/tests/scenarios.rs`, 20 `#[test]` functions across
17 numbered scenarios — S7, S13, and S16 each cover two; numbering skips 17-18,
reserved for the five-state session model work): S1 spawn
reaches Active with harness metadata; S2 a permission prompt sets `needs_input` and
`pulpo input` resolves it; S3 a clean exit resolves through Ready then Stopped and
`pulpo resume` continues the same harness conversation (`--resume <id>`); S4 the
tmux server dying marks the session Lost and resume reactivates it; S5 a daemon
restart preserves a live session and auto-resumes one whose tmux died while the
daemon was down, named after the session (not a stale `$N` id); S6 the budget
breaker stops an over-budget session and delivers a webhook; S7 the idle-timeout
kill intervention fires, and `--idle-threshold 0` disables the time-based
transition for a generic command; S8 a due schedule fires a session; S9 two
worktrees on one repo stay distinct, survive a plain stop, and are removed by
`pulpo cleanup`; S10 hooks reach a non-default port (`PULPO_URL`); S11 a generic
(harness-less) command uses the scrollback/exit-marker path and ends Stopped; S12 a
spawned command's quoted, multi-word `-p` prompt (and a later `--model` flag) reaches
the harness as intact arguments — `fake-claude` records its own `argv` to
`pulpo-fake-argv.json`, proving `shell_words::join`/`split` round-trip through the CLI,
the harness adapter's rewrite, and `wrap_command`'s own shell-escaping (regression
test for the v0.3.0 `command.join(" ")` quoting bug); the same fix on the
`schedule add` path is covered by a `pulpo-cli` unit test asserting the stored
`command` string round-trips, rather than a second slow (cron-minute-boundary) e2e
wait — S8 already proves a schedule's fired session goes through the identical spawn
path S12 exercises. S13 covers two database-recovery cases (a garbage/corrupt
`state.db`, and a downgraded database carrying a migration version ahead of the
binary's own embedded migrator) where the daemon used to crash-loop with only an
unhelpful "pulpod did not start in time" from the CLI; it now quarantines the
unusable file as `state.db.unusable-<UTC timestamp>` and starts fresh, proven by
`pulpo ls --all` succeeding afterward (see
[Release and Distribution](docs/operations/release-and-distribution.md) "Upgrading
pulpod"). S14 drives `fake-codex` through spawn (Active, `harness ==
"codex"`), the first hook learning `harness_session_id`, a permission prompt
(`needs_input`), a clean exit, and `pulpo resume` reaching the fake as `codex
resume <id> ...` inside the *same* isolated `CODEX_HOME` — and that `pulpo usage
--scan` counts the fake's own rollout file. S15 is the pi equivalent, additionally
proving `--session-id` idempotency: the resume command has the identical shape as
the original spawn (`pi --session-id <id> -e <path> ...`), and the fake reopens the
same on-disk session file rather than minting a new one. S16 covers the resume
*fallback* (see below) end to end for Codex, in two cases: the first, a scenario
that never fires `SessionStart` (hooks "disabled") leaves `harness_session_id`
unknown even after a clean exit, and `pulpo resume` still reaches the fake as
`codex ... --dangerously-bypass-hook-trust resume --last ...` — the harness's own
"most recent conversation here" flag — rather than silently starting a fresh
thread; the second additionally removes the session's worktree before resuming
and proves the fallback still succeeds (not refused) — Codex's `resume --last` is
keyed by an isolated `CODEX_HOME`, not by the directory it runs from, unlike
Claude/pi's `--continue` (see "Resume fallback" below, and the
`session::manager` unit tests covering the refusal for those two). S19 starts a
second `pulpod` against the same data directory while the first is still
running and asserts it exits non-zero without touching the first's `state.db` —
the single-instance lock `store::lock` exists specifically because the database
is opened well before the port is bound (see [Release and
Distribution](docs/operations/release-and-distribution.md) "Single-instance
lock").

**Running it**: `make e2e` (builds `pulpod`/`pulpo`/every `fake-*` binary first,
then runs the suite serially — `cargo test -p pulpo-e2e -- --test-threads=1`; each
test boots its own daemon and tmux server, and running several of those
concurrently on a laptop is exactly the kind of flakiness this strategy moved away
from). Needs `tmux`. Not part of `make ci`/the pre-commit hook — the full suite
takes a bit over a minute (S8 has to wait for a real cron minute boundary) — it has
its own CI job (`e2e`, independent of `coverage`) and is meant to be run manually
before a PR that touches session/watchdog/harness/schedule behavior.

**Adding a scenario**: pick (or add) a step in the shared `FAKE_AGENT_SCENARIO`
vocabulary (`fake-claude.rs`/`fake-codex.rs`/`fake-pi.rs`) if the existing steps
don't cover the behavior you're proving, add a `#[test]` in `scenarios.rs` using the
`Daemon` harness in `src/lib.rs`, and assert on real daemon state
(`daemon.session(name)`/`wait_status`/`wait_for`) — never on an internal function
call sequence.

**Fake harnesses are unverified against the real CLIs.** `fake-claude` imitates
behavior verified against a real Claude Code v2.1.266 binary (see
`docs/architecture/harness-adapters.md`); `fake-codex`/`fake-pi` imitate the *Codex*
and *pi* adapters' own documented/researched behavior, but neither fake has been
run against a real `codex`/`pi` installation — they prove the daemon side (spawn
rewrite → hook ingestion → state transition → resume) is internally consistent and
matches the adapters' own contract, not that a real Codex/pi binary actually speaks
that contract. Treat S14-S16 passing as "the adapter does what its own
documentation says," not as field verification.

- **Rust**: `cargo test --workspace --exclude pulpo-e2e` for unit tests (pulpo-e2e
  needs pre-built binaries and a tmux server — see `make e2e` above). Tests live
  alongside source code in `#[cfg(test)] mod tests` blocks.
- **Real-tmux unit tests are serialized**: a handful of unit tests still talk to
  tmux directly (`backend::tmux`, `session::manager`'s `real_tmux_tests`) instead
  of going through `MockBackend`. `crate::test_serial::lock()`
  (`crates/pulpod/src/test_serial.rs`) is a process-wide `Mutex<()>` these tests
  acquire and hold for their whole duration, so they never run concurrently
  against each other under a parallel `cargo test` (session-name collisions and
  tmux-server-startup races were a real source of flakiness before this existed).
  It has no effect on the other thousands of `MockBackend`-based tests.
- **Web**: `vitest` with jsdom environment. Test files use `*.test.ts` or `*.spec.ts` naming.
- Run `make test` to run all unit tests (Rust + web); `make e2e` for the scenario suite.

### Coverage

- Rust coverage is enforced by the executable gate `make coverage-rust`.
- The full local quality gate is `make ci`.
- Uses `cargo-llvm-cov`. Run `make coverage` for Rust + web coverage, or `make coverage-html` for an HTML report.
- Run `make coverage-html` for an HTML report at `target/llvm-cov/html/index.html`.
- Every new function, branch, and error path must have a test. No exceptions.
- `main.rs` files are excluded from coverage — they are thin `#[cfg(not(coverage))]` wrappers. All logic lives in `lib.rs`.
- `embed.rs` is excluded from coverage — it contains only the `#[derive(Embed)]` macro for `rust-embed`, which generates uncoverable code.
- `crates/pulpo-e2e` (the scenario suite) is excluded from the coverage run entirely
  (`--exclude pulpo-e2e`) — it has no unit tests of its own, and instrumenting it
  would need `pulpod`/`pulpo`/every `fake-*` binary pre-built and a real tmux server.
  See "Testing" above for how it's run and gated instead.

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

The pre-commit hook does **not** run `make e2e` — the scenario suite takes a bit over
a minute per full run (S8 waits for a real cron minute boundary) and boots a real
tmux server per test, which is too slow for every commit. Run it manually before a
PR that touches session/watchdog/harness/schedule behavior; CI runs it in its own
`e2e` job, independent of `coverage`.

## Reliability Priorities

1. Correct failure detection (watchdog, session state transitions).
2. Safe intervention semantics (no false `dead` on failed stop).
3. Clear auditability (intervention reasons/events).
4. Bounded, explicit recovery behavior.

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
- **Harness adapters**: `session/manager.rs` resolves a `HarnessAdapter` (`harness/`) at spawn/resume time to rewrite the command so the harness (Claude Code, Codex, pi) reports lifecycle events to `pulpo hook <harness>` → `POST /api/v1/sessions/{id}/harness-events`. Shipped for Claude Code (hook mechanics verified against v2.1.266), Codex and pi (implemented from their docs, exercised against fake harnesses in the e2e suite — see "Testing" above — but still unverified against a real Codex/pi process). Once a session's `harness_last_event_at` is set, the watchdog stops applying scrollback-heuristic *detection* to it — but per-signal (`watchdog::owned_signals`/`HarnessAdapter::owned_signals`), not all-or-nothing: an adapter missing a signal (Codex has no error/rate-limit hook) keeps that one heuristic running from scrollback even while its lifecycle events flow. `idle_timeout` and the budget-cap fields always apply regardless of harness ownership. **Resume fallback**: when a session has an adapter but no known `harness_session_id` (a legacy row, or a hook/rollout that never reported one), `resolve_resume_command` uses `HarnessAdapter::fallback_resume_command` — the harness's own "most recent conversation here" flag (`claude --continue`, `codex resume --last`, `pi -c`) — instead of silently replaying the original command as a fresh conversation. When that fallback is cwd-scoped (`HarnessAdapter::fallback_resume_is_cwd_scoped`, default `true` — true for Claude/pi, false for Codex since `resume --last` is keyed by an isolated `CODEX_HOME` instead) and the session's worktree is gone (`effective_resume_workdir` fell back to the plain `workdir`), `resolve_resume_command` refuses the resume outright rather than risk silently continuing an unrelated conversation in the wrong directory. See ADR [0001](docs/adr/0001-hook-driven-agent-state.md) and `docs/architecture/harness-adapters.md`.
- **Session IDs**: `backend_session_id` stores the tmux `$N` session ID (monotonically increasing, never reused while tmux server runs). At startup, name-based IDs are upgraded to `$N` IDs.
- **Database**: SQLite via `sqlx`. Versioned schema migrations live in `crates/pulpod/migrations/`; `store/mod.rs` contains the runtime store API only. Use `sqlx::query!` macro for compile-time checked queries when possible. `store::open_and_migrate` quarantines (renames away) `state.db` only when the failure means the file itself is corrupt or on an incompatible schema (`store::core::is_quarantine_worthy`) — anything else (a lock, an I/O error, a failed pre-migration backup) refuses to start instead of risking a healthy database. `store::lock` acquires an exclusive advisory `flock` on `{data_dir}/pulpod.lock` before the database is even opened, so a second `pulpod` against the same data dir refuses to start rather than race the first. See [Release and Distribution](docs/operations/release-and-distribution.md).
- **Config**: TOML config at `~/.pulpo/config.toml`, and is the **sole source of truth** — there is no config-editing API (see ADR [0005](docs/adr/0005-config-file-is-source-of-truth.md)). All fields have sensible defaults — pulpod runs with zero config. An unrecognized config key logs a startup warning and is otherwise ignored (see ADR [0008](docs/adr/0008-unknown-config-keys-warn-and-are-ignored.md)) rather than failing config load. Key watchdog config fields: `idle_threshold_secs` (seconds of unchanged output before Active→Idle, default 60), `waiting_patterns` (extra user-defined patterns appended to the built-in waiting-for-input patterns).
- **Per-session idle**: Sessions accept `idle_threshold_secs: Option<u32>` — `None` = use global, `Some(0)` = never idle, `Some(N)` = N seconds. CLI: `pulpo spawn <name> --idle-threshold <secs>`.
- **Logging**: Use `tracing` macros (`info!`, `warn!`, `error!`, `debug!`). Set level via `RUST_LOG` env var.
- **No `unsafe` code** — `forbid(unsafe_code)` is set workspace-wide.
- **No `.unwrap()`** in production code — use `?` or handle the error. `.unwrap()` is fine in tests.

## Guardrails

- Keep changes minimal and scoped.
- Avoid broad refactors unless they directly unblock the task.
- Update docs when behavior changes; record a significant decision as an ADR (see
  "Recording Decisions" above).
- Prefer small, test-backed increments over speculative platform work.
- Maintain backward compatibility for existing configs and databases.
- Do not add features that agents (Claude Code, Codex) now handle natively.

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
| `make test` | Run all unit tests (Rust + web) |
| `make e2e` | Run the end-to-end scenario suite (real daemon + tmux + fake harness) |
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
| `make docs` | Build the mdBook docs site (`docs/book/`) |
| `make docs-serve` | Serve the mdBook docs site locally with live reload |

## Project Layout

```
pulpo/
├── CLAUDE.md                     # One-line @AGENTS.md import (Claude Code only)
├── AGENTS.md                     # This file — the single development guide for any agent
├── ROADMAP.md                    # Mission, the bet, shipped/parked/removed, links to ADRs
├── CONTRIBUTING.md               # Short human-facing contributor workflow
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
├── docs/                         # mdBook docs site (getting-started/guides/reference/architecture/operations/adr)
│   ├── book.toml                 # mdBook config (src = ".", build-dir = "book")
│   ├── SUMMARY.md                # mdBook table of contents
│   └── adr/                      # Architecture Decision Records (MADR-style; see docs/adr/README.md)
├── examples/                     # Runnable CLI/API/config examples
├── scripts/                      # demo.sh, install-pulpo.sh
├── .pulpo/config.toml.example    # `make dev` local-dev config template
├── crates/
│   ├── pulpod/src/
│   │   ├── main.rs               # Thin entry point (cfg(coverage) excluded)
│   │   ├── lib.rs                # Daemon logic: Cli, init_tracing, build_app
│   │   ├── config.rs             # TOML config loading
│   │   ├── platform.rs           # OS detection (macOS/Linux/WSL2)
│   │   ├── auth_info.rs          # Agent-name detection for command -> usage-reader dispatch
│   │   ├── coverage_macros.rs    # coverage_warn!/coverage_info! (keep rare log-only branches out of coverage)
│   │   ├── test_serial.rs        # Process-wide mutex serializing the real-tmux unit tests
│   │   ├── api/                  # Axum REST API
│   │   │   ├── mod.rs            # AppState, router setup
│   │   │   ├── routes.rs         # Route definitions + auth middleware
│   │   │   ├── auth.rs           # Bearer-token middleware + GET /auth/token endpoint
│   │   │   ├── config.rs         # Read-only effective-config endpoint (GET only)
│   │   │   ├── health.rs         # Health check endpoint
│   │   │   ├── sessions.rs       # Session CRUD + input/stop/resume/handoff/harness-events handlers
│   │   │   ├── sessions_tests.rs # Session handler tests (split out of sessions.rs)
│   │   │   ├── test_support.rs   # Shared API test fixtures/helpers
│   │   │   ├── node.rs           # Node info endpoint
│   │   │   ├── schedules.rs      # Schedule CRUD + run-history handlers
│   │   │   ├── notifications.rs  # Read-only notification config endpoint (GET only)
│   │   │   ├── usage.rs          # Exact per-session usage + scan endpoints
│   │   │   ├── watchdog.rs       # Read-only watchdog config endpoint (GET only)
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
│   │   │   └── tests.rs          # Integration tests exercising the store API end-to-end
│   │   ├── notifications/        # Webhook notifications
│   │   │   ├── mod.rs            # Module declaration + dispatcher
│   │   │   └── webhook.rs        # Plain webhook delivery (in-memory queue, fixed retry schedule)
│   │   ├── watchdog/             # Resource monitoring
│   │   │   ├── mod.rs            # Watchdog loop (idle detection + breakers)
│   │   │   ├── idle.rs           # Idle detection + status transitions
│   │   │   ├── metadata.rs       # PR/branch/rate-limit/error/usage scraping from output
│   │   │   ├── output_patterns.rs # Waiting-for-input/rate-limit/error/PR-URL pattern matching
│   │   │   ├── git.rs            # Branch/commit detection for sessions
│   │   │   ├── budget.rs         # Per-session cost budget alerts + auto-stop
│   │   │   ├── intervention.rs   # Shared stop-and-record path for forced session stops
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
│   │       ├── rollup.rs         # Per-session exact usage + per-repo/worktree rollups
│   │       └── scan.rs           # Read-only scan of all local agent history
│   ├── pulpo-cli/src/
│   │   ├── main.rs               # Thin entry point (cfg(coverage) excluded)
│   │   ├── lib.rs                # CLI logic: Cli, Commands, execute
│   │   ├── hook.rs               # `pulpo hook <harness>` internal subcommand (lifecycle events → daemon)
│   │   ├── format.rs             # Terminal output rendering (tables/reports)
│   │   └── http.rs               # HTTP client helpers (auth, base-URL/token resolution)
│   ├── pulpo-common/src/
│   │   ├── lib.rs
│   │   ├── session.rs            # Session, SessionStatus, InterventionCode types
│   │   ├── node.rs               # NodeInfo type
│   │   ├── event.rs              # SessionEvent for SSE + notifications
│   │   ├── auth.rs               # BindMode (local/tailscale/public)
│   │   └── api.rs                # API request/response types
│   └── pulpo-e2e/                # End-to-end scenario suite (dev-only, never released)
│       ├── src/
│       │   ├── bin/fake-claude.rs # Fake Claude Code CLI/hook surface (FAKE_AGENT_SCENARIO-driven)
│       │   ├── bin/fake-codex.rs # Fake Codex CLI/hook/notify surface (isolated CODEX_HOME, rollout files)
│       │   ├── bin/fake-pi.rs    # Fake pi CLI surface (--session-id idempotency, pulpo hook pi --event)
│       │   └── lib.rs            # Scenario harness: boots an isolated pulpod + private tmux server
│       └── tests/scenarios.rs    # S1-S13, S14-S16: spawn, needs-input, resume, restart, budget, idle, schedule, worktrees, db recovery, Codex/pi, resume fallbacks, ...
└── web/                          # React 19 + Vite + Tailwind v4 + shadcn/ui
    ├── src/
    │   ├── index.css             # Tailwind imports + dark theme CSS vars
    │   ├── main.tsx              # Entry point
    │   ├── App.tsx                # React Router setup
    │   ├── api/
    │   │   ├── types.ts          # Shared TypeScript interfaces
    │   │   ├── client.ts         # API fetch functions
    │   │   └── connection.ts     # testConnection
    │   ├── hooks/
    │   │   ├── use-connection.tsx      # Connection context (baseUrl, token, saved)
    │   │   ├── use-sse.tsx             # SSE event stream + session state
    │   │   ├── use-schedules-filter.ts # Schedule list filtering
    │   │   └── use-mobile.ts           # Mobile breakpoint detection
    │   ├── lib/
    │   │   ├── utils.ts          # cn() helper, formatDuration, formatSessionStatus
    │   │   ├── notifications.ts  # Desktop notification helpers
    │   │   └── cron.ts           # Cron expression parsing/formatting
    │   ├── components/
    │   │   ├── ui/               # shadcn generated components
    │   │   ├── layout/           # Sidebar, header, app shell, disconnected banner
    │   │   ├── dashboard/        # Status summary, node/session cards, new session dialog
    │   │   ├── session/          # Output view, terminal view (ghostty-web)
    │   │   ├── schedules/        # Schedule dialog, run-history panel, schedule row
    │   │   ├── history/          # Session filter (reused by dashboard)
    │   │   └── connect/          # Connect form, saved connections
    │   └── pages/
    │       ├── dashboard.tsx     # Sessions list (the landing page)
    │       ├── session-detail.tsx # Single-session detail view
    │       ├── schedules.tsx     # Schedule management
    │       ├── usage.tsx         # Usage/cost gauge
    │       ├── settings.tsx      # Read-only effective-config view (edit config.toml + restart to change)
    │       └── connect.tsx       # Connection screen (standalone)
    ├── eslint.config.js
    ├── .prettierrc
    ├── vite.config.ts            # Vite config + Tailwind plugin + API proxy
    └── vitest.config.ts          # Vitest config
```
