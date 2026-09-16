#!/usr/bin/env bash
# Recovery workflows — resume sessions after crashes, reboots, or agent exits.
#
# Session states and resumability (five-state model — see docs/operations/session-lifecycle.md):
#   Starting/Working/Waiting → still running, nothing to resume
#   Done                     → agent exited (clean or via `pulpo stop`/a watchdog
#                               intervention) — always resumable
#   Lost                     → tmux disappeared (crash/reboot) with no exit marker —
#                               resumable
set -euo pipefail

URL="${URL:-localhost:7433}"

# 1. Check what happened (--all also shows `done` sessions, hidden by default)
echo "=== Current session states ==="
pulpo --url "${URL}" list --all

# 2. Resume a lost session (replays the harness's own resume command, e.g.
#    `claude --resume <id>`, or the original command for a generic one)
# pulpo --url "${URL}" resume my-api
# This auto-attaches. Use Ctrl-b d to detach.

# 3. Resume a done session (agent finished, was stopped, or a watchdog
#    intervention ended it — resume always recreates the backend and reruns/continues)
# pulpo --url "${URL}" resume auth-review

# 4. Check intervention history (why was it stopped?)
# pulpo --url "${URL}" interventions my-api
# Shows: idle_timeout, budget_exceeded
# (a plain `pulpo stop` is not an intervention and never shows up here)

# 5. After a daemon restart, pulpod re-checks every previously running session:
#    - tmux still alive              → stays working/waiting
#    - tmux gone, exit marker present → resolves to done (status_reason = exited),
#      even if the agent finished while the daemon was down
#    - tmux gone, no exit marker      → marked lost
#    Resume a `done` or `lost` session the same way (step 2/3 above).

# 6. Stop and re-spawn if you need a fresh start
# pulpo --url "${URL}" stop my-api            # alias: kill
# pulpo --url "${URL}" stop my-api --purge    # also remove from history
# pulpo --url "${URL}" spawn my-api --workdir ~/repos/my-api -- claude -p "Start over"
