#!/usr/bin/env bash
# Attach to a running session and send input.
#
# When an agent is waiting for input (Idle state), you can:
# 1. Attach to the tmux session and type directly
# 2. Send text remotely via `pulpo input`
set -euo pipefail

NODE="${NODE:-localhost:7433}"
NAME="${NAME:-my-api}"

# Attach to the session's terminal (opens tmux)
# Detach with Ctrl-b d
pulpo --node "${NODE}" attach "${NAME}"

# Or send input without attaching (useful from phone/remote):

# Send "yes" to approve a prompt
pulpo --node "${NODE}" input "${NAME}" "yes"

# Send Enter (empty input = Enter key)
pulpo --node "${NODE}" input "${NAME}"

# Send a multi-word response
pulpo --node "${NODE}" input "${NAME}" "Apply all suggested changes"

# Check if the agent resumed after your input
pulpo --node "${NODE}" logs "${NAME}" --lines 20
