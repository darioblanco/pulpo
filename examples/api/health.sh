#!/usr/bin/env bash
set -euo pipefail

PULPOD_URL="${PULPOD_URL:-http://localhost:7433}"

echo "GET ${PULPOD_URL}/api/v1/health"
curl -fsS "${PULPOD_URL}/api/v1/health"
echo
