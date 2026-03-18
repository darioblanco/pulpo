#!/usr/bin/env bash
# Adopt external tmux sessions — pulpo discovers and manages tmux sessions
# that were created outside of pulpo (e.g., via `tmux new-session`).
#
# This is one of pulpo's unique features: you don't have to use `pulpo spawn`
# to get lifecycle management. Start tmux however you want, and pulpo will
# find it, classify it, and track it.
#
# How it works:
# 1. The watchdog runs every check_interval_secs (default: 10s)
# 2. It calls `tmux list-sessions` and compares against known sessions
# 3. Unknown tmux sessions are adopted with:
#    - Name = tmux session name
#    - Status = Active (if running an agent like claude/codex) or Ready (if running bash/zsh)
#    - Command = full command line from `ps` (not just process name)
#    - Backend ID = tmux's internal $N ID
#
# Requirements:
# - adopt_tmux = true in [watchdog] config (default: true)
# - pulpod must be running
set -euo pipefail

echo "=== Step 1: Create a tmux session outside of pulpo ==="
echo "Run this in another terminal:"
echo ""
echo "  tmux new-session -s external-claude -c ~/repos/my-api 'claude -p \"review code\"'"
echo ""
echo "Or for a plain shell:"
echo "  tmux new-session -s my-shell -c ~/projects"
echo ""

echo "=== Step 2: Wait ~10 seconds for the watchdog to discover it ==="
echo ""

echo "=== Step 3: Check pulpo — the session should appear ==="
echo "  pulpo list"
echo ""
echo "Expected output:"
echo "  NAME               STATUS       COMMAND"
echo "  external-claude     active       claude -p \"review code\""
echo "  my-shell            ready        bash"
echo ""

echo "=== Step 4: Now you can manage it like any pulpo session ==="
echo "  pulpo logs external-claude --follow"
echo "  pulpo attach external-claude"
echo "  pulpo input external-claude 'yes'"
echo "  pulpo kill external-claude"
echo ""

echo "=== Corner case: Ghost fix ==="
echo "If you kill a session and then create a new tmux session with the same name,"
echo "pulpo will correctly adopt the new one. Killed sessions don't block re-adoption"
echo "because pulpo uses tmux's internal \$N IDs, not session names."
