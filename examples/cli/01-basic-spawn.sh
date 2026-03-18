#!/usr/bin/env bash
# Basic spawn workflow: create a session, check status, view output.
#
# By default, `pulpo spawn` auto-attaches to the tmux session.
# Detach with Ctrl-b d to return to your shell.
set -euo pipefail

NODE="${NODE:-localhost:7433}"

# Spawn a session running Claude Code in a repo
# The name is the first argument. Everything after -- is the command.
pulpo --node "${NODE}" spawn my-api \
  --workdir ~/repos/my-api \
  -- claude -p "Fix the failing auth tests"

# After detaching (Ctrl-b d), check status:
# pulpo list
# pulpo logs my-api --lines 50
# pulpo logs my-api --follow     # tail -f style
