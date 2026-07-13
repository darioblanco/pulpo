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

echo "==> lifecycle: user exit resolves to stopped (not lost)"
LIFE="e2e-lifecycle-$$"
FAST="$TMP/fast-agent.sh"
printf 'echo LIFECYCLE_AGENT_DONE
' >"$FAST"
"$PULPO" --node "$NODE" spawn "$LIFE" -d --workdir /tmp -- sh "$FAST" || fail "lifecycle spawn failed"
# wait for the agent to finish (the wrapper then lingers in a fallback shell)
for _ in $(seq 1 60); do
  "$PULPO" --node "$NODE" logs "$LIFE" 2>/dev/null | grep -q LIFECYCLE_AGENT_DONE && break
  sleep 0.25
done
# user types `exit` in the lingering shell → tmux ends cleanly
tmux send-keys -t "pulpo-$LIFE" "exit" Enter 2>/dev/null || tmux send-keys -t "$LIFE" "exit" Enter   || fail "could not send exit keys to tmux"
stopped=0
for _ in $(seq 1 80); do
  status=$("$PULPO" --node "$NODE" list --all 2>/dev/null | grep "$LIFE" | tr '[:upper:]' '[:lower:]')
  case "$status" in
    *stopped*) stopped=1; break ;;
    *lost*) fail "lifecycle regression: user exit classified as LOST" ;;
  esac
  sleep 0.25
done
[ "$stopped" -eq 1 ] || fail "session did not resolve to stopped after user exit"
"$PULPO" --node "$NODE" stop "$LIFE" --purge >/dev/null 2>&1 || true

echo "E2E SMOKE PASSED"
