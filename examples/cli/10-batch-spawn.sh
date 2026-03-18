#!/usr/bin/env bash
# Batch spawn — run multiple agents in parallel across repos.
#
# Spawn detached sessions and monitor them all.
set -euo pipefail

NODE="${NODE:-localhost:7433}"

echo "=== Spawning agents across repos ==="

# Review multiple repos in parallel
for repo in my-api my-frontend my-infra; do
  pulpo --node "${NODE}" spawn "${repo}-review" \
    --workdir ~/repos/${repo} \
    --description "Nightly review of ${repo}" \
    --detach \
    -- claude -p "Review the codebase. Focus on security and performance."
  echo "Spawned ${repo}-review"
done

echo ""
echo "=== All sessions ==="
pulpo --node "${NODE}" list

echo ""
echo "=== Monitor all output ==="
echo "Open multiple terminals:"
for repo in my-api my-frontend my-infra; do
  echo "  pulpo logs ${repo}-review --follow"
done

echo ""
echo "=== Stream events (all sessions) ==="
echo "  curl -N http://${NODE}/api/v1/events"

echo ""
echo "=== Kill all when done ==="
echo "  for repo in my-api my-frontend my-infra; do"
echo "    pulpo kill \${repo}-review"
echo "  done"
