# Recovery Guide

> **Core Behavior:** Recovery is part of Pulpo's core runtime contract. If you want to understand what Pulpo guarantees, this guide matters more than optional layers like Tailscale bind or webhook notifications.

## Session States

| State | Meaning | Terminal? |
|-------|---------|-----------|
| **Creating** | tmux session is being set up | No |
| **Active** | Agent is working — terminal output is changing | No |
| **Idle** | Agent needs attention — waiting for input or at its prompt | No |
| **Ready** | Agent process exited — task is done | Yes (resumable) |
| **Stopped** | Session was terminated by user or a watchdog intervention | Yes (resumable) |
| **Lost** | tmux process disappeared unexpectedly (crash, reboot) | Yes (resumable) |

## Common Recovery Path

```bash
pulpo list
# my-api   lost   ...

pulpo resume my-api
```

`resume` auto-attaches to the tmux session after restarting the agent. Detach with `Ctrl-b d`.

It works for **lost** (tmux gone after crash/reboot), **ready** (agent exited normally),
and **stopped** (terminated by user or a watchdog intervention) sessions. The session
command is re-executed in a new tmux session — for a harness with its own resume
mechanism (Claude Code, Codex, pi), that means the harness's resume command, so the
conversation continues. A **ready** session's fallback shell is often still alive (the
tmux pane lingers after the agent exits), but resume still recreates it and reruns the
resume command rather than just flipping the status back to active — the agent process
itself has already exited, so there's nothing to "reattach" to otherwise.

A session still **active**, **idle**, or **creating** cannot be resumed — it's still
running. Start a fresh session with `pulpo spawn` instead.

## Recovery After Daemon Restart

When `pulpod` starts, it checks all previously active sessions:
- If the tmux session is still alive → stays **active** (backend ID upgraded to tmux `$N` ID)
- If the tmux session is gone → re-created automatically, stays **active**

If auto-resume fails, sessions are marked **lost** and appear in `pulpo list` for manual resume.

## Interventions

Inspect intervention history to understand why a session was stopped:

```bash
pulpo interventions <name>
```

Common intervention reasons:
- `idle_timeout` — session was idle longer than allowed (when `idle_action = "kill"`)
- `budget_exceeded` — the session's `--budget-cost` cap was reached

A historical session stopped by the burn-velocity governor before its removal
(September 2026) may still show `burn_rate` in its own stored record, but the code no
longer produces it and `pulpo interventions`/the session's `intervention_code` reads that
value back as unset rather than erroring.

A plain `pulpo stop` (or the API's stop endpoint) is **not** an intervention and is never
recorded here — interventions are only the watchdog's own forced stops.

`memory_pressure` may still appear on historical sessions from before the memory-pressure
intervention was removed — it is no longer produced by new stops.
