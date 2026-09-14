# Recovery Guide

> **Core Behavior:** Recovery is part of Pulpo's core runtime contract. If you want to understand what Pulpo guarantees, this guide matters more than optional layers like Tailscale bind or webhook notifications.

## Session States

| State | Meaning | Terminal? |
|-------|---------|-----------|
| **Starting** | tmux session is being set up | No |
| **Working** | Agent is working — terminal output is changing | No |
| **Waiting** | Agent is at its prompt — `status_reason` says whether it needs input from you or is just idle | No |
| **Done** | Agent process exited and the backend is gone — task is done, or the session was stopped by the user or a watchdog intervention (`status_reason` says which) | Yes (resumable) |
| **Lost** | tmux process disappeared unexpectedly (crash, reboot), with no evidence of a clean end | Yes (resumable) |

## Common Recovery Path

```bash
pulpo list
# my-api   lost   ...

pulpo resume my-api
```

`resume` auto-attaches to the tmux session after restarting the agent. Detach with `Ctrl-b d`.

It works for **lost** (tmux gone after crash/reboot) and **done** (agent exited
normally, was stopped by the user, or ended via a watchdog intervention) sessions. The
session command is re-executed in a new tmux session — for a harness with its own
resume mechanism (Claude Code, Codex, pi), that means the harness's resume command, so
the conversation continues. A **done** session's backend is already gone by the time it
reaches that state — there's no lingering shell to reattach to — so resume always
recreates the backend and reruns the resume command rather than just flipping the
status back to `working`.

A session still **working**, **waiting**, or **starting** cannot be resumed — it's
still running. Start a fresh session with `pulpo spawn` instead.

## Recovery After Daemon Restart

When `pulpod` starts, it checks all previously running sessions:
- If the tmux session is still alive → stays **working**/**waiting** (backend ID upgraded to tmux `$N` ID)
- If the tmux session is gone → re-created automatically, stays **working**

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
