# Session Lifecycle

Complete reference for Pulpo session states, transitions, and detection mechanisms.

## State Machine

```
  spawn           agent working        agent exits / watchdog detects
    │                   │                         │
    ▼                   ▼                         ▼
┌────────┐       ┌──────────┐              ┌──────────┐
│CREATING│──────▶│  ACTIVE  │─────────────▶│  READY   │
└────────┘       └──────────┘              └──────────┘
                   ▲      │                      │
            output │      │ waiting for          │ TTL expired
           changed │      │ input / idle         ▼
                   │      ▼                ┌──────────┐
                   │ ┌──────────┐          │ STOPPED  │
                   └─│   IDLE   │          └──────────┘
                     └──────────┘                ▲
                                                 │
                                          watchdog / user
                   ┌──────────┐
                   │   LOST   │◀── tmux disappeared (crash/reboot)
                   └──────────┘
```

## States

| State | Meaning | Terminal? |
|-------|---------|-----------|
| **Creating** | tmux session is being set up | No |
| **Active** | Agent is working — terminal output is changing | No |
| **Idle** | Agent needs attention — waiting for input or at its prompt | No |
| **Ready** | Agent process exited — task is done (exit code recorded); fallback shell still alive | Yes (resumable) |
| **Stopped** | Session ended intentionally: `pulpo stop`, watchdog intervention, TTL cleanup, or the user exited the session's shell (exit markers present) | Yes (resumable) |
| **Lost** | tmux process disappeared with no exit markers — crash, reboot, or external kill mid-run | Yes (resumable) |

## Transitions

### Creating → Active
- **Trigger**: Session creation succeeds.
- **Detection**: After the backend creates the session successfully, Pulpo marks it Active immediately. Separate liveness checks handle later Active/Idle → Lost transitions.

### Active → Idle
- **Trigger**: Watchdog detects output unchanged — either via known waiting patterns (immediate) or sustained unchanged output (configurable, default 60 seconds).
- **Detection**: The watchdog compares `output_snapshot` on each tick. Two paths to Idle:
  1. **Pattern match (immediate)**: If output is unchanged and the last 5 lines match known waiting patterns (permission prompts, "what's next?" prompts), transition happens on the first unchanged tick.
  2. **Sustained silence (universal)**: If `last_output_at` exceeds `idle_threshold_secs` (default: 60, configurable in `[watchdog]`), transition happens regardless of terminal content. Per-session override via `idle_threshold_secs` on the session (`0` = never idle).

### Idle → Active
- **Trigger**: Watchdog detects output changed since last tick.
- **Detection**: New output in the terminal means the agent (or user) resumed work.

### Active/Idle → Ready
- **Trigger**: The wrapper's `{id}.code` exit-marker file appears (written the moment the
  agent command finishes, containing its exit code), or — fallback for sessions without a
  wrapper, e.g. adopted external tmux sessions — the `[pulpo] Agent exited` text in
  captured output.
- **Detection**: The watchdog checks the marker (deterministic) before any output
  scraping or idle logic. The agent's exit code is persisted to the session. The fallback
  shell keeps the tmux session alive for inspection.
- **Side effects**: SSE event emitted.

### Active/Idle → Stopped
- **Trigger**: User runs `pulpo stop`, a watchdog intervention (memory/budget/burn/idle
  kill) — or the session's shell exits normally (the user typed `exit`, or closed tmux
  after the agent finished).
- **Detection**: Explicit stop and interventions act directly. The clean-shell-exit case
  is classified by the exit markers the command wrapper writes under `{data_dir}/exit/`:
  `{id}.code` (agent finished, exit code inside) and `{id}.clean` (shell ended normally).
  When the tmux session is gone and either marker exists, the session resolves to
  **Stopped** (exit code persisted) instead of Lost. Markers are removed on purge and
  swept by `pulpo cleanup`.

### Active/Idle → Lost
- **Trigger**: `is_alive()` returns false for a session that was Active or Idle **and no
  exit marker exists** — the tmux process died without the wrapper running to completion
  (crash, reboot, `tmux kill-session`/`kill-server` mid-run).
- **Detection**: On `get_session` or `list_sessions`, if the backend (tmux) session is
  gone the markers are consulted; with none present the session is marked Lost. A
  5-second grace period protects freshly spawned sessions from false positives. Adopted
  external sessions (no wrapper, no markers) always resolve to Lost.

