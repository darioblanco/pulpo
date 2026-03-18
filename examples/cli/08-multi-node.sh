#!/usr/bin/env bash
# Multi-node operations — spawn and manage sessions across machines.
#
# Pulpo nodes discover each other via Tailscale, mDNS, or seed peers.
# The --node flag targets a specific machine.
set -euo pipefail

# Your machines (replace with your actual addresses)
MAC_MINI="mac-mini:7433"      # Tailscale hostname
LINUX="linux-server:7433"
GPU="gpu-box:7433"

# For public bind mode, set the token
TOKEN="${TOKEN:-}"

# Spawn on different machines
pulpo --node "${MAC_MINI}" spawn api-tests \
  --workdir ~/repos/my-api \
  --detach \
  -- claude -p "Fix failing API tests"

pulpo --node "${LINUX}" spawn security-scan \
  --workdir ~/repos/my-api \
  --detach \
  -- claude -p "Run a security audit"

pulpo --node "${GPU}" spawn ml-training \
  --workdir ~/repos/ml-project \
  --idle-threshold 0 \
  --detach \
  -- python train.py --epochs 100

# Check status across nodes
echo "=== Mac Mini ==="
pulpo --node "${MAC_MINI}" list

echo "=== Linux Server ==="
pulpo --node "${LINUX}" list

echo "=== GPU Box ==="
pulpo --node "${GPU}" list

# The web UI at any node shows peers and their sessions.
# open "http://${MAC_MINI}"

# Stream events from any node
# curl -N "http://${MAC_MINI}/api/v1/events"
