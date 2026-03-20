#!/usr/bin/env bash
# Docker runtime — run agents in isolated containers.
#
# The --runtime docker flag runs the session in a Docker container instead of tmux.
# The workdir is mounted at /workspace — the agent can read and write code
# but can't touch the rest of your system.
#
# This makes --dangerously-skip-permissions safe to use — the agent has
# full access inside the container but zero access to the host.
#
# Prerequisites:
# - Docker installed and running
# - Configure the Docker image in ~/.pulpo/config.toml:
#     [docker]
#     image = "my-agents-image:latest"
#   The image must have the agent tools installed (claude, codex, etc.)
set -euo pipefail

NODE="${NODE:-localhost:7433}"

echo "=== Basic Docker runtime session ==="
echo "pulpo spawn docker-task --runtime docker --workdir ~/repos/my-api -- claude --dangerously-skip-permissions -p 'refactor the auth module'"
echo ""

echo "=== Docker runtime + worktree (maximum isolation) ==="
echo "pulpo spawn isolated --runtime docker --worktree --workdir ~/repos/my-api -- claude --dangerously-skip-permissions -p 'rewrite tests'"
echo ""

echo "=== Docker runtime on a remote node ==="
echo "pulpo --node gpu-box spawn ml-task --runtime docker -- python train.py"
echo ""

echo "=== Docker runtime with auto node selection ==="
echo "pulpo spawn heavy-task --runtime docker --auto -- codex 'optimize database queries'"
echo ""

echo "=== Check Docker runtime sessions ==="
echo "pulpo list"
echo "# Docker runtime sessions show 'docker:pulpo-<name>' as backend ID"
echo ""

echo "=== View Docker runtime output ==="
echo "pulpo logs docker-task --follow"
echo "# Output comes from 'docker logs' instead of 'tmux capture-pane'"
echo ""

echo "=== Kill Docker runtime session (removes the container) ==="
echo "pulpo kill docker-task"
echo "# Runs 'docker stop' + 'docker rm' under the hood"
