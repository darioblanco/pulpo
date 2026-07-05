<p align="center">
  <img src="web/public/logo.png" alt="Pulpo" width="128" height="128" />
</p>
<h1 align="center">Pulpo</h1>

<p align="center">
  <strong>The self-hosted meter and breaker box for coding agents.</strong><br />
  See — and control — what every coding agent costs, across all your machines and accounts.
  Run Claude Code, Codex, Gemini CLI, Aider, or any terminal agent on infrastructure you own,
  with exact usage metering, budget enforcement, and monitoring that forwards to your own stack.
</p>

<p align="center">
  <a href="https://github.com/darioblanco/pulpo/actions/workflows/ci.yml"><img src="https://github.com/darioblanco/pulpo/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/darioblanco/pulpo/releases"><img src="https://img.shields.io/github/v/release/darioblanco/pulpo?display_name=tag" alt="Latest Release"></a>
  <a href="https://github.com/darioblanco/pulpo#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="License"></a>
</p>

## Install

```bash
brew install darioblanco/tap/pulpo
```

That's it. The daemon auto-starts when you run your first command.

<details>
<summary>Linux / manual install</summary>

```bash
# Linux (systemd)
curl -fsSL https://github.com/darioblanco/pulpo/releases/latest/download/pulpod-x86_64-unknown-linux-gnu.tar.xz | tar xJ
sudo mv pulpod pulpo /usr/local/bin/
pulpod  # or: make service-install-linux
```

For a one-liner install or upgrade that works on macOS and Linux, run the script that auto-detects your OS/arch:

```bash
curl -fsSL https://raw.githubusercontent.com/darioblanco/pulpo/main/scripts/install-pulpo.sh | bash
```

Set `BIN_DIR` or `TARGET` when invoking the script if you need a different install directory or target triple. Re-running the script always pulls the latest release, so it doubles as the update command.

Download binaries from [GitHub Releases](https://github.com/darioblanco/pulpo/releases).
</details>

## Quick Start

```bash
# Run an agent as a durable session on infrastructure you control
pulpo spawn my-api --workdir ~/repos/my-api -- claude -p "Fix failing auth tests"

# See what every agent is costing — across accounts, machines, and agents
pulpo usage

# Put a hard budget on a run: alert at 80%, stop at 100%
pulpo spawn nightly-review --budget-cost 5 -- claude -p "review today's diff"

# Open the dashboard (installable as a PWA on your phone)
pulpo ui
```

```
SESSION          SOURCE   TOKENS     COST      $/HR   QUOTA
my-api           claude     1.2M    $2.41   $2.41/h     ~3%
nightly-review   claude     310K    $0.74   $0.74/h     ~1%
```

## Why This Exists

Coding agents have become background workers — and a **quota-and-cost multiplier**.
Run a few in parallel and a weekly subscription allowance can vanish in an afternoon.

The tools that could tell you what's happening won't:

- A vendor's `/usage` is **one account, one machine, one vendor**, and you have to go look at it.
  No vendor will ever aggregate spend across *your* accounts — that would help you arbitrage
  their own rate limits.
- Vendors tell you that you *overspent*. Only the thing actually running the session can
  **prevent** it — stop a runaway, alert before the wall, refuse to start over budget.
- Your code and your usage data are exactly what you'd least want flowing through a third-party
  relay.

Pulpo fills that gap. It runs your agents as durable sessions on machines you own, reads
**exact token counts** from each agent's own session files (and costs them from your rate
table), enforces budgets, and forwards alerts and events to whatever observability stack
*you* run. Sovereign by architecture: usage and account data never leave your infrastructure.

## What Pulpo Does

**Meter — exactly, everywhere.** Pulpo parses the session files Claude Code and Codex write
themselves, so **token counts are exact** (not scraped) and costed from your rate table —
attributed per session and rolled up per account, billing pool, **ink, and repo** ("the
nightly-review ink cost €11"), across every machine and agent you run. (Codex reports exact
subscription quota rather than a per-token cost.) Unknown
models still report tokens; `[rates.<model>]` config prices a new or repriced model with no
code change.

```bash
pulpo usage --scan              # zero-setup: scan ALL local Claude + Codex + pi history →
                                # total spend by agent and repo (no sessions routed through pulpo)
pulpo usage                     # live burn rate ($/hr, tokens/hr), time-to-cap, quota
```

`pulpo usage --scan` is the fastest way in: it reads the agents' *own* session files and
shows what every agent has cost you, **unified across Claude, Codex, and pi, broken down
by agent, model, and repo** — the unified view a single-vendor `/usage` page can't give,
plus the budgets and enforcement a read-only tool can't add. (pi sessions carry the exact dollar cost the agent recorded itself.) Nothing has to run through Pulpo first. It's **worktree-aware**: a repo's git
worktrees and subdirectories roll up to the origin repo, so per-repo spend means *this repo*
and not *this checkout* (add `--by-worktree` to keep each checkout separate). Narrow to a
window with `--since <days>`, or pipe the raw numbers somewhere with `--json`.

**Control — pull the plug before the wall.** Per-session and per-ink cost caps that alert at
80% and stop at 100%, plus a burn-velocity governor that catches the catastrophic 2 a.m.
runaway a flat budget misses. Alert-only by default; opt in to auto-stop.

