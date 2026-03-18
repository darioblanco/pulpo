#!/usr/bin/env bash
# Spawn a session via the REST API.
set -euo pipefail

PULPOD_URL="${PULPOD_URL:-http://localhost:7433}"
PULPOD_TOKEN="${PULPOD_TOKEN:-}"
WORKDIR="${WORKDIR:-$HOME}"
COMMAND="${COMMAND:-echo 'Hello from pulpo'}"

AUTH_ARGS=()
if [[ -n "${PULPOD_TOKEN}" ]]; then
  AUTH_ARGS=(-H "Authorization: Bearer ${PULPOD_TOKEN}")
fi

echo "POST ${PULPOD_URL}/api/v1/sessions"
curl -fsS "${PULPOD_URL}/api/v1/sessions" \
  "${AUTH_ARGS[@]}" \
  -H "Content-Type: application/json" \
  -d "{
    \"name\": \"api-example\",
    \"workdir\": \"${WORKDIR}\",
    \"command\": \"${COMMAND}\",
    \"description\": \"Session created via API example\"
  }"
echo
