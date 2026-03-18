#!/usr/bin/env bash
# Spawn using an ink preset — reusable command templates from config.
#
# Inks are defined in ~/.pulpo/config.toml:
#   [inks.reviewer]
#   description = "Code reviewer"
#   command = "claude -p 'Review this code for issues.'"
#
# The ink's command is used unless you override it with -- <command>.
set -euo pipefail

NODE="${NODE:-localhost:7433}"

# Use the "reviewer" ink preset
pulpo --node "${NODE}" spawn auth-review \
  --workdir ~/repos/my-api \
  --ink reviewer \
  --detach

# Use an ink but override its command
pulpo --node "${NODE}" spawn custom-review \
  --workdir ~/repos/my-api \
  --ink reviewer \
  --detach \
  -- claude -p "Focus only on the auth module"

# Add a description for context
pulpo --node "${NODE}" spawn security-audit \
  --workdir ~/repos/my-api \
  --ink reviewer \
  --description "Weekly security audit of auth endpoints" \
  --detach

pulpo --node "${NODE}" list
