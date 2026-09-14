# 0009. Five-state session model: starting / working / waiting / done / lost

- **Status:** Accepted — implementation pending
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

**This ADR records the decision; implementation is pending.** The enum rename, the
`sessions.status` DB migration, and every consumer of `SessionStatus` (CLI output
formatting, web UI badges, the `lifecycle.<subtype>` webhook event naming, the API
reference, and every doc that currently enumerates six states) are a separate body of
work, to be scheduled and landed on their own, not as part of this documentation
consolidation batch.

## Consequences

- Once implemented, the mental model for both users and docs gets simpler: five states
  instead of six, with a cleaner one-to-one match to "what is this session doing right
  now."
- Until implemented, every current doc, API reference, and CLI output continues to
  describe the six-state model faithfully — that is the *correct*, accurate
  description of shipped behavior today. This ADR is forward-looking and must not be
  read as describing current behavior; docs should not be rewritten to the five-state
  model until the code is.
- The migration touches a wide surface once it starts: the `status` column and its
  historical values, every CLI/API/web consumer, and the webhook event taxonomy
  (`lifecycle.{creating,active,idle,ready,stopped,error,rate_limited,lost}` today) all
  need a compatibility story for existing integrations reading the old state names.
- Collapsing `ready`/`stopped` into `done` means the two are no longer top-level-state
  distinguishable without inspecting metadata — any tooling that currently branches on
  `ready` vs. `stopped` specifically will need to read the underlying reason instead
  once this ships.
