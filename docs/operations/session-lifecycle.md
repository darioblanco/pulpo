# Session Lifecycle

Complete reference for Pulpo session states, transitions, and detection mechanisms.

## State Machine

```
  spawn           agent working        agent exits / watchdog detects
    │                   │                         │
    ▼                   ▼                         ▼
┌────────┐       ┌──────────┐              ┌──────────┐
│CREATING│──────▶│  ACTIVE  │─────────────▶│ FINISHED │
└────────┘       └──────────┘              └──────────┘
                   ▲      │                      │
            output │      │ waiting for          │ TTL expired
           changed │      │ input / idle         ▼
                   │      ▼                ┌──────────┐
                   │ ┌──────────┐          │  KILLED  │
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
| **Finished** | Agent process exited — task is done | Yes (resumable) |
| **Killed** | Session was terminated by user, watchdog, or TTL cleanup | Yes (not resumable) |
| **Lost** | tmux process disappeared unexpectedly (crash, reboot) | Yes (resumable) |

## Transitions

### Creating → Active
- **Trigger**: Backend reports tmux session is alive after spawn.

### Active → Idle
- **Trigger**: Watchdog detects output unchanged — either via known waiting patterns (immediate) or sustained unchanged output (20+ seconds).
- **Detection**: The watchdog compares `output_snapshot` on each tick. Two paths to Idle:
  1. **Pattern match (immediate)**: If output is unchanged and the last 5 lines match known waiting patterns (permission prompts, "what's next?" prompts), transition happens on the first unchanged tick.
  2. **Sustained silence (universal)**: If `last_output_at` is more than 20 seconds ago, transition happens regardless of terminal content. This catches all providers without needing provider-specific patterns.

### Idle → Active
- **Trigger**: Watchdog detects output changed since last tick.
- **Detection**: New output in the terminal means the agent (or user) resumed work.

### Active/Idle → Finished
- **Trigger**: Watchdog detects `[pulpo] Agent exited` marker in captured output.
- **Detection**: Every agent command is wrapped with `echo '[pulpo] Agent exited'; exec bash`. The watchdog checks for this marker before any idle logic. The `exec bash` keeps the tmux shell alive for inspection.
- **Side effects**: SSE event emitted, culture harvested (pending files from the session).

### Active/Idle → Killed
- **Trigger**: User runs `pulpo kill`, watchdog memory intervention, or watchdog idle timeout with `action: kill`.
- **Detection**: Explicit kill command or watchdog policy.

### Active → Lost
- **Trigger**: `is_alive()` returns false for a session that was Active.
- **Detection**: On `get_session`, if the backend session is gone, the session is marked Lost.

### Finished → Killed
- **Trigger**: `finished_ttl_secs` expires (if configured > 0).
- **Detection**: Watchdog checks `updated_at` of Finished sessions against the TTL on each tick. After expiry, kills the tmux shell and marks Killed.

## Resume Semantics

| From State | Resume? | What happens |
|-----------|---------|--------------|
| **Lost** | Yes | Recreates tmux session, restarts agent with `--resume <conversation-id>` |
| **Finished** | Yes | Restarts agent in the tmux session (or recreates if gone) |
| **Killed** | No | Error: "session cannot be resumed" |
| **Active/Idle** | No | Error: session is still running |
| **Creating** | No | Error: session is still running |

## Waiting Patterns (Idle Detection)

The watchdog inspects the last 5 lines of terminal output for these patterns (case-insensitive):

- `Do you trust`
- `Yes / No`
- `(y/n)`, `[Y/n]`, `[yes/no]`, `(yes/no)`, `? [Y/n]`, `? (y/N)`
- `Press Enter`
- `approve this`

These cover permission prompts from Claude Code, Codex, and other agents.

## Mode × Guard Matrix

The `unrestricted` setting is a **guard toggle**, not a mode. It passes through to agent CLI flags (e.g., `--dangerously-skip-permissions` for Claude Code). Pulpo does not enforce permissions — the agent binary does. Pulpo only observes terminal output.

| Mode | Unrestricted | Behavior |
|------|-------------|----------|
| **Interactive** | false | Cycles Active ⇄ Idle. Idle fires on permission prompts AND "what's next?" prompts. |
| **Interactive** | true | Cycles Active ⇄ Idle. Idle fires only on "what's next?" prompts (no permission prompts). |
| **Autonomous** | false | Active → Idle → Active → ... → Finished. May hit Idle on permission prompts. |
| **Autonomous** | true | Active → Finished. Agent runs without stopping. |
| **Shell** | N/A | Cycles Active ⇄ Idle based on whether a command is running in bash. |

## Ocean Visual Mapping

| State | Color | Sprite | Behavior |
|-------|-------|--------|----------|
| Active | Lavender | active-swim | Full swimming animation |
| Idle | Amber/Gold | idle-idle | Minimal movement, small radius |
| Finished | Emerald | finished-idle | Stationary |
| Killed | Red | killed-idle | Stationary (same sprite as Lost, recolored) |
| Lost | Red | lost-idle | Stationary (same sprite as Killed, recolored) |

## Configuration

### Watchdog (in `config.toml`)

```toml
[watchdog]
enabled = true
check_interval_secs = 10     # How often to check
idle_timeout_secs = 600       # Seconds before idle action triggers
idle_action = "alert"         # "alert" (mark idle_since) or "kill"
finished_ttl_secs = 0         # Seconds after Finished before tmux is killed (0 = disabled)
memory_threshold = 90         # Memory % to trigger intervention
breach_count = 3              # Consecutive breaches before kill
```

### Notification Events

Default notification events: `["finished", "killed"]`. Configure via:

```toml
[notifications.discord]
webhook_url = "https://discord.com/api/webhooks/..."
events = ["finished", "killed", "lost"]
```

## Corner Cases

- **Agent exits but `exec bash` keeps tmux alive**: This is intentional. The `[pulpo] Agent exited` marker distinguishes "agent done" from "shell still running". The Finished state reflects the agent's completion while keeping the tmux shell accessible for inspection.

- **Interactive session never finishes**: Interactive sessions cycle Active ⇄ Idle indefinitely. They become Finished only when the user exits the agent (causing `[pulpo] Agent exited`), or Killed by user/watchdog.

- **Lost on daemon restart**: When the daemon starts, all Active sessions whose tmux sessions are gone are marked Lost. The user can resume them.

- **Culture extraction timing**: Culture is harvested (pending files) on Finished and Killed transitions. For Finished, the watchdog triggers it. For Killed, the SessionManager triggers it.

- **Finished + TTL → Killed**: When `finished_ttl_secs > 0`, finished sessions are automatically cleaned up after the grace period. This prevents tmux shell accumulation. The status changes from Finished to Killed, blocking further resume.
