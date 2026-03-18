#!/usr/bin/env bash
# Per-session idle threshold — control when sessions transition to Idle.
#
# By default, sessions go Active→Idle after 60s of unchanged output.
# Override per session with --idle-threshold:
#   0     = never go idle (useful for long-thinking agents)
#   120   = 2 minutes of silence before idle
#   None  = use the global default from config
set -euo pipefail

NODE="${NODE:-localhost:7433}"

# Agent that thinks for long periods — never mark as idle
pulpo --node "${NODE}" spawn deep-thinker \
  --workdir ~/repos/complex-project \
  --idle-threshold 0 \
  --detach \
  -- claude -p "Analyze the entire codebase and write a comprehensive architecture doc"

# Quick task — go idle quickly so you know it's waiting
pulpo --node "${NODE}" spawn quick-task \
  --workdir ~/repos/my-api \
  --idle-threshold 15 \
  --detach \
  -- claude -p "Add a health check endpoint"

# Default behavior (global idle_threshold_secs from config, default 60s)
pulpo --node "${NODE}" spawn normal-task \
  --workdir ~/repos/my-api \
  --detach \
  -- claude -p "Fix the bug in the login flow"

pulpo --node "${NODE}" list
