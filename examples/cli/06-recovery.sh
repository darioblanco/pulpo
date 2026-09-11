#!/usr/bin/env bash
# Recovery workflows — resume sessions after crashes, reboots, or agent exits.
#
# Session states and resumability:
#   Active/Idle/Creating → still running, nothing to resume
#   Ready                → agent exited normally, can resume to re-run
#   Lost                 → tmux disappeared (crash/reboot), can resume
#   Stopped              → terminated by user/watchdog/TTL, or exited cleanly — can resume
set -euo pipefail

URL="${URL:-localhost:7433}"

# 1. Check what happened
echo "=== Current session states ==="
pulpo --url "${URL}" list

# 2. Resume a lost session (re-executes the original command)
# pulpo --url "${URL}" resume my-api
# This auto-attaches. Use Ctrl-b d to detach.

# 3. Resume a ready session (agent finished, re-run the task)
# pulpo --url "${URL}" resume auth-review

# 4. Check intervention history (why was it stopped?)
# pulpo --url "${URL}" interventions my-api
# Shows: memory_pressure, idle_timeout, budget_exceeded, burn_rate
# (a plain `pulpo stop` is not an intervention and never shows up here)

# 5. After daemon restart, pulpod auto-resumes active sessions.
#    Check the daemon logs for "Auto-resumed N session(s)".
#    Sessions that couldn't be auto-resumed become "lost".

# 6. Stop and re-spawn if you need a fresh start
# pulpo --url "${URL}" stop my-api            # alias: kill
# pulpo --url "${URL}" stop my-api --purge    # also remove from history
# pulpo --url "${URL}" spawn my-api --workdir ~/repos/my-api -- claude -p "Start over"
