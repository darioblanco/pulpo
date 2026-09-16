# Session Lifecycle

Complete reference for Pulpo session states, transitions, and detection mechanisms.

## State Machine

```
  spawn           agent working        agent exits
    │                   │                    │
    ▼                   ▼                    ▼
┌──────────┐      ┌──────────┐         ┌──────────┐
│ STARTING │──────▶│ WORKING  │────────▶│   DONE   │
└──────────┘      └──────────┘         └──────────┘
                    ▲      │                  ▲
             output │      │ turn finished /  │
            changed │      │ needs input      │
                    │      ▼                  │
                    │ ┌──────────┐            │
                    └─│ WAITING  │────────────┘
                      └──────────┘   watchdog / user /
                                      dead backend + exit marker

                    ┌──────────┐
                    │   LOST   │◀── dead backend, no exit marker
                    └──────────┘         (from Working or Waiting)
```

## States

| State | Meaning | Terminal? |
|-------|---------|-----------|
| **Starting** | Spawn requested — the backend (tmux session) hasn't been confirmed yet | No |
| **Working** | The agent process is running and busy — terminal output is changing | No |
| **Waiting** | The agent is at its prompt. `status_reason` says why: `needs_input:<reason>` when it's blocked on the user (a permission prompt, a question, an idle-prompt, or another harness-specific reason — from a hook or a scrollback waiting-pattern match), or plain `idle` when the turn simply finished (or there's no new output) with nothing specifically blocking on you | No |
| **Done** | The agent process has exited **and** the backend (tmux session) is gone. `exit_code` is recorded when known; `status_reason` says how: `exited` (clean end), `stopped` (explicit `pulpo stop`), or an intervention code (`idle_timeout`, `budget_exceeded`, or `memory_pressure` — historical only, the memory-pressure intervention was removed in September 2026, see [ROADMAP.md](https://github.com/darioblanco/pulpo/blob/main/ROADMAP.md) "Removed"). Replaces the old `ready` **and** `stopped` — both always meant "not running, and resumable"; the *how* moved from a distinguishable top-level status into `status_reason` | Yes (resumable) |
| **Lost** | The backend died with no evidence of a clean end — crash, reboot, or external kill mid-run | Yes (resumable) |

## Transitions

### Starting → Working
- **Trigger**: The backend confirms the session is up and running. For a harness-driven command this lines up with the harness's own `SessionStarted`/`Working` hook event; for any other command it's simply the backend having been created successfully.
- **Detection**: After the backend creates the session successfully, Pulpo marks it `working` immediately. Separate liveness checks handle later `working`/`waiting` → `lost` transitions.

### Working → Waiting
- **Trigger**: Watchdog detects output unchanged — either via known waiting patterns (immediate) or sustained unchanged output (configurable, default 60 seconds) — or, for a harness-driven session, a `TurnFinished`/`NeedsInput` hook event.
- **Detection**: The watchdog compares `output_snapshot` on each tick. Two scrollback paths, both landing on `waiting`:
  1. **Pattern match (immediate)**: If output is unchanged and the last 5 lines match known waiting patterns (permission prompts, "what's next?" prompts), the session moves to `waiting` with `status_reason = needs_input:permission` on the first unchanged tick — a matched pattern really is "blocked on the user," not just idle. See [Waiting Patterns](#waiting-patterns-idle-detection) below.
  2. **Sustained silence (universal)**: If `last_output_at` exceeds `idle_threshold_secs` (default: 60, configurable in `[watchdog]`), the session moves to `waiting` with `status_reason = idle`, regardless of terminal content. Per-session override via `idle_threshold_secs` on the session (`0` = never idle).

  A harness with its own lifecycle hooks skips both scrollback paths once its events are flowing: `TurnFinished` sets `waiting`/`idle` directly, and `NeedsInput` sets `waiting`/`needs_input:<reason>` directly. See [Harness-Driven Transitions](#harness-driven-transitions-claude-code-codex-pi) below.

### Waiting → Working
- **Trigger**: Watchdog detects output changed since last tick (for a session the watchdog still owns — see [Watchdog bypass](../architecture/harness-adapters.md#watchdog-bypass)), or a harness's `SessionStarted`/`Working` hook event.
- **Detection**: New output in the terminal means the agent (or user) resumed work; a harness event clears any `needs_input:<reason>` reason immediately and sets `status_reason` back to `None`, regardless of what scrollback shows.

### Working/Waiting → Done
- **Trigger**: The wrapper's `{id}.code` exit-marker file appears (written the moment the agent command finishes, containing its exit code) and the wrapper shell then exits immediately — there is no more lingering fallback shell keeping the backend alive after the agent process ends (see [Exit Markers](#exit-markers) below) — or the session's shell otherwise exits normally (the user typed `exit` in a bare-shell session, or an explicit `pulpo stop`/watchdog intervention ended it).
- **Detection**: The watchdog's own idle-check tick calls `is_alive()` on every `working`/`waiting` session *before* attempting any output capture, scraping, or idle logic — a dead backend is resolved (and its `lifecycle` event/webhook fired) within that same tick, not whenever something next happens to call the API. The same resolution also runs lazily on `get_session`/`list_sessions` (whichever notices a dead backend first wins) and eagerly again at startup via `resume_lost_sessions`. Dead-backend classification is then simply: exit marker present → `done` (`status_reason = exited`, `exit_code` persisted from the marker) — unless an explicit stop or watchdog intervention already recorded a more specific reason (`stopped`, `idle_timeout`, `budget_exceeded`, `memory_pressure`) — no marker → `lost` (see below).
- **Harness-driven sessions**: a `SessionEnded` hook event leaves the status alone — the agent process is typically still in the middle of exiting when the hook fires — **unless** the `.code` exit marker has already landed by the time the event is processed, in which case the session moves straight to `done`/`exited` immediately. Otherwise the ordinary dead-backend classification above catches it on the watchdog's next tick, and `exit_code` still ends up recorded the same way either way.
- **Side effects**: SSE event emitted.

### Working/Waiting → Lost
- **Trigger**: `is_alive()` returns false for a session that was `working` or `waiting` **and no exit marker exists** — the tmux process died without the wrapper running to completion (crash, reboot, `tmux kill-session`/`kill-server` mid-run).
- **Detection**: The same `is_alive()` check described above (the watchdog's own tick, `get_session`/`list_sessions`, or `resume_lost_sessions` at startup) finds the backend gone and consults the markers; with none present the session is marked `lost`. A 5-second grace period protects freshly spawned sessions from false positives.

Sessions stay listed once they reach `done` — there is no TTL-based auto-purge. The
session record is only reclaimed by an explicit `pulpo stop [--purge]`, `pulpo rm`/
`DELETE /api/v1/sessions/:id`, or `pulpo cleanup`. (There is no more lingering fallback
tmux shell to separately reclaim — see [Exit Markers](#exit-markers).)

## Resume Semantics

| From State | Resume? | What happens |
|-----------|---------|--------------|
| **Lost** | Yes | Recreates tmux session, re-executes the session command |
| **Done** | Yes | Recreates the backend and re-executes the command — the backend is already gone by the time a session reaches `done` (no fallback shell to reuse), so resume always starts a fresh backend rather than just flipping the status back to `working` |
| **Working/Waiting** | No | Error: session is still running |
| **Starting** | No | Error: session is still running |

For a session whose harness has a resume mechanism (Claude Code, Codex, pi), "re-executes
the session command" above means the harness's *own* resume command — `claude --resume <id>`,
`codex resume <id>`, or pi's idempotent `--session-id <id>` — so the conversation
continues where it left off instead of starting fresh. See
[Harness Adapters](../architecture/harness-adapters.md) for the exact rewrite per harness.

`pulpo attach` on a `done`/`lost` session errors with a hint pointing at `pulpo resume`
or `pulpo logs` instead — there's no live backend left to attach to.

## Harness-Driven Transitions (Claude Code, Codex, pi)

Everything above describes scrollback-based detection — the default for any command.
For a harness with its own lifecycle hooks, `pulpo hook <harness>` reports real events
(`SessionStarted`, `Working`, `TurnFinished`, `NeedsInput`, `Failed`, `SessionEnded`) that
drive the *same* `working`/`waiting`/`done`/`lost` states directly, and — once a session's
first event lands — the watchdog stops applying the waiting-for-input/rate-limit/error
heuristics below for whatever signals that harness's events cover (an adapter missing a
signal, like Codex's rate-limit/error detection, keeps the scrollback fallback for just
that signal).

Event mapping:

- **`SessionStarted`/`Working`** → `working`, clearing any `needs_input:<reason>` status
  reason.
- **`TurnFinished`** → `waiting`, `status_reason = idle`.
- **`NeedsInput`** → `waiting`, `status_reason = needs_input:<reason>` (`permission`,
  `question`, `idle`, or a harness-specific label), rendered as `waiting (needs input:
  <reason>)` in `pulpo ls` and the web UI — distinguishing "blocked on me" from a plain
  idle prompt.
- **`Failed`** → `waiting`, `status_reason = idle` — the error itself is still recorded
  in metadata (`error_status`/`error_status_at`), same as before this model changed; only
  the status/`status_reason` side of the transition was renamed.
- **`SessionEnded`** → leaves the status alone (the agent process is typically still
  exiting) unless the `.code` exit marker has already landed by the time the event is
  processed, in which case the session moves straight to `done`/`exited` immediately —
  the marker is written by the wrapper only once the agent process actually terminates,
  which can lag slightly behind the harness's own "I'm done" hook. Otherwise the ordinary
  dead-backend classification above (see
  [Working/Waiting → Done](#workingwaiting--done)) catches it on the watchdog's next tick,
  same as any other command.

`status_reason` is a new, plain-string field on the session (and on the SSE
`SessionEvent` payload) — not a nested type — that carries what used to be either a
distinguishable top-level status (`ready` vs. `stopped`) or an ad hoc metadata key
(`needs_input`). The SSE payload also still carries a legacy `needs_input: string | null`
field for one release (deprecated), populated from `status_reason` whenever it's
`needs_input:<reason>`. Full event mapping, per-harness spawn/resume rewrites, and the
watchdog-bypass mechanism: [Harness Adapters](../architecture/harness-adapters.md).

## Waiting Patterns (Idle Detection)

The watchdog inspects the last 5 lines of terminal output for these patterns (case-insensitive). The built-in patterns cover major coding agents and common CLI prompts:

- **Generic**: `(y/n)`, `[Y/n]`, `[yes/no]`, `(yes/no)`, `Yes / No`, `Do you trust`, `Press Enter`, `approve this`, `Are you sure`, `Continue?`, `Confirm?`, `Proceed?`
- **Claude Code**: `(Y)es`, `(N)o`, `(A)lways`, `Do you want to proceed`, `I trust this folder`, `Enter to confirm`
- **Codex CLI**: `Allow command?`
- **Gemini CLI**: `Allow?`, `Approve?`
- **Aider**: `to the chat?`, `Apply edit?`, `shell command?`, `Create new file`
- **Amazon Q**: `Allow this action?`, `Accept suggestion?`
- **SSH/sudo**: `continue connecting (yes/no)`, `'s password:`, `[sudo] password`

A match moves the session straight to `waiting` with `status_reason = needs_input:permission`
— the same shape a harness's own `NeedsInput{Permission}` hook event produces — rather than
plain `idle`, since a `(y/n)` prompt or a `sudo password:` line really is "blocked on the
user." Add custom patterns via `waiting_patterns` in `[watchdog]` config — they are
appended to the built-in list and matched the same way.

### Idle-timeout breaker exempts `needs_input`

`idle_timeout_secs`/`idle_action` (below) never fires on a session whose
`status_reason` is `needs_input:<reason>` — it's blocked on a real decision from the
operator, not "idle" in the sense this breaker means, and might legitimately sit there
for hours waiting for a person to come back. The alert/kill action still applies to a
plain `waiting:idle` session (sustained silence, no pending prompt) and to a
harness-owned `working` session producing no output at all.

## Configuration

### Watchdog (in `config.toml`)

```toml
[watchdog]
enabled = true
check_interval_secs = 10     # How often to check
idle_timeout_secs = 600       # Seconds before idle action triggers
idle_action = "alert"         # "alert" (mark idle_since) or "kill"
idle_threshold_secs = 60      # Seconds of unchanged output before Working→Waiting (default: 60)
waiting_patterns = []         # Extra patterns for waiting-for-input detection
```

### Notification Events

Webhook endpoints filter the universal event stream by `<type>.<subtype>` globs (empty
means all). Session state changes are `lifecycle` events, and the subtype is the session's
new status directly (`lifecycle.starting`, `lifecycle.working`, `lifecycle.waiting`,
`lifecycle.done`, `lifecycle.lost`):

```toml
[[webhooks]]
name = "primary"
url = "https://example.com/hooks/pulpo"
events = ["lifecycle.done", "lifecycle.lost"]
```

See the [config reference](../reference/config.md#webhooks) for `min_severity` and the full
event catalogue. The legacy `[[notifications.webhooks]]` form still works.

## Exit Markers

Two marker files under `{data_dir}/exit/` are what let the daemon distinguish an
intentional end (a session that finished on its own) from tmux disappearing out from
under a still-running session:

| Marker | Written by | Meaning |
|--------|-----------|---------|
| `{id}.code` | The wrapped agent command, immediately after it exits (`$?`) | The agent process finished; contains its exit code |
| `{id}.clean` | The wrapper script (or a bare-shell spawn's own login shell), as the very last thing it does before exiting | The session's shell ended normally |

**No more lingering fallback shell.** Previously, once the wrapped agent process
exited, the wrapper dropped into a fallback interactive shell so the tmux session stayed
alive — an observable window where "the process is done but the backend is still alive"
(the old `ready` state). That fallback shell is gone: the wrapper now writes `{id}.code`
and then exits immediately once the agent command finishes, so `{id}.clean` lands right
behind it and tmux closes the session right away, with no interval where a finished
agent leaves a session sitting around still nominally "alive." A bare-shell session
(`pulpo spawn <name>` with no command) is unaffected by this — it's a genuine
interactive shell the user is typing into, not a fallback, and still only writes
`{id}.clean` when the user exits it (or the shell dies unexpectedly).

Both marker files are written directly by the wrapper shell script itself (not by the
daemon), so their presence is race-free and daemon-uptime-independent — they're read
fresh from disk every time the daemon checks, even hours after being written or across a
daemon restart. Dead-backend classification is now simply: either marker present → `done`
(`status_reason = exited`, `exit_code` from `{id}.code` when it exists) — unless an
explicit `pulpo stop` or a watchdog intervention already recorded a more specific reason
— no marker at all → `lost`. Markers are removed when a session is purged or resumed (a
resume reuses the same session id, so stale markers from a *previous* run are cleared
first) and orphaned markers (no matching session row) are swept by `pulpo cleanup`.

## Corner Cases

- **User exits a bare-shell session — not `lost`**: A `pulpo spawn <name>` with no agent
  command is a plain interactive shell; typing `exit` (or otherwise closing it) makes the
  tmux process disappear. The wrapper writes the `{id}.clean` exit marker as the last
  thing it does before the shell process ends, so this resolves to `done`
  (`status_reason = exited`) rather than `lost` — the failure mode this fixed, before
  exit markers existed at all, was exactly this case being indistinguishable from a crash.

- **Long-running session never exits**: Some sessions cycle `working` ⇄ `waiting`
  indefinitely. They only reach `done` when the command exits (the `.code` marker
  appears) or the session is ended early by the user/watchdog (`pulpo stop`, an idle
  timeout kill, a budget breaker stop).

- **Lost on daemon restart**: When the daemon starts, `working`/`waiting` sessions whose
  tmux sessions are gone are checked against their exit markers just like any other
  staleness check: with a marker present they resolve to `done` (`exited`) retroactively
  (even if the session ended while the daemon was down); with no marker they're marked
  `lost`. The user can resume a resolved session with `pulpo resume` (which
  auto-attaches). Unlike the old `ready` state, a `done` session never has a live backend
  to separately lose — the moment a session becomes `done` its backend is already gone —
  so there's no equivalent of the old special-cased eager re-check that `ready` sessions
  needed at daemon startup.

- **Done sessions never auto-purge**: A `done` session stays listed indefinitely — there
  is no TTL that removes it automatically. It only leaves `done` via an explicit
  `pulpo stop [--purge]`, `pulpo rm`/`DELETE /api/v1/sessions/:id`, or `pulpo cleanup`.
  Since a `done` session's backend is already gone the moment it becomes `done` (no more
  lingering fallback shell that could itself
  later die and need re-classifying), there's nothing further for it to transition to on
  its own — unlike the old `ready` state, it can't quietly become `lost` out from under
  you while it sits there.
