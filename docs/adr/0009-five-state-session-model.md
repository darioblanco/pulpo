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
