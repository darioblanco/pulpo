#!/usr/bin/env bash
# Spawn without attaching — useful for scripts, CI, and batch operations.
# The -d / --detach flag skips auto-attach.
set -euo pipefail

NODE="${NODE:-localhost:7433}"

# Spawn detached — returns immediately
pulpo --node "${NODE}" spawn lint-check \
  --workdir ~/repos/my-api \
  --detach \
  -- npm run lint

echo "Session spawned. Monitor with:"
echo "  pulpo logs lint-check --follow"
echo "  pulpo list"

# You can also spawn with just a path (auto-generates name from directory):
# pulpo ~/repos/my-api
# This creates a session named "my-api" and auto-attaches.
