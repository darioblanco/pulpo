#!/usr/bin/env bash
#
# Pulpo demo — a recordable, narrated walkthrough of the meter / breaker / monitor story.
#
# This is the storyboard for the launch demo. It types and runs *real* pulpo commands so
# you can capture a terminal cast instead of editing a video. Record it with asciinema:
#
#   asciinema rec pulpo-demo.cast -c "./scripts/demo.sh"
#   # then publish, or render to SVG/GIF:
#   #   npx svg-term-cli --in pulpo-demo.cast --out pulpo-demo.svg --window
#   #   agg pulpo-demo.cast pulpo-demo.gif        # https://github.com/asciinema/agg
#
# Prerequisites for a real recording: pulpo installed, an agent on PATH (e.g. `claude`),
# and a repo to point at. Set DEMO_REPO and DEMO_AGENT below. Steps pause for <Enter> so
# you control pacing; set DEMO_AUTO=1 to play through with fixed delays instead.
#
# Nothing here is destructive: it spawns demo-prefixed sessions and stops them at the end.

set -euo pipefail

DEMO_REPO="${DEMO_REPO:-$HOME/repos/your-project}"
DEMO_AGENT="${DEMO_AGENT:-claude}"
DEMO_AUTO="${DEMO_AUTO:-0}"
TYPE_DELAY="${TYPE_DELAY:-0.03}"

bold() { printf '\033[1m%s\033[0m\n' "$1"; }
dim() { printf '\033[2m# %s\033[0m\n' "$1"; }

pause() {
  if [[ "$DEMO_AUTO" == "1" ]]; then sleep "${1:-2}"; else read -r -p $'\033[2m  <Enter>\033[0m'; fi
}

# Type a command out like a human, then run it.
run() {
  printf '\033[32m$\033[0m '
  local i
  for ((i = 0; i < ${#1}; i++)); do
    printf '%s' "${1:i:1}"
    sleep "$TYPE_DELAY"
  done
  printf '\n'
  eval "$1"
}

clear
bold "Pulpo — the self-hosted meter and breaker box for coding agents"
dim "Run any terminal agent on infra you own. See exactly what it costs. Pull the plug before the wall."
pause

bold $'\n1) Run an agent as a durable, attributable session'
dim "tmux-backed, survives reboots, tracked from spawn"
run "pulpo spawn demo-fix --workdir $DEMO_REPO -- $DEMO_AGENT -p 'fix the failing auth tests'"
pause

run "pulpo ls"
pause

bold $'\n2) Meter — exactly, across agents and accounts'
dim "tokens + cost read from the agent's own session files, not scraped; live burn rate"
run "pulpo usage"
pause

bold $'\n3) Breaker — a hard budget that intervenes'
dim "alert at 80%, stop at 100% — recorded as an intervention. On subscriptions this also"
dim "protects the shared pool; on prepaid credits / API keys it protects real dollars."
run "pulpo spawn demo-review --budget-cost 5 --workdir $DEMO_REPO -- $DEMO_AGENT -p 'review the diff'"
pause

bold $'\n4) Monitor — forward every event to your own stack'
dim "signed canonical events to your webhooks (durable outbox + backoff) + a Prometheus"
dim "/metrics endpoint (opt-in). Pulpo is the event plane; your Grafana / Datadog / Slack"
dim "is the dashboard. Add to ~/.pulpo/config.toml:"
run "printf '%s\n' '[[webhooks]]' 'url = \"https://collector.example.com/pulpo\"' 'secret = \"…\"' 'events = [\"usage_alert.*\", \"intervention.*\", \"lifecycle.lost\"]' 'min_severity = \"warn\"' '' '[metrics]' 'enabled = true'"
pause

bold $'\n5) The gauge on your phone'
dim "single binary serves an installable PWA; reach it over your tailnet with bind = tailscale"
run "pulpo ui"
pause

bold $'\nCleanup'
run "pulpo stop demo-fix demo-review --purge"

bold $'\nSelf-hosted. Your machines, your accounts, your budgets, your observability.'
dim "brew install darioblanco/tap/pulpo  ·  https://pulpo.darioblanco.com"
