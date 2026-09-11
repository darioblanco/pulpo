# Recovery Guide

::: tip Core Behavior
Recovery is part of Pulpo's core runtime contract. If you want to understand what Pulpo guarantees, this guide matters more than optional layers like Tailscale bind or webhook notifications.
:::

## Session States

| State | Meaning | Terminal? |
|-------|---------|-----------|
| **Creating** | tmux session is being set up | No |
| **Active** | Agent is working — terminal output is changing | No |
| **Idle** | Agent needs attention — waiting for input or at its prompt | No |
| **Ready** | Agent process exited — task is done | Yes (resumable) |
| **Stopped** | Session was terminated by user, watchdog, or TTL cleanup | Yes (resumable) |
| **Lost** | tmux process disappeared unexpectedly (crash, reboot) | Yes (resumable) |

## Common Recovery Path

```bash
pulpo list
# my-api   lost   ...

pulpo resume my-api
```

`resume` auto-attaches to the tmux session after restarting the agent. Detach with `Ctrl-b d`.

It works for **lost** (tmux gone after crash/reboot), **ready** (agent exited normally),
and **stopped** (terminated by user, watchdog, or TTL cleanup) sessions. The session
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
- `memory_pressure` — system memory exceeded the configured threshold
- `idle_timeout` — session was idle longer than allowed (when `idle_action = "kill"`)
- `budget_exceeded` — the session's `--budget-cost` cap was reached
- `burn_rate` — the burn-velocity governor's ceiling was crossed with `burn_action = "stop"`

A plain `pulpo stop` (or the API's stop endpoint) is **not** an intervention and is never
recorded here — interventions are only the watchdog's own forced stops. `ready_ttl_secs`
cleanup is likewise a separate, unrecorded path: it stops the tmux shell of a long-idle
`Ready` session directly and does not appear in `pulpo interventions`.
