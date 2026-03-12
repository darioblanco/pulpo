# Recovery Guide

## Session States

| State | Meaning | Terminal? |
|-------|---------|-----------|
| **Creating** | tmux session is being set up | No |
| **Active** | Agent is working — terminal output is changing | No |
| **Idle** | Agent needs attention — waiting for input or at its prompt | No |
| **Finished** | Agent process exited — task is done | Yes (resumable) |
| **Killed** | Session was terminated by user, watchdog, or TTL cleanup | Yes (not resumable) |
| **Lost** | tmux process disappeared unexpectedly (crash, reboot) | Yes (resumable) |

## Common Recovery Path

```bash
pulpo list
# my-api   lost   ...

pulpo resume my-api
pulpo logs my-api --follow
```

`resume` works for **lost** (tmux gone after crash/reboot) and **finished** (agent exited normally) sessions. The agent is restarted with `--resume` to continue from its previous conversation.

**Killed** sessions cannot be resumed — start a new session with `pulpo spawn`.

## Recovery After Daemon Restart

When `pulpod` starts, it checks all previously active sessions:
- If the tmux session is still alive → stays **active**
- If the tmux session is gone → marked **lost**

Lost sessions appear in `pulpo list` and can be resumed.

## Interventions

Inspect intervention history to understand why a session was killed:

```bash
pulpo interventions <name>
```

Common intervention reasons:
- `memory_pressure` — system memory exceeded the configured threshold
- `idle_timeout` — session was idle longer than allowed (when `idle_action = "kill"`)
- `finished_ttl` — finished session exceeded its TTL grace period

## Culture on Recovery

When a session finishes or is killed, Pulpo automatically harvests any culture entries the agent wrote to the `pending/` directory. These entries are committed to the culture repo and become available to future sessions. This means even sessions that are killed by the watchdog can still contribute learnings.
