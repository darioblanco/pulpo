# 0001. Hook-driven agent state instead of scrollback scraping

- **Status:** Accepted
- **Date:** 2026-09-10 (PR [#97](https://github.com/darioblanco/pulpo/pull/97))

## Context

Pulpo inferred a session's real-world state entirely by substring-matching the last
lines of `tmux` scrollback (`watchdog/output_patterns.rs`: waiting-for-input,
rate-limit, and error patterns, plus a time-based idle fallback). This is fragile — it
silently breaks whenever an agent TUI changes its wording — and it never learns the
agent's own conversation/session id, so resuming a lost or stopped session re-ran the
bare original command and the conversation was lost, not resumed.

Modern coding-agent harnesses (Claude Code, Codex, pi) expose structured lifecycle
signals of their own: hooks that fire on real events, and a `--session-id`/resume flag
that can replay the same conversation. Nothing in Pulpo's design used them yet.

## Decision

We will add a `HarnessAdapter` trait + registry (`crates/pulpod/src/harness/`) that
translates one agent CLI's own conventions into a normalized `HarnessEvent`
(`SessionStarted`, `Working`, `TurnFinished`, `NeedsInput`, `Failed`, `SessionEnded`).
At spawn/resume time, `session/manager.rs` resolves the adapter for the command and
rewrites it so the harness reports back to `pulpo hook <harness>` →
`POST /api/v1/sessions/{id}/harness-events`, which drives the same
`Active`/`Idle`/`Ready`/`Stopped` states directly instead of guessing from scrollback.

Shipped for Claude Code (hook mechanics verified against v2.1.266), Codex and pi
(implemented from their docs, unverified in the field at ship time). A command with no
matching adapter (`GenericAdapter`) behaves exactly as before — no rewrite, no events,
scrollback heuristics only. The watchdog stops applying scrollback heuristics **only
for the signals a harness's own events cover** (`HarnessAdapter::owned_signals()`) —
e.g. Codex has no error/rate-limit hook, so those two keep running from scrollback even
once its lifecycle events flow.

Two additive user-visible effects once a session's events start flowing:

1. `needs input (<reason>)` — a `NeedsInput` event sets `Idle` plus a `needs_input`
   metadata reason (`permission`, `question`, `idle`, ...), distinguishing "blocked on
   me" from a plain idle prompt.
2. Real resume — `pulpo resume` replays the harness's own resume command
   (`claude --resume <id>`, `codex resume <id>`, pi's idempotent `--session-id <id>`)
   instead of re-running the bare original command.

## Consequences

- Pulpo stays harness-agnostic by design: nothing outside `pulpod/src/harness/` may
  assume a specific agent, and an unrecognized command is unaffected.
- Each new harness adapter is its own maintenance surface (hook wiring, event parsing,
  resume rewrite) that can drift from the upstream agent's CLI across versions — Codex
  and pi's adapters shipped unverified in the field and needed follow-up fixes
  (see PR [#109](https://github.com/darioblanco/pulpo/pull/109)).
- A harness missing a given signal (Codex's error/rate-limit hooks) still depends on
  the old scrollback heuristics for exactly that signal — the two detection paths
  coexist per-signal, not all-or-nothing, which is more precise but a more complex
  mental model than "hooks replace scrollback entirely."
- This is the foundation the later test strategy change (ADR
  [0003](0003-scenario-tests-as-behavior-gate.md)) validates against: a `fake-claude`
  binary plays the harness side of this exact hook contract in the scenario suite.
