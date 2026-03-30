# Pulpo Agent Instructions

Instructions for coding agents (Codex, Claude, and compatible tools).

## Product Focus

Pulpo is a **self-hosted control plane for background coding agents** — agent-agnostic infrastructure you own. It runs any CLI agent (Claude Code, Codex, Aider, Goose, etc.) on your machines with durable session lifecycle, watchdog supervision, scheduling, and multi-node fleet management.

**Positioning:** infrastructure layer, not an agent. Sovereign by architecture — code never leaves your infrastructure. Key differentiators: scheduling, cost control (coming), self-hosted fleet management, EU sovereignty compliance.

Current core scope: `pulpod` daemon, `pulpo` CLI, embedded web UI.
Do not expand scope into desktop/mobile clients unless explicitly requested.

## What to Build Next

See `ROADMAP.md` "What's Next" section. In priority order:

1. **Cost tracking (P5.1)** — token parsing exists, need cost rates, per-session budgets, auto-stop
2. **Agent completion callbacks** — `PULPO_CALLBACK_URL` env var for reliable idle detection
3. **Landing page + demo video**

**Do NOT build:** mDNS/seed discovery (removed), ocean gamification features, MCP expansion, Kubernetes backend, team/multi-user features.

## Quick Reference

- `CLAUDE.md` — detailed conventions, code examples, file paths, coverage rules
- `SPEC.md` — architecture, session lifecycle, API design
- `ROADMAP.md` — competitive landscape, sovereignty positioning, priority list

Development: `make setup` | `make fmt` | `make lint` | `make test` | `make coverage-rust` | `make ci`

## Security Rules

These are mandatory for all code changes:

- **Session names are validated** via `validate_session_name()` in `session/manager.rs` — kebab-case only (`[a-z0-9-]`). Any new code path that creates sessions MUST go through this validation. Session names are interpolated into shell commands; invalid names enable shell injection.
- **Schedule names** are validated with the same rules in `api/schedules.rs`.
- **Secrets temp files** are written to `~/.pulpo/data/secrets/` with atomic `0o600` permissions (Unix). Never write secrets to `/tmp`.
- **Auth tokens** are compared using constant-time comparison (`constant_time_eq` in `auth.rs`). Do not use `==` for token comparison.
- **ConnectInfo** absence is treated as remote (fail-closed). Do not change this to fail-open.
- **CORS** is restricted in Public bind mode. Do not set `allow_origin(Any)` for Public mode.
- **No `unsafe` code.** `forbid(unsafe_code)` is set workspace-wide.
- **No `.unwrap()` in production code.** Use `?` or handle the error.

## Engineering Standards

- **TDD**: write failing test, implement, refactor, verify coverage.
- **Coverage**: 98% line coverage enforced locally and in CI. Use `cfg(coverage)` only for genuinely untestable I/O.
- **CI**: `--test-threads=1` for coverage runs (prevents sqlx-sqlite prepared statement cache races).
- Rust logic in `lib.rs`; `main.rs` is a thin wrapper.
- Use `tracing` for operational events.
- If a pre-commit/CI check fails, fix it — do not bypass hooks.

## Reliability Priorities

1. Correct failure detection (watchdog, session state transitions).
2. Safe intervention semantics (no false `dead` on failed stop).
3. Clear auditability (intervention reasons/events).
4. Bounded, explicit recovery behavior.

## Guardrails

- Keep changes minimal and scoped.
- Avoid broad refactors unless they directly unblock the task.
- Update docs when behavior changes.
- Prefer small, test-backed increments over speculative platform work.
- Maintain backward compatibility for existing configs and databases.
- Do not add features that agents (Claude Code, Codex) now handle natively.
