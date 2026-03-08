#!/usr/bin/env bash
set -euo pipefail

NODE="${NODE:-localhost:7433}"
WORKDIR="${WORKDIR:-$HOME}"
NAME="${NAME:-nightly-review}"

pulpo --node "${NODE}" schedule create \
  --name "${NAME}" \
  --cron "0 2 * * *" \
  --workdir "${WORKDIR}" \
  --ink reviewer \
  --concurrency skip \
  --max-executions 30 \
  "Review all changes from the last day and report regressions."

pulpo --node "${NODE}" schedule list
pulpo --node "${NODE}" schedule run "${NAME}"
pulpo --node "${NODE}" schedule history "${NAME}" --limit 10
