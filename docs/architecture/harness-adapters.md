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
rewrite, no resume id, no events. Adding Gemini CLI (or any other harness) support
later means writing one more adapter and registering it — the trait and registry don't
need to change. Claude Code, Codex, and pi adapters ship today (sections below): shipped
for Claude Code (hook mechanics verified against v2.1.266), Codex and pi (implemented
from their docs, unverified in the field).

## Shipped: the Claude Code adapter

`harness/claude.rs` is the first concrete adapter. Verified against Claude Code
v2.1.266: `--session-id <uuid>`, `--settings <file-or-json>` (highest CLI precedence),
`-r`/`--resume [session-id]`, `-c`/`--continue`.

**On spawn** (no existing `--session-id`/`--settings` flag — those two mean identity is
already fully pinned down, so the command is spawned unchanged): generates a UUID,
writes `<data_dir>/harness/<session_id>/claude-settings.json` wiring every hook of
interest to `pulpo hook claude`, and rewrites the command to
`claude --session-id <uuid> --settings <path> <original args>`. The id is known
up front and stored immediately — resume still works even if the `SessionStart` hook
never fires (e.g. the agent crashes before Claude's hook runner starts). If the spawn
command already carries `--resume`/`-r`/`--continue`/`-c` (spawning straight into an
existing conversation), `--settings` is still injected so hooks stay wired — only
`--session-id` is skipped, since Claude Code doesn't expect both a resume target and a
preset id on the same invocation.

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
`rate`/`429`/`overloaded`), `SessionEnd` → `SessionEnded` **except** when its `reason` is
`clear` (`/clear`) or `resume` (`/resume`) — those are in-process session replacement,
not the harness process actually exiting (verified against the v2.1.266 binary's reason
enum: `clear, resume, logout, prompt_input_exit, other`), so they're ignored (`Ok(None)`)
rather than flipping a still-running session to `Ready`/`Stopped` out from under itself.
`Notification` → `NeedsInput` (`permission_prompt` → Permission, `idle_prompt` → Idle,
`agent_needs_input`/`elicitation_*` → Question). Anything else is `Ok(None)` — pulpo
doesn't care about it.

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

## Shipped: the pi adapter

`harness/pi.rs` is the second concrete adapter, for Mario Zechner's coding agent.

> **Package name change.** `github.com/badlogic/pi-mono` now redirects to
> `github.com/earendil-works/pi`, and the npm package `@mariozechner/pi-coding-agent`
> is deprecated and frozen at `0.73.1` — which has **neither** `agent_settled` nor
> `ui_prompt_start`/`ui_prompt_end` nor `--session-id`, the three primitives this
> adapter depends on. The active package is `@earendil-works/pi-coding-agent`; this
> adapter is verified against `0.85.1`'s shipped `dist/*.d.ts`/`dist/*.js`/`docs/*.md`,
> plus targeted runtime checks against the real installed `0.85.1` binary (flag-conflict
> exit codes, subcommand dispatch, and — critically — the `pulpo.ts` extension's own
> event-ordering fix: loaded and ran under `pi -e <path> -p ... --no-session`, correctly
> ordering `session_start` before `session_shutdown` even under an artificially slow
> `session_start` hook that reliably reproduced the opposite, broken order against the
> pre-fix version of the file). No API key was available, so the full event mapping
> table (`agent_start`/`agent_settled`/`ui_prompt_start`/`ui_prompt_end` firing from a
> real model turn) remains **unverified in the field** — only startup/shutdown fired in
> testing.

Unlike Claude Code, pi has no separate resume flag: `--session-id <id>` is
**idempotent create-or-open**, scoped to `(cwd, sessionDir)` — it opens the session if
one already exists under that id for the current working directory, or creates it if
not. That makes a fresh spawn and a resume the *same command shape*,
`pi --session-id <id> -e <ext_path> <args>`, differing only in whether `<id>` already
has a session file on disk.

