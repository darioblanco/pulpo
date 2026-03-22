# Recovery Guide

::: tip Core Behavior
Recovery is part of Pulpo's core runtime contract. If you want to understand what Pulpo guarantees, this guide matters more than optional layers like discovery, Discord, or MCP.
:::

## Session States

| State | Meaning | Terminal? |
|-------|---------|-----------|
| **Creating** | tmux session is being set up | No |
| **Active** | Agent is working — terminal output is changing | No |
| **Idle** | Agent needs attention — waiting for input or at its prompt | No |
| **Ready** | Agent process exited — task is done | Yes (resumable) |
| **Killed** | Session was terminated by user, watchdog, or TTL cleanup | Yes (not resumable) |
| **Lost** | tmux process disappeared unexpectedly (crash, reboot) | Yes (resumable) |

## Common Recovery Path

```bash
pulpo list
# my-api   lost   ...

pulpo resume my-api
```

`resume` auto-attaches to the tmux session after restarting the agent. Detach with `Ctrl-b d`.

It works for **lost** (tmux gone after crash/reboot) and **ready** (agent exited normally) sessions. The session command is re-executed in a new tmux session.

**Killed** sessions cannot be resumed — start a new session with `pulpo spawn`.

## Recovery After Daemon Restart

When `pulpod` starts, it checks all previously active sessions:
- If the tmux session is still alive → stays **active** (backend ID upgraded to tmux `$N` ID)
- If the tmux session is gone → re-created automatically, stays **active**

If auto-resume fails, sessions are marked **lost** and appear in `pulpo list` for manual resume.

## Interventions

Inspect intervention history to understand why a session was killed:

```bash
pulpo interventions <name>
```

Common intervention reasons:
- `memory_pressure` — system memory exceeded the configured threshold
- `idle_timeout` — session was idle longer than allowed (when `idle_action = "kill"`)
- `ready_ttl` — ready session exceeded its TTL grace period
