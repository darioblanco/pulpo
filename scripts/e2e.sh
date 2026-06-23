#!/usr/bin/env bash
#
# Isolated end-to-end smoke test for Pulpo.
#
# Boots a real `pulpod` on a temporary config/port/data-dir (never touches ~/.pulpo),
# spawns a trivial agent, verifies the session runs and its output is captured, checks
# the usage endpoint, then stops it. Catches packaging / PATH / wiring / daemon-startup
# breakage that no unit or integration test sees.
#
# Requires: tmux, a built workspace (the script builds debug binaries), curl.
# Run with: make e2e   (or: bash scripts/e2e.sh)

set -euo pipefail

PORT="${PULPO_E2E_PORT:-7599}"
TMP="$(mktemp -d)"
DATA_DIR="$TMP/data"
CONFIG="$TMP/config.toml"
NODE="localhost:$PORT"
SESSION="pulpo-e2e-smoke"
PULPOD_PID=""

cleanup() {
  [ -n "$PULPOD_PID" ] && kill "$PULPOD_PID" 2>/dev/null || true
  tmux kill-session -t "$SESSION" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT

mkdir -p "$DATA_DIR"
cat >"$CONFIG" <<TOML
[node]
port = $PORT
data_dir = "$DATA_DIR"
bind = "local"

# Stay isolated: don't adopt the developer's other tmux sessions into this throwaway daemon.
[watchdog]
adopt_tmux = false
TOML

echo "==> building pulpod + pulpo (debug)"
cargo build --quiet --bin pulpod --bin pulpo

PULPOD="./target/debug/pulpod"
PULPO="./target/debug/pulpo"

echo "==> starting pulpod on $NODE (isolated data: $DATA_DIR)"
"$PULPOD" --config "$CONFIG" >"$TMP/pulpod.log" 2>&1 &
PULPOD_PID=$!

fail() {
  echo "E2E FAILED: $1" >&2
  echo "--- pulpod.log ---" >&2
  cat "$TMP/pulpod.log" >&2 || true
  exit 1
}

echo "==> waiting for health"
for i in $(seq 1 60); do
  curl -fsS "http://$NODE/api/v1/health" >/dev/null 2>&1 && break
  sleep 0.25
  [ "$i" -eq 60 ] && fail "pulpod did not become healthy"
done

# `pulpo spawn -- <args>` joins trailing args with spaces, so pass the agent as a
# script file rather than `sh -c '…'` (whose quoting wouldn't survive the join).
AGENT="$TMP/agent.sh"
printf 'echo PULPO_E2E_OK\nsleep 30\n' >"$AGENT"

echo "==> spawning session (detached — no TTY in a smoke run)"
"$PULPO" --node "$NODE" spawn "$SESSION" -d --workdir /tmp -- sh "$AGENT" || fail "spawn failed"

echo "==> waiting for captured output"
ok=0
for _ in $(seq 1 60); do
  if "$PULPO" --node "$NODE" logs "$SESSION" 2>/dev/null | grep -q PULPO_E2E_OK; then
    ok=1
    break
  fi
  sleep 0.25
done
[ "$ok" -eq 1 ] || fail "session output not captured"

echo "==> usage endpoint"
"$PULPO" --node "$NODE" usage >/dev/null || fail "usage failed"

echo "==> stopping session"
"$PULPO" --node "$NODE" stop "$SESSION" --purge || fail "stop failed"

echo "E2E SMOKE PASSED"
