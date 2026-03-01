#!/usr/bin/env bash
set -euo pipefail

PULPOD_URL="${PULPOD_URL:-http://localhost:7433}"
PULPOD_TOKEN="${PULPOD_TOKEN:-}"
WORKDIR="${WORKDIR:-$HOME}"
PROMPT="${PROMPT:-Summarize this repository and propose 3 improvements.}"

AUTH_ARGS=()
if [[ -n "${PULPOD_TOKEN}" ]]; then
  AUTH_ARGS=(-H "Authorization: Bearer ${PULPOD_TOKEN}")
fi

echo "POST ${PULPOD_URL}/api/v1/sessions"
curl -fsS "${PULPOD_URL}/api/v1/sessions" \
  "${AUTH_ARGS[@]}" \
  -H "Content-Type: application/json" \
  -d "{
    \"workdir\": \"${WORKDIR}\",
    \"prompt\": \"${PROMPT}\",
    \"provider\": \"claude\",
    \"mode\": \"autonomous\"
  }"
echo
