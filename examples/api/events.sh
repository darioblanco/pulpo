#!/usr/bin/env bash
set -euo pipefail

PULPOD_URL="${PULPOD_URL:-http://localhost:7433}"
PULPOD_TOKEN="${PULPOD_TOKEN:-}"

URL="${PULPOD_URL}/api/v1/events"
if [[ -n "${PULPOD_TOKEN}" ]]; then
  URL="${URL}?token=${PULPOD_TOKEN}"
fi

echo "Streaming SSE from: ${URL}"
echo "Press Ctrl+C to stop."
curl -N -fsS "${URL}"
