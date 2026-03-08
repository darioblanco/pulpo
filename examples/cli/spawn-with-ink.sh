#!/usr/bin/env bash
set -euo pipefail

NODE="${NODE:-localhost:7433}"
WORKDIR="${WORKDIR:-$HOME}"
INK="${INK:-reviewer}"

pulpo --node "${NODE}" spawn \
  --ink "${INK}" \
  --workdir "${WORKDIR}" \
  "Review recent changes and provide a prioritized action list."

pulpo --node "${NODE}" list
