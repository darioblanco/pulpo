#!/usr/bin/env bash
# Recovery workflows — resume sessions after crashes, reboots, or agent exits.
#
# Session states and resumability:
#   Active/Idle → still running, nothing to resume
#   Ready       → agent exited normally, can resume to re-run
#   Lost        → tmux disappeared (crash/reboot), can resume
#   Killed      → terminated by user/watchdog, cannot resume
set -euo pipefail

NODE="${NODE:-localhost:7433}"

# 1. Check what happened
echo "=== Current session states ==="
pulpo --node "${NODE}" list

# 2. Resume a lost session (re-executes the original command)
# pulpo --node "${NODE}" resume my-api
# This auto-attaches. Use Ctrl-b d to detach.

# 3. Resume a ready session (agent finished, re-run the task)
# pulpo --node "${NODE}" resume auth-review

# 4. Check intervention history (why was it killed?)
# pulpo --node "${NODE}" interventions my-api
# Shows: memory_pressure, idle_timeout, user_kill

# 5. After daemon restart, pulpod auto-resumes active sessions.
#    Check the daemon logs for "Auto-resumed N session(s)".
#    Sessions that couldn't be auto-resumed become "lost".

# 6. Kill and re-spawn if you need a fresh start
# pulpo --node "${NODE}" kill my-api
# pulpo --node "${NODE}" delete my-api   # remove from history
# pulpo --node "${NODE}" spawn my-api --workdir ~/repos/my-api -- claude -p "Start over"