```bash
pulpo spawn fix --budget-cost 10 -- claude -p "..."   # hard $10 cap, recorded as an intervention
```

**Monitor — forward to your own stack.** Every lifecycle change, intervention, and usage/cost
alert becomes a signed canonical event delivered to any number of `[[webhooks]]` (durable
outbox, exponential backoff, HMAC; receivers dedupe on a stable event id), plus an optional
Prometheus `/metrics` endpoint. Pulpo is the event plane; your Grafana / Datadog / SIEM /
Slack is the dashboard.

**Run — durable and unattended.** Each agent runs in a `tmux` session with explicit lifecycle
states that survive reboots, a watchdog for idle / memory / error / completion detection, and
per-session git worktrees so parallel agents on one repo never collide.

That model works for Claude Code, Codex, Gemini CLI, Aider, shell scripts, and any other
terminal command — Pulpo is not tied to one vendor or one model.

## Sovereign & Self-Hosted

```bash
# your machines      — runs where you put it, no vendor cloud
# your accounts      — usage + identity read from local files, never relayed
# your budgets       — enforcement runs in the session, not after the invoice
# your observability — events forwarded to your collector, not a SaaS
# your choice of agent
```

Reach a node's dashboard or API from your phone over your private network with
**Tailscale transport** (`bind = "tailscale"` → HTTPS via `tailscale serve`, zero setup,
no ports exposed to the public internet).

## Who It Is For

Developers and teams who:

- run coding agents on servers or always-on machines and want to know what they cost
- run more than one agent, account, or machine and want **one** gauge for all of them
- need budgets and alerts that actually intervene, not a post-hoc invoice
- require self-hosting, private-network access, and vendor independence

## Multi-machine

Pulpo is **single-node-first**. Each node meters and governs its own sessions independently —
no central server required, and nothing breaks if you only ever run one machine.

There is deliberately no control plane joining machines together. Reach any node directly —
`pulpo --node <name|host:port>` from the CLI, a saved connection in the web UI, or SSH/tmux
— see [Control Your Agents From Anywhere](docs/guides/remote-control.md). For a view across
machines, point every node's **event forwarding** (`[[webhooks]]` + `/metrics`) at a
collector you already run, and aggregate there. This is the supported cross-node story: it
adds no single point of failure and integrates with your existing observability.

## Core Capabilities

- **Exact usage metering**: structured readers for Claude Code, Codex & pi (tokens, cost, cache, quota; pi in `--scan` only for now), cross-account / cross-agent rollups, `[rates.<model>]` config, output-scraping fallback for other agents.
- **Cost control**: per-session / per-ink budget caps (alert 80%, stop 100%) and a burn-velocity ($/hr) governor — alert-first, opt-in stop.
- **Monitoring backbone**: signed canonical events to multiple webhooks with a durable outbox + backoff; toggleable Prometheus `/metrics`; SSE stream; web push.
- **Durable sessions**: explicit lifecycle (`creating`, `active`, `idle`, `ready`, `stopped`, `lost`) with resume and stored output; survives reboots; adopts external tmux sessions.
- **Watchdog supervision**: idle detection, memory-pressure intervention, ready cleanup, error/completion patterns, git telemetry (branch, diff; PR URL detected from output).
- **Execution isolation**: per-session git worktrees for parallel work on one repo.
- **Sovereign access**: single binary with embedded web UI/PWA, CLI, REST API; Tailscale transport for private remote access.
- **Command-agnostic**: any terminal agent or command.

## How It Works

```text
command → session → tmux backend → lifecycle → metering + control + events
```

1. You start a command as a managed **session**.
2. `pulpod` runs it on a `tmux` backend on a machine you control.
3. The watchdog drives lifecycle, reads exact usage, enforces budgets, and emits events.
4. You inspect, meter, budget, and supervise it from the CLI, API, web UI, or your own stack.

The daemon owns the truth; every surface reflects or operates on the same sessions.

## Comparison

|  | Pulpo | vendor `/usage` | ccusage |
|---|---|---|---|
| Cross-account, cross-machine cost | Yes | One account / machine | Single machine |
| Cross-agent (Claude + Codex + …) | Yes | One vendor | Yes (many CLIs) |
| Live burn rate + projection | Yes | No | Post-hoc |
| Budget enforcement (auto-stop) | Yes | No | No |
| Alerts before the wall | Yes | No | No |
| Forward events to your stack | Webhooks + `/metrics` | No | No |
| Self-hosted, data stays local | Yes | n/a | Yes |
| Runs the sessions | Yes | n/a | No (reads logs) |

ccusage proves the demand for the gauge; it's read-only and single-machine because it doesn't
run your sessions. Vendor dashboards show one account after the fact. Pulpo is live,
cross-everything, and — because it runs the sessions — it can also pull the plug.

<h3 align="center">
  <a href="https://pulpo.darioblanco.com/getting-started/quickstart">Quickstart</a>
  <span> · </span>
  <a href="https://pulpo.darioblanco.com">Documentation</a>
  <span> · </span>
  <a href="ROADMAP.md">Roadmap</a>
  <span> · </span>
  <a href="CONTRIBUTING.md">Contributing</a>
</h3>

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
