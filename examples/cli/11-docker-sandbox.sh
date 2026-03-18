#!/usr/bin/env bash
# Docker sandbox — run agents in isolated containers.
#
# The --sandbox flag runs the session in a Docker container instead of tmux.
# The workdir is mounted at /workspace — the agent can read and write code
# but can't touch the rest of your system.
#
# This makes --dangerously-skip-permissions safe to use — the agent has
# full access inside the container but zero access to the host.
#
# Prerequisites:
# - Docker installed and running
# - Configure the sandbox image in ~/.pulpo/config.toml:
#     [sandbox]
#     image = "my-agents-image:latest"
#   The image must have the agent tools installed (claude, codex, etc.)
set -euo pipefail

NODE="${NODE:-localhost:7433}"

echo "=== Basic sandbox session ==="
echo "pulpo spawn sandbox-task --sandbox --workdir ~/repos/my-api -- claude --dangerously-skip-permissions -p 'refactor the auth module'"
echo ""

echo "=== Sandbox + worktree (maximum isolation) ==="
echo "pulpo spawn isolated --sandbox --worktree --workdir ~/repos/my-api -- claude --dangerously-skip-permissions -p 'rewrite tests'"
echo ""

echo "=== Sandbox on a remote node ==="
echo "pulpo --node gpu-box spawn ml-task --sandbox -- python train.py"
echo ""

echo "=== Sandbox with auto node selection ==="
echo "pulpo spawn heavy-task --sandbox --auto -- codex 'optimize database queries'"
echo ""

echo "=== Check sandbox sessions ==="
echo "pulpo list"
echo "# Sandbox sessions show 'docker:pulpo-<name>' as backend ID"
echo ""

echo "=== View sandbox output ==="
echo "pulpo logs sandbox-task --follow"
echo "# Output comes from 'docker logs' instead of 'tmux capture-pane'"
echo ""

echo "=== Kill sandbox session (removes the container) ==="
echo "pulpo kill sandbox-task"
echo "# Runs 'docker stop' + 'docker rm' under the hood"
