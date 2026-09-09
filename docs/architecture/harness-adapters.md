# Harness Adapters

## Problem

Pulpo used to infer agent state entirely by substring-matching the last lines of tmux
scrollback (`watchdog/output_patterns.rs`: waiting-for-input, rate-limit, and error
patterns, plus a time-based idle fallback). That's fragile — it silently breaks
whenever an agent TUI changes wording — and it never learns the agent's *own* session
id, so resuming a lost session re-ran the bare command and the conversation was lost.

Modern agent harnesses expose structured lifecycle signals instead (hooks, a
`--session-id`/resume flag, ...). A **harness adapter** translates one agent CLI's own
conventions into a normalized event pulpo understands, so the daemon can react to real
signals instead of guessing from text.

Pulpo stays harness-agnostic by design: nothing outside `pulpod/src/harness/` may
assume a specific agent. A command with no matching adapter behaves exactly as it did
before — no functional change to plain shells or unrecognized CLIs.

## The `HarnessAdapter` trait

`crates/pulpod/src/harness/mod.rs`:

```rust
pub trait HarnessAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn matches(&self, argv0: &str) -> bool;
    fn prepare_spawn(&self, ctx: &SpawnContext) -> Result<SpawnPlan>;
    fn resume_command(&self, original_command: &str, harness_session_id: &str) -> Option<String>;
    fn parse_event(&self, raw: &serde_json::Value) -> Result<Option<HarnessEvent>>;
    fn emits_events(&self) -> bool;
}
```

- **`matches`** — does this adapter own the command, by basename of `argv[0]`?
- **`prepare_spawn`** — rewrite the command line (and optionally inject extra env
  vars / write files under `<data_dir>/harness/<session_id>/`) so the harness reports
  events back to pulpo. Adapters never fail a spawn on their own account — a problem
  preparing the rewrite falls back to the unchanged command, logged as a warning.
- **`resume_command`** — given the original command and the harness's own session id,
  produce a command that resumes that exact conversation. `None` if the adapter has
  no resume support or no id is known yet.
- **`parse_event`** — translate one raw hook payload into a [`HarnessEvent`]:

  ```rust
  pub enum HarnessEvent {
      SessionStarted { harness_session_id: Option<String>, resumed: bool },
      Working,
      TurnFinished { summary: Option<String> },
      NeedsInput { reason: NeedsInputReason }, // Permission | Question | Idle | Other(String)
      Failed { error: String, rate_limited: bool },
      SessionEnded { reason: Option<String> },
  }
  ```

`HarnessRegistry` (`harness/registry.rs`) holds adapters in priority order and
resolves a command line via `shell_words::split` + basename of `argv[0]` — handling
`env FOO=bar claude`, absolute paths, etc. `GenericAdapter` always matches last: no
rewrite, no resume id, no events. Adding Codex/pi/Gemini support later means writing
one more adapter and registering it — the trait and registry don't need to change.

## Shipped: the Claude Code adapter

`harness/claude.rs` is the first concrete adapter. Verified against Claude Code
v2.1.266: `--session-id <uuid>`, `--settings <file-or-json>` (highest CLI precedence),
`-r`/`--resume [session-id]`, `-c`/`--continue`.

