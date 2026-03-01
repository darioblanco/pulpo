#!/usr/bin/env bash
set -euo pipefail

NODE="${NODE:-localhost:7433}"
NAME="${NAME:-my-api}"

echo "Checking session state"
pulpo --node "${NODE}" list

echo "Attempting resume (works only for stale sessions)"
pulpo --node "${NODE}" resume "${NAME}"

echo "Recent logs"
pulpo --node "${NODE}" logs "${NAME}" --lines 80
