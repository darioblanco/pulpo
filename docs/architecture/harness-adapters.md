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
    fn owned_signals(&self) -> HarnessSignals { HarnessSignals::all() } // default impl
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

- **`owned_signals`** — which watchdog scrollback heuristics this adapter's own events
  replace, once they're flowing (see [Watchdog bypass](#watchdog-bypass)). Defaults to
  "all"; an adapter missing some signals (Codex has no error/rate-limit hook) overrides
  it so the watchdog keeps covering those from scrollback.

`HarnessRegistry` (`harness/registry.rs`) holds adapters in priority order and
resolves a command line via `shell_words::split` + basename of `argv[0]` — handling
`env FOO=bar claude`, absolute paths, etc. `GenericAdapter` always matches last: no
rewrite, no resume id, no events. Adding pi/Gemini support later means writing one
more adapter and registering it — the trait and registry don't need to change.

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

> The exact JSON field a `Notification` hook uses to carry its matcher tag couldn't be
> independently confirmed from the CLI surface alone. `parse_event` checks a few
> plausible field names (`matcher`, `notification_type`, `type`) before falling back
> to keyword-matching the human-readable `message` text — worth re-verifying against a
> real fired hook and tightening if the field name turns out to differ.

## Shipped: the Codex adapter

`harness/codex.rs` mirrors the Claude adapter's isolated-file approach at one remove.
Codex has no `--settings`-equivalent flag and no confirmed way to inject a `[hooks]`
table via `-c`, but `CODEX_HOME` governs `config.toml`, `auth.json`, `history.jsonl`,
and the `sessions/` directory together — so the adapter redirects the whole thing per
pulpo session instead of writing one settings file.