**On spawn** (no existing `--session`/`--continue`/`-c`/`--resume`/`-r`/`--fork`/
`--no-session` flag, and the token right after `pi` isn't one of its own subcommands —
see below): writes `<data_dir>/harness/<session_id>/pulpo.ts`, an extension file
(loaded via `-e`, a repeatable flag with no conflict against any session flag) that
wires pi's own event bus to `pulpo hook pi --event <name>` for `session_start`,
`agent_start`, `agent_settled`, `ui_prompt_start`, `ui_prompt_end`, and
`session_shutdown` (`agent_end` is also hooked, but only to cache the last assistant
message for `agent_settled` to report — it never posts on its own; see the file's
comments for why `agent_settled` rather than the more frequent `turn_end`/`agent_end`
is the "turn finished" signal). Reuses the pulpo session uuid as pi's `--session-id`
(it already matches pi's id regex) and rewrites the command to
`pi --session-id <uuid> -e <path> <original args>`. A `--session-id` with no value at
all (the last token in the command) mints the pulpo uuid as its value instead of
leaving it valueless — otherwise the inserted `-e <path>` would become the (invalid)
session id and pi would reject the command. Flag scanning (both the session-selection
no-op check and the `--session-id`-presence check) stops at a literal `--` separator,
since everything after it is a positional argument (e.g. a chat message), never a
flag.

`prepare_spawn` is also a full no-op when the token immediately after `pi` is one of
its own subcommands (`install`, `remove`, `uninstall`, `update`, `list`, `config`,
`auth` — verified via `pi --help`, 0.85.1): each is dispatched by matching `args[0]`
before pi's normal chat-session parser ever runs, so inserting pulpo's flags right
after `pi` would push the subcommand word to `args[2]` and silently turn `pi update`
into a chat session with `"update"` as the prompt instead of actually running the
update.

If `--session-id` is *already* present in the command (this is exactly what happens
when `resolve_resume_command` in `session/manager.rs` calls `resume_command` first —
see below), `prepare_spawn` leaves it
exactly as-is and only adds `-e <path>` with a freshly-rewritten extension file. This
is the one place pi's adapter deliberately diverges from Claude's no-op rule: Claude
fully no-ops when its own identity flag (`--session-id`) is already present, but for
pi that flag *is* the resume mechanism, so a full no-op there would mean hooks never
get re-wired on resume.

**On resume**: strips any existing `--session-id <val>`, then inserts
`--session-id <id>` right after argv0 — nothing else changes. Returns `None` (falls
back to the plain original command) if `--session`, `--continue`/`-c`, `--resume`/`-r`,
or `--fork` is present. The first three are a hard error in pi itself
(`validateSessionIdFlags`, exit 1) whenever combined with `--session-id`; `--fork` is
*not* rejected when `--session-id` names a brand-new id (used together to fork *into*
a chosen id), but resuming means the id, by definition, already has a session on
disk — and pi's `createSessionManager` exits 1 with `Session already exists with id
'<id>'` in exactly that case (verified against 0.85.1) — so `resume_command` treats
`--fork` as a conflict too. That command is run through `prepare_spawn` again, which
(per the paragraph above) adds `-e <path>` without touching `--session-id`.

**Event mapping**: `session_start` → `SessionStarted` (`resumed` is advisory — pi
itself can't distinguish "opened an existing session" from "created a fresh one with
the given id" at `reason: "startup"`; pulpo's call site already knows the real
answer), `agent_start` → `Working`, `agent_settled` → `TurnFinished` when its `error`
field is null, else `Failed` (rate-limited on a case-insensitive match for
`rate.?limit`/`too many requests`/a standalone `429` — not a bare substring, so
`"request 14290 done"` doesn't false-positive — narrowed from pi's own broader
retryable-error classifier, which also treats 5xx/timeouts/overloaded as retryable but
not necessarily rate-limited), `ui_prompt_start` → `NeedsInput` (`kind: "confirm"` →
Permission, anything else → Question), `ui_prompt_end` → `Working` (clears
`needs_input`; not something pi's docs call a distinct state, just the natural "the
blocking dialog is over" signal), `session_shutdown` → `SessionEnded` only when
`reason: "quit"` (a real process exit); every other reason (`reload`/`new`/`resume`/
`fork`) is in-process session replacement and is `Ok(None)` — a fresh `session_start`
follows immediately.

**Event ordering**: each `reportEvent` call inside `pulpo.ts` spawns a detached
`pulpo hook pi` child process, and pi awaits every handler in turn (including
`session_shutdown` before it exits) — but a plain fire-and-forget spawn per event
raced independent child processes against each other, so two hook POSTs could reach
the daemon out of order (reproduced: `session_shutdown` logged before `session_start`
in some runs). The extension instead threads every `reportEvent` call through one
`chain` promise and `await`s it before the handler returns, so pi's own sequential
handler-awaiting forces one event's child process to fully finish (or hit a 2.5s
safety timeout) before the next event's handler is even invoked.

`matches()` is a plain basename check (`pi`) with no version probing — an installed
`pi` binary older than whatever version first shipped `agent_settled`/
`ui_prompt_*`/`--session-id` (somewhere between `0.73.1` and `0.85.1`, exact version
unverified) will run without firing any of these events; the extension load itself
degrading gracefully is pi's problem, not this adapter's. `pi` run via `npx pi ...`
resolves to argv0 basename `npx`, which this adapter does not claim — a known,
documented gap (no `npx`-aware unwrapping), not a bug.

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
`<data_dir>/harness/<session_id>/codex-home/` and seeds it with:
- a copy of the real `auth.json` (from `$CODEX_HOME` if pulpod's own process has that
  env var, else `~/.codex`; skipped silently if absent), with its mode forced to
  `0600` regardless of the source file's own mode;
- symlinks to the user's real `AGENTS.md`, `skills/`, `rules/`, `plugins/`, `prompts/`,
  `memories/`, cached model list (`models_cache.json`), and `installation_id`, when
  present — so a pulpo-spawned session still sees the user's own instructions/skills
  instead of an empty isolated home. Idempotent: an existing symlink (or anything else
  already at the destination) is left alone, so a resume never fails re-linking these.
  Deliberately excluded: `sessions/`/`history.jsonl` (each pulpo session gets its own
  Codex thread history), `log/`, and Codex's sqlite state;
- `config.toml`, built by parsing the user's real one (if any) into a `toml::Table` and
  merging pulpo's own `notify` and `[[hooks.<Event>]]` entries into it as structured
  data — see [Config merge](#codex-config-merge) below — for `SessionStart`,
  `UserPromptSubmit`, `Stop`, `SessionEnd`, and `PermissionRequest`;

and rewrites the command to `codex --dangerously-bypass-hook-trust <original args>`
with `CODEX_HOME` set to that directory. Unlike Claude, Codex has no flag to preset a
session/thread id at launch — `harness_session_id` is `None` until a `SessionStart`
hook reports it (the rollout-discovery fallback below resumes a lost thread without
ever learning its id either).

Each hook command is `<pulpo-bin> hook codex --event <Name>` — a deliberate deviation
from relying on an unconfirmed event-name field in Codex's own hook JSON (unlike
Claude's `hook_event_name`), reusing the `--event` mechanism `pulpo hook` already
offers generically. `<pulpo-bin>` is shell-quoted (`shell_words::quote`) both here and
in the `notify` line below, since each becomes part of a shell command string Codex
executes verbatim — an unquoted path containing a space would otherwise be split into
multiple arguments.

<a id="codex-config-merge"></a>**Config merge, not string concatenation**: the
adapter parses the user's real `config.toml` (if any) into a `toml::Table` — an
unparseable file only warns and starts from an empty table — then sets `notify` (any
pre-existing `notify` is *replaced*, not merged: Codex's config format has only one
`notify` slot, and a `CODEX_HOME`-wide hook takeover is inherently exclusive with the
user's own `notify` setup) and *appends* pulpo's own handler to each `hooks.<Event>`
array, creating it if missing and preserving any hooks the user already configured for
that event. An earlier version of this adapter built the file by string concatenation
(the user's real `config.toml` pasted in, then pulpo's `notify`/`hooks` keys appended
as text) — this broke on any machine whose real `~/.codex/config.toml` already had a
root `notify` key (a second `notify = [...]` line makes the document invalid TOML,
and Codex refuses to start) and had a companion table-scoping hazard (a bare key
appended after one or more `[table]` headers gets silently absorbed into whichever
table came last, rather than landing at the document root). The structured merge
avoids both: there's no such thing as "the wrong scope" once `notify`/`hooks` are
inserted as keys of the root `Table` value directly.

**`notify` and the `codex-notify` CLI variant**: Codex's `notify` config delivers its
JSON as a trailing argv element, not stdin, so the adapter also wires
`notify = ["sh", "-c", "'<pulpo-bin>' hook codex-notify \"$0\""]`. See the
[CLI reference](/reference/cli#hook-internal) for `pulpo hook codex-notify`'s contract
— it posts the raw notify payload, unmodified, as harness `"codex"`; `parse_event`
maps its `type` field, `agent-turn-complete`, to a single `TurnFinished` event. An
earlier version also posted a synthetic `SessionStart`-shaped event first whenever the
payload carried a thread id, hoping to learn `harness_session_id` even if the real
`SessionStart` hook never fired — but that fired on *every* `agent-turn-complete`
notification, not just the first, which flapped the session Active (via the synthetic
event) then Idle (via the real `TurnFinished` right after it) on every turn, and
duplicated the `Stop` hook's own `TurnFinished` for the same turn boundary. Removed
rather than fixed to fire once: `session::manager::apply_harness_event` only stores
`harness_session_id` from a `SessionStarted`-shaped event
(`harness::transition_for_event`), and the CLI process is a one-shot per invocation
with no way to ask the daemon whether it already knows the session's id before
deciding whether to post one. A lost session whose `SessionStart` hook never fired is
instead recovered by the rollout-discovery fallback below, on its *next* spawn/resume.

**Rollout-discovery fallback (lost session recovery)**: if a pulpo-spawned session's
`SessionStart` hook never fires, `harness_session_id` stays unknown and a later resume
attempt falls back to replaying the session's *original* command (see
`session::manager::resolve_resume_command`) — with no `resume` token at all, which
would otherwise start a brand new Codex thread and silently lose the old one.
`prepare_spawn` guards against this: when the command has no `resume` token but
`<codex-home>/sessions/` already contains at least one `rollout-*.jsonl` file
(recursively, since Codex nests them under `sessions/YYYY/MM/DD/`), it rewrites the
command to `codex --dangerously-bypass-hook-trust resume --last <original args>`
instead. This is safe specifically *because* `CODEX_HOME` is isolated per pulpo
session: any rollout under this exact directory can only be that same session's own
prior thread, so `--last` resumes it correctly without needing to know its id.

**On resume**: `resume_command` strips any existing `resume <id>`/`--last` and any
trailing positional prompt argument, then produces `codex resume <id> <remaining
flags>`. Codex replays a positional argument as a brand new turn
(`codex resume <id> 'fix the bug'` submits "fix the bug" again on top of the resumed
session) — an earlier version of this adapter kept the original prompt positional,
which re-submitted it as a duplicate turn on every resume; it's now stripped
(non-flag tokens that aren't a preceding flag's value, from a fixed list of Codex's
documented value-taking global flags — `-m/--model`, `-s/--sandbox`,
`-a/--ask-for-approval`, `-c/--config`, `-C/--cd`, `-p/--profile`). For
`codex exec ...`, `resume` nests *after* `exec` instead
(`codex exec resume <id> <flags>`) — **UNVERIFIED**: no confirmed docs/example of this
exact shape, inferred from [PR #26434](https://github.com/openai/codex/pull/26434)
("Preserve hook trust bypass in codex exec threads"), which explicitly forwards the
bypass flag for "fresh thread start/resume/fork" under `codex exec`. `prepare_spawn`
runs again on the resulting command and **reuses the same `codex-home` directory**
(`create_dir_all` is a no-op when it already exists) — Codex's session rollout files
live under it, so resuming must keep using the same isolated `CODEX_HOME`. The adapter
tells its own `resume_command` output apart from a user directly resuming a session
pulpo has never isolated (redirecting `CODEX_HOME` for the latter would break it — the
target wouldn't exist under a fresh, empty isolated dir) by checking whether this
session's `codex-home` directory already exists.

**Event mapping**: `SessionStart` → `SessionStarted` (accepts `session_id`, as
documented, or `thread_id`/`thread-id` — the same tolerance the `notify` payload's
fields get, in case a hook payload ever uses that mechanism's field naming instead),
`UserPromptSubmit` → `Working`, `Stop` → `TurnFinished`, `SessionEnd` →
`SessionEnded`, `PermissionRequest` → `NeedsInput{Permission}` (the hook itself never
emits `hookSpecificOutput`, so pulpo stays observational and the real TUI approval
prompt still runs), notify `agent-turn-complete` → `TurnFinished`.
`PreToolUse`/`PostToolUse`/`PreCompact`/`PostCompact`/`SubagentStart`/`SubagentStop`/
`Interrupt` exist but map to `Ok(None)` — out of scope for pulpo's state machine today.
`parse_event` routes on `hook_event_name` before falling back to the notify payload's
`type` field, so a hook event is never misrouted as a notify payload.

**Usage scan**: `usage::scan_local_usage` counts Codex rollouts from the user's real
`~/.codex` *and* every pulpo-spawned session's isolated
`<data_dir>/harness/<session_id>/codex-home` (deduped by each rollout file's
canonicalized path) — the real `~/.codex` alone would miss every Codex session pulpo
itself spawned, since its rollout files live under the isolated `codex-home` instead.
`pulpo cleanup` deletes each session's `codex-home` — and the rollouts under it —
together with the rest of that session's harness dir.

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
pulpo-managed process, alongside `PULPO_SESSION_NAME` and `PULPO_URL`) is how the hook
knows which session it's reporting for. `PULPO_URL` (`http://127.0.0.1:<port>`) is what
the hook actually posts to, so it reaches the daemon on whatever port it's configured
with rather than a hardcoded default. A hook always exits `0` and prints nothing on
success: it must never block or break the agent it's wired into, regardless of what the
daemon does or doesn't do.

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

The Claude Code adapter's hook mechanics were verified against a real binary
(v2.1.266); the Codex and pi adapters are built from their docs and shipped code but
are unverified in the field (see their caveats). A Gemini CLI adapter is follow-up work
— the trait and registry make that a matter of writing one more adapter, not touching
core daemon code. A `working / needs_input / done / exited / lost` rename of
`SessionStatus` itself (cleaner than overloading `Idle` + a `needs_input` metadata
flag) is a deliberate follow-up once hook-driven events are proven in the field, not
part of this change.

Exact rate-limit/error detection for Codex sessions is still heuristic (scrollback
scraping) rather than hook-driven, since Codex has no such hook today — see the
Codex adapter section above and [Watchdog bypass](#watchdog-bypass).