### Ready → Stopped
- **Trigger**: `ready_ttl_secs` expires (if configured > 0).
- **Detection**: Watchdog checks `updated_at` of Ready sessions against the TTL on each tick. After expiry, stops the tmux shell and marks Stopped.

## Resume Semantics

| From State | Resume? | What happens |
|-----------|---------|--------------|
| **Lost** | Yes | Recreates tmux session, re-executes the session command |
| **Ready** | Yes | Re-executes the command in the tmux session (or recreates if gone) |
| **Stopped** | Yes | Recreates tmux session, re-executes the session command |
| **Active/Idle** | No | Error: session is still running |
| **Creating** | No | Error: session is still running |

## Waiting Patterns (Idle Detection)

The watchdog inspects the last 5 lines of terminal output for these patterns (case-insensitive). 29 built-in patterns cover major coding agents and common CLI prompts:

- **Generic**: `(y/n)`, `[Y/n]`, `[yes/no]`, `(yes/no)`, `Yes / No`, `Do you trust`, `Press Enter`, `approve this`, `Are you sure`, `Continue?`, `Confirm?`, `Proceed?`
- **Claude Code**: `(Y)es`, `(N)o`, `(A)lways`, `Do you want to proceed`
- **Codex CLI**: `Allow command?`
- **Gemini CLI**: `Allow?`, `Approve?`
- **Aider**: `to the chat?`, `Apply edit?`, `shell command?`, `Create new file`
- **Amazon Q**: `Allow this action?`, `Accept suggestion?`
- **SSH/sudo**: `continue connecting (yes/no)`, `'s password:`, `[sudo] password`

Add custom patterns via `waiting_patterns` in `[watchdog]` config — they are appended to the built-in list.

## Ocean Visual Mapping

| State | Color | Sprite | Behavior |
|-------|-------|--------|----------|
| Active | Lavender | active-swim | Full swimming animation |
| Idle | Amber/Gold | idle-idle | Minimal movement, small radius |
| Ready | Emerald | ready-idle | Stationary |
| Stopped | Red | stopped-idle | Stationary (same sprite as Lost, recolored) |
| Lost | Red | lost-idle | Stationary (same sprite as Stopped, recolored) |

## Configuration

### Watchdog (in `config.toml`)

```toml
[watchdog]
enabled = true
check_interval_secs = 10     # How often to check
idle_timeout_secs = 600       # Seconds before idle action triggers
idle_action = "alert"         # "alert" (mark idle_since) or "kill"
idle_threshold_secs = 60      # Seconds of unchanged output before Active→Idle (default: 60)
ready_ttl_secs = 0            # Seconds after Ready before tmux is stopped (0 = disabled)
memory_threshold = 90         # Memory % to trigger intervention
breach_count = 3              # Consecutive breaches before stop
waiting_patterns = []         # Extra patterns for waiting-for-input detection
```

### Notification Events

Webhook endpoints filter the universal event stream by `<type>.<subtype>` globs (empty
means all). Session state changes are `lifecycle` events (`lifecycle.ready`,
`lifecycle.stopped`, `lifecycle.lost`, ...):

```toml
[[webhooks]]
name = "primary"
url = "https://example.com/hooks/pulpo"
events = ["lifecycle.ready", "lifecycle.stopped", "lifecycle.lost"]
```

See the [config reference](/reference/config#webhooks) for `min_severity` and the full
event catalogue. The legacy `[[notifications.webhooks]]` form still works.

## Corner Cases

- **Agent exits but `exec bash` keeps tmux alive**: This is intentional. The `[pulpo] Agent exited (session: <name>). Run: pulpo resume <name>` marker distinguishes "agent done" from "shell still running". The Ready state reflects the agent's completion while keeping the tmux shell accessible for inspection.

- **Long-running session never exits**: Some sessions cycle Active ⇄ Idle indefinitely. They become Ready only when the command exits (causing `[pulpo] Agent exited`), or Stopped by user/watchdog.

- **Lost on daemon restart**: When the daemon starts, all Active and Idle sessions whose tmux sessions are gone are marked Lost. The user can resume them with `pulpo resume` (which auto-attaches).

- **Ready + TTL → Stopped**: When `ready_ttl_secs > 0`, ready sessions are automatically cleaned up after the grace period. This prevents tmux shell accumulation. The status changes from Ready to Stopped, blocking further resume.