**Requires Codex CLI ≥ the fix in [PR #24317](https://github.com/openai/codex/pull/24317)**
(verified against 0.153.0): `--dangerously-bypass-hook-trust` was broken for the
interactive TUI in 0.131.0–0.133.0 ([issue #24093](https://github.com/openai/codex/issues/24093))
— without a working bypass flag, the first hook fires a blocking "Hooks need review" TUI
prompt pulpo can't answer, and the adapter's spawn rewrite is ineffective.

**On spawn** (no existing `resume` subcommand, `--dangerously-bypass-hook-trust` flag,
or `CODEX_HOME=` prefix — see below): creates
`<data_dir>/harness/<session_id>/codex-home/`, copies the real `auth.json` in (from
`$CODEX_HOME` if pulpod's own process has that env var, else `~/.codex`; skipped
silently if absent), writes `config.toml` (the user's real one, if any, plus pulpo's
`notify` line and `[[hooks.<Event>]]` tables for `SessionStart`, `UserPromptSubmit`,
`Stop`, `SessionEnd`, and `PermissionRequest`), and rewrites the command to
`codex --dangerously-bypass-hook-trust <original args>` with `CODEX_HOME` set to that
directory. Unlike Claude, Codex has no flag to preset a session/thread id at launch —
`harness_session_id` is `None` until a `SessionStart` hook (or the `notify` fallback,
see below) reports it.

Each hook command is `<pulpo-bin> hook codex --event <Name>` — a deliberate deviation
from relying on an unconfirmed event-name field in Codex's own hook JSON (unlike
Claude's `hook_event_name`), reusing the `--event` mechanism `pulpo hook` already
offers generically.

**`notify` and the `codex-notify` CLI variant**: Codex's `notify` config delivers its
JSON as a trailing argv element, not stdin, so the adapter also wires
`notify = ["sh", "-c", "'<pulpo-bin>' hook codex-notify \"$0\""]`. See the
[CLI reference](/reference/cli#hook-internal) for `pulpo hook codex-notify`'s contract —
it maps `agent-turn-complete` to `TurnFinished` and, when the payload carries a
session/thread id, also posts a synthetic `SessionStart`-shaped event first so pulpo
learns the harness session id even if the `SessionStart` hook itself never fired.

**On resume**: `resume_command` strips any existing `resume <id>`/`--last`, then
produces `codex resume <id> <remaining args>`. `prepare_spawn` runs again on that
command and **reuses the same `codex-home` directory** (`create_dir_all` is a no-op
when it already exists) — Codex's session rollout files live under it, so resuming
must keep using the same isolated `CODEX_HOME`. The adapter tells its own
`resume_command` output apart from a user directly resuming a session pulpo has never
isolated (redirecting `CODEX_HOME` for the latter would break it — the target wouldn't
exist under a fresh, empty isolated dir) by checking whether this session's
`codex-home` directory already exists.

**Event mapping**: `SessionStart` → `SessionStarted`, `UserPromptSubmit` → `Working`,
`Stop` → `TurnFinished`, `SessionEnd` → `SessionEnded`, `PermissionRequest` →
`NeedsInput{Permission}` (the hook itself never emits `hookSpecificOutput`, so pulpo
stays observational and the real TUI approval prompt still runs), notify
`agent-turn-complete` → `TurnFinished`. `PreToolUse`/`PostToolUse`/`PreCompact`/
`PostCompact`/`SubagentStart`/`SubagentStop`/`Interrupt` exist but map to `Ok(None)` —
out of scope for pulpo's state machine today.

> No Codex hook distinguishes "asked a clarifying question" from "finished the turn"
> (both are `Stop`), and no hook or notify event carries API-error/429/quota
> information — `SessionEnd.reason` is documented as always `"other"` today. **Because
> of this, Codex sessions keep the scrollback-based `detect_rate_limit`/`detect_error`
> heuristics running even once its lifecycle hooks are flowing** — see
> [Watchdog bypass](#watchdog-bypass) below for the mechanism. Exact
> error/rate-limit detection for Codex stays heuristic until Codex ships an error hook.
>
> Also UNVERIFIED: the exact field spelling in Codex's `notify` payload for the
> session/thread id and turn summary — `parse_event` and the CLI's
> `codex-notify` handler both check `thread_id`/`thread-id` and
> `last_assistant_message`/`last-assistant-message`.

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
stops applying scrollback heuristics **that the session's own harness adapter owns**:
`HarnessAdapter::owned_signals()` returns a [`HarnessSignals`] value (`lifecycle`,
`rate_limit`, `error`) saying which of the following the adapter's own events replace:

- `lifecycle` — waiting-for-input pattern matching and the time-based Active→Idle
  transition.
- `rate_limit` — `detect_rate_limit` scrollback scraping.
- `error` — `detect_error` scrollback scraping.

The default implementation returns "all" — correct for an adapter (Claude's) whose
hooks cover lifecycle, errors, and rate limits alike. An adapter missing some of those
signals overrides it: **Codex has no error/rate-limit hook**, so
`CodexAdapter::owned_signals()` returns "lifecycle only," and `detect_rate_limit`/
`detect_error` keep running from scrollback for Codex sessions even while its
lifecycle events (`SessionStart`/`Stop`/`PermissionRequest`/...) are flowing.

Everything else keeps running unconditionally regardless of ownership — memory
intervention, git telemetry, PR/branch detection, and `idle_timeout` (alert/kill after
N seconds idle) all still apply to harness-managed sessions exactly as they do to any
other. Sessions without events (a generic/unrecognized command, or a harness whose
hook installation silently failed) keep today's heuristics unchanged for every signal,
since `harness_last_event_at` never gets set for them —
`watchdog::owned_signals(session)` treats "events not flowing" as owning nothing,
regardless of what the adapter would otherwise claim.

`watchdog::owned_signals(session)` is the one helper that combines
`harness_owns_state` (is `harness_last_event_at` set) with the adapter's own
`owned_signals()` (via a small process-wide `HarnessRegistry`); it's checked in one
place (`watchdog/idle.rs::check_session_idle`, which passes the result into
`detect_and_store_output_metadata` and gates the waiting-for-input/time-based-idle
block directly).

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

A pi adapter is follow-up work on top of this branch — the trait and registry are
built to make that a matter of writing one more adapter, not touching core daemon
code. A `working / needs_input / done / exited / lost` rename of `SessionStatus`
itself (cleaner than overloading `Idle` + a `needs_input` metadata flag) is a
deliberate follow-up once hook-driven events are proven in the field, not part of this
change.

Exact rate-limit/error detection for Codex sessions is still heuristic (scrollback
scraping) rather than hook-driven, since Codex has no such hook today — see the
Codex adapter section above and [Watchdog bypass](#watchdog-bypass).
