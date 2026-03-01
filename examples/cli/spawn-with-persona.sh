#!/usr/bin/env bash
set -euo pipefail

NODE="${NODE:-localhost:7433}"
WORKDIR="${WORKDIR:-$HOME}"
PERSONA="${PERSONA:-reviewer}"

pulpo --node "${NODE}" spawn \
  --persona "${PERSONA}" \
  --workdir "${WORKDIR}" \
  "Review recent changes and provide a prioritized action list."

pulpo --node "${NODE}" list
