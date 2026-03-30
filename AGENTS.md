# Pulpo Agent Instructions

Instructions for coding agents (Codex, Claude, and compatible tools).

## Product Focus

Pulpo is a self-hosted agent operations stack:

- Run coding agents on your own machines.
- Recover from failures (watchdog + session persistence).
- Control sessions via command-agnostic configuration (any shell command).

Current core scope: `pulpod` daemon, `pulpo` CLI, embedded web UI.
Do not expand scope into desktop/mobile clients unless explicitly requested.

## Quick Reference

See `CLAUDE.md` for detailed conventions, code examples, and file paths.
See `SPEC.md` for architecture, session lifecycle, and API design.
See `ROADMAP.md` for project sequencing and next steps.

Development commands: `make setup` | `make fmt` | `make lint` | `make test` | `make coverage-rust` | `make coverage` | `make ci`

## Engineering Standards

- **TDD**: Write failing test, implement minimal fix, refactor, re-run coverage gates.
- **Quality gates are executable**: changes must pass the repo's enforced commands (`make ci` locally, CI on GitHub).
- Rust logic belongs in `lib.rs`; keep `main.rs` thin wrappers.
- No `unsafe` code. No `.unwrap()` in production code.
- Use `tracing` for operational events.
- If a pre-commit/CI check fails, fix it — do not bypass hooks.

## Reliability Priorities

When working on roadmap items, prioritize:

1. Correct failure detection behavior (watchdog and session state transitions).
2. Safe intervention semantics (no false `dead` state on failed stop).
3. Clear auditability (intervention reasons/events).
4. Bounded, explicit recovery behavior.

## Guardrails for Agent Edits

- Keep changes minimal and scoped to the requested goal.
- Avoid broad refactors unless they directly unblock the task.
- Update README/docs when behavior changes.
- Prefer small, test-backed increments over speculative platform work.
- Maintain backward compatibility for existing configs and databases.

## Source of Truth

- **Agent guardrails**: this file (`AGENTS.md`)
- **Detailed conventions and examples**: `CLAUDE.md`
- **Product and lifecycle design**: `SPEC.md`
- **Project sequencing**: `ROADMAP.md`