**On spawn** (no existing `--resume`/`-r`/`--continue`/`-c`/`--session-id`/`--settings`
flag): generates a UUID, writes `<data_dir>/harness/<session_id>/claude-settings.json`
wiring every hook of interest to `pulpo hook claude`, and rewrites the command to
`claude --session-id <uuid> --settings <path> <original args>`. The id is known
up front and stored immediately — resume still works even if the `SessionStart` hook
never fires (e.g. the agent crashes before Claude's hook runner starts).

```json
{
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "pulpo hook claude", "timeout": 5 }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "pulpo hook claude", "timeout": 5 }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "pulpo hook claude", "timeout": 5 }] }],
    "StopFailure": [{ "hooks": [{ "type": "command", "command": "pulpo hook claude", "timeout": 5 }] }],
    "SessionEnd": [{ "hooks": [{ "type": "command", "command": "pulpo hook claude", "timeout": 5 }] }],
    "Notification": [{
      "matcher": "permission_prompt|idle_prompt|agent_needs_input|elicitation_dialog|elicitation_url_dialog",
      "hooks": [{ "type": "command", "command": "pulpo hook claude", "timeout": 5 }]
    }]
  }
}
```

**On resume**: strips any `--session-id`/`--settings` pulpo itself inserted, then
produces `claude --resume <id> <remaining args>`. That command is run through
`prepare_spawn` again so a fresh `--settings` file is written and hooks stay wired —
`--session-id` is never re-added once `--resume` is present.

**Event mapping**: `SessionStart` → `SessionStarted`, `UserPromptSubmit` → `Working`,
`Stop` → `TurnFinished`, `StopFailure` → `Failed` (rate-limited when the error mentions
`rate`/`429`/`overloaded`), `SessionEnd` → `SessionEnded`, `Notification` → `NeedsInput`
(`permission_prompt` → Permission, `idle_prompt` → Idle, `agent_needs_input`/
`elicitation_*` → Question). Anything else is `Ok(None)` — pulpo doesn't care about it.

> Verified 2026-09-09 against Claude Code v2.1.266 with a real permission prompt: the
> `Notification` payload carries `"notification_type": "permission_prompt"` plus a
> human-readable `"message"`. `parse_event` reads `notification_type` (and tolerates
> `matcher`/`type`) before falling back to keyword-matching `message`.
>
> Known gap: Claude's *workspace trust* dialog ("Yes, I trust this folder") is shown
> before `SessionStart` fires, so no hook can report it. Until events flow the watchdog
> still applies the scrollback heuristics, and the built-in waiting patterns include that
> dialog's wording, so the session is reported as waiting for input anyway. `Stop` does
> not fire for a turn that was interrupted (Esc); `SessionEnd` still does.

## Event ingestion

`POST /api/v1/sessions/{id}/harness-events` (see the [API reference](/reference/api))
is the ingestion endpoint; `pulpo hook <harness>` (see the
[CLI reference](/reference/cli#hook-internal)) is what actually posts to it — command
hooks receive the harness's JSON on stdin and inherit the harness process's
environment, so `PULPO_SESSION_ID` (exported by the session wrapper into every
pulpo-managed process) is how the hook knows which session it's reporting for. A hook
always exits `0` and prints nothing on success: it must never block or break the agent
it's wired into, regardless of what the daemon does or doesn't do.

On ingestion the daemon resolves the session's adapter, calls `parse_event`, applies
the state transition below, stamps `harness_last_event_at`, stores
`harness_session_id` on `SessionStarted`, and emits the existing SSE `session` event —
notifications for `NeedsInput`/`Failed` ride that same event through the existing
webhook/push paths; no new channel was added.

## State transitions

| Event | Status | Other fields |
|---|---|---|
| `SessionStarted` | Active | sets `harness_session_id`; clears `needs_input` metadata |
| `Working` | Active | clears `needs_input`, `error_status` metadata |
| `TurnFinished` | Idle | `idle_since = now`; `last_summary` metadata (truncated to 200 chars) |
| `NeedsInput` | Idle | `needs_input` metadata = the reason; triggers a notification |
| `Failed` | Idle | `error_status`/`error_status_at` metadata (+ `rate_limit`/`rate_limit_at` when rate-limited); triggers a notification |
| `SessionEnded` | Ready if the backend is still alive, else Stopped | follows the same semantics as a command that exits on its own |

`Idle` therefore means "the agent is waiting at its prompt" in general; the
`needs_input` metadata field is what distinguishes "done with the turn" from "blocked
on me." `pulpo ls` and the web session list/detail render `needs input (<reason>)`
distinctly from plain `idle`. The `SessionStatus` enum itself is unchanged — this is
additive, not a state-machine rename. See
[Session Lifecycle](/operations/session-lifecycle) for the full state machine
(unaffected states/transitions aren't repeated here).

## Watchdog bypass

Once a session's `harness_last_event_at` is set (events are flowing), the watchdog
stops applying scrollback heuristics to it: no waiting-for-input pattern matching, no
rate-limit/error-status scraping, no time-based Active→Idle transition. Hook events
own those signals instead. Everything else keeps running unconditionally — memory
intervention, git telemetry, PR/branch detection, and `idle_timeout` (alert/kill after
N seconds idle) all still apply to harness-managed sessions exactly as they do to any
other. Sessions without events (a generic/unrecognized command, or a harness whose
hook installation silently failed) keep today's heuristics unchanged, since
`harness_last_event_at` never gets set for them.

The switch is one helper, `watchdog::harness_owns_state(session)`, checked in one
place (`watchdog/idle.rs::check_session_idle`).

## Persistence

`sessions` gained three nullable columns (migration `0007_harness.sql`): `harness`
(adapter id, e.g. `"claude"`; `NULL` only on rows from before this feature),
`harness_session_id` (the harness's own session/thread id), and
`harness_last_event_at` (timestamp of the last ingested event — the watchdog-bypass
switch above). All three are mirrored on `pulpo_common::Session` and the session JSON.

Adapter files live under `<data_dir>/harness/<session_id>/` and are removed whenever
the session itself is removed (`pulpo stop --purge`, `pulpo cleanup`) — the same
cleanup path that already reclaims worktrees and exit markers.

## What's not here yet

Codex and pi adapters are follow-up work on top of this branch — the trait and
registry are built to make that a matter of writing one more adapter, not touching
core daemon code. A `working / needs_input / done / exited / lost` rename of
`SessionStatus` itself (cleaner than overloading `Idle` + a `needs_input` metadata
flag) is a deliberate follow-up once hook-driven events are proven in the field, not
part of this change.
