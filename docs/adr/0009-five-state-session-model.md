# 0009. Five-state session model: starting / working / waiting / done / lost

- **Status:** Accepted — Implemented (#129)
- **Date:** 2026-09-14

## Context

The current `SessionStatus` enum has six states — `creating`, `active`, `idle`,
`ready`, `stopped`, `lost` (see `docs/operations/session-lifecycle.md`) — split across
two axes that don't map cleanly onto how an operator actually reasons about a session
day to day: is it running, is it doing something or stuck, did it finish, is it gone.
`ready` vs. `stopped` in particular is a recurring source of confusion: both mean "not
running, and resumable," and the distinction is more about *why* the session stopped
running than about what an operator should do next (both answers are "run `pulpo
resume`").

## Decision

We will adopt a **five-state session model** — `starting`, `working`, `waiting`,
`done`, `lost` — as the target for the session lifecycle going forward. The intended
mapping from today's six states:

| Old state | New state | Notes |
|-----------|-----------|-------|
| `creating` | `starting` | Unchanged meaning |
| `active` | `working` | Unchanged meaning |
| `idle` | `waiting` | The existing `needs_input` metadata reason (`permission`, `question`, `idle`, ...) continues to qualify *why* a session is waiting |
| `ready` | `done` | Merges with `stopped` below |
| `stopped` | `done` | Both `ready` and `stopped` mean "not running, and resumable" — the *how it got there* becomes metadata under `done`, not a separate top-level state |
| `lost` | `lost` | Unchanged |

**Implemented.** The enum rename, the `sessions.status` DB migration, and every
consumer of `SessionStatus` (CLI output formatting, web UI badges, the
`lifecycle.<subtype>` webhook event naming, the API reference, and every doc that used
to enumerate six states) have landed together — see "Implementation notes" under
Consequences below for the specifics.

## Consequences

- The mental model for both users and docs is simpler now: five states instead of six,
  with a cleaner one-to-one match to "what is this session doing right now."
- **Implementation notes**: the migration is
  `crates/pulpod/migrations/0010_five_state_status.sql`. `status_reason` is a new plain
  TEXT column/field (not a separate top-level enum) added to `sessions`/`Session`/the SSE
  `SessionEvent`, carrying the "how"/"why" that used to be either a distinguishable
  top-level status (`ready` vs. `stopped`) or an ad hoc metadata key (`needs_input`). Old
  JSON/DB text (`creating`, `active`, `idle`, `ready`, `stopped`, `killed`) keeps
  deserializing via `#[serde(alias = ...)]` on `SessionStatus` and the matching
  [`FromStr`] arms, so external tooling reading old data before it's migrated (or a stale
  client) isn't broken by the rename.
- The migration touched a wide surface: the `status` column and its historical values,
  every CLI/API/web consumer, and the webhook event taxonomy
  (`lifecycle.{starting,working,waiting,done,lost}` now, replacing
  `lifecycle.{creating,active,idle,ready,stopped,error,rate_limited,lost}`) all needed a
  compatibility story for existing integrations reading the old state names — the
  `status_reason`/alias approach above is that story.
- Collapsing `ready`/`stopped` into `done` means the two are no longer top-level-state
  distinguishable without inspecting `status_reason` — any tooling that used to branch on
  `ready` vs. `stopped` specifically now needs to read `status_reason` instead.
- **The idle-timeout breaker exempts `needs_input`.** A session parked on
  `waiting:needs_input:<reason>` is blocked on a real decision from the operator (a
  permission/approval prompt), not "idle" in the sense `idle_timeout_secs` means — it
  might sit there for five minutes or five hours waiting for a person, and force-stopping
  it would destroy work that was correctly paused, not stuck. `idle_timeout_secs`'s
  alert/kill action still applies to plain `waiting:idle` (sustained silence, no pending
  prompt) and to a harness-owned `working` session producing no output at all. See
  `watchdog::idle::check_session_idle`.
- **Removing the `Ready` sweep required the watchdog to take over dead-backend
  resolution itself**, not just the lazy `get_session`/`list_sessions`/`resume_lost_sessions`
  paths: `wrap_command` closing the pane the instant the agent exits means a session's
  backend can die between watchdog ticks with nobody polling the API to notice. The
  watchdog's own idle-check tick now calls `is_alive()` on every `working`/`waiting`
  session before any output capture, resolving a dead one (and firing its `lifecycle`
  event/webhook) within that same tick via the same shared
  `session::manager::resolve_dead_backend_session` the lazy paths use — see
  [Session Lifecycle](../operations/session-lifecycle.md) "Working/Waiting → Done"/"→
  Lost". The same function also best-effort-preserves a session's final output (a live
  tmux capture attempt, then — since that almost never catches a cleanly-closed pane —
  the per-session pipe-pane log), which is why `capture_session_output` defaults to
  `true` as of this model (see `docs/reference/config.md`).
