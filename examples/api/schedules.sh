#!/usr/bin/env bash
set -euo pipefail

PULPOD_URL="${PULPOD_URL:-http://localhost:7433}"
PULPOD_TOKEN="${PULPOD_TOKEN:-}"
SCHEDULE_NAME="${SCHEDULE_NAME:-nightly-review}"
WORKDIR="${WORKDIR:-$HOME}"
CRON="${CRON:-0 2 * * *}"
PROMPT="${PROMPT:-Review the latest changes and list risks.}"

AUTH_ARGS=()
if [[ -n "${PULPOD_TOKEN}" ]]; then
  AUTH_ARGS=(-H "Authorization: Bearer ${PULPOD_TOKEN}")
fi

echo "Creating schedule: ${SCHEDULE_NAME}"
curl -sS "${PULPOD_URL}/api/v1/schedules" \
  "${AUTH_ARGS[@]}" \
  -H "Content-Type: application/json" \
  -d "{
    \"name\": \"${SCHEDULE_NAME}\",
    \"cron\": \"${CRON}\",
    \"workdir\": \"${WORKDIR}\",
    \"prompt\": \"${PROMPT}\",
    \"provider\": \"claude\",
    \"mode\": \"autonomous\",
    \"concurrency\": \"skip\"
  }"
echo

echo "Listing schedules"
curl -fsS "${PULPOD_URL}/api/v1/schedules" "${AUTH_ARGS[@]}"
echo

echo "Triggering schedule now"
curl -fsS -X POST "${PULPOD_URL}/api/v1/schedules/${SCHEDULE_NAME}/run" "${AUTH_ARGS[@]}"
echo

echo "Execution history"
curl -fsS "${PULPOD_URL}/api/v1/schedules/${SCHEDULE_NAME}/executions?limit=20" "${AUTH_ARGS[@]}"
echo
