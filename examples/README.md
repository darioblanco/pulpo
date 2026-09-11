# Examples

Runnable examples for common Pulpo workflows.

## What is Pulpo?

Pulpo is the **self-hosted meter and breaker box for coding agents**. It runs your agent
sessions as durable background workers in tmux, on machines you own — with exact usage
metering (tokens/cost read from the agent's own session files), budget enforcement, and
monitoring that forwards to your own observability stack.

**The problem**: a few coding agents running in parallel can burn a weekly subscription
allowance in an afternoon, and no vendor `/usage` page aggregates spend across your
accounts or machines. Pulpo fills that gap — see [Why Pulpo](https://pulpo.darioblanco.com/getting-started/why-pulpo)
for the full case, and [Alternatives And Comparisons](https://pulpo.darioblanco.com/getting-started/alternatives)
for how it compares to cost readers (ccusage), native multi-agent UX tools (Conductor,
Claude Code Remote Control), and hosted agent clouds.

**What makes Pulpo unique**:

- **Exact usage metering** — structured readers for Claude Code, Codex, and pi; cross-account/cross-agent rollups; `[rates.<model>]` for new or repriced models
- **Cost control** — per-session/schedule budget caps (alert 80%, stop 100%) plus a burn-velocity governor
- **Hook-driven supervision** — Claude Code, Codex, and pi report real lifecycle events (`needs input (<reason>)`, real resume) instead of scrollback guessing; any other command falls back to watchdog heuristics (idle, memory, error, completion)
- **Monitoring backbone** — signed events to any number of webhooks (durable outbox, HMAC) plus an opt-in Prometheus `/metrics` endpoint
- **Command-agnostic** — runs Claude Code, Codex, Gemini CLI, Aider, shell scripts, anything
- **4 control surfaces** — CLI, web UI (PWA with push notifications), REST API, SSE

## Layout

- `config/` — sample `config.toml` files
- `api/` — `curl` scripts for the REST API
- `cli/` — `pulpo` CLI workflow examples

## Quick Start

```bash
# From repo root — start the daemon
make dev

# In another terminal, run an example
bash examples/cli/01-basic-spawn.sh
```

Most scripts use these environment variables:

- `URL` / `PULPOD_URL` (default: `localhost:7433` / `http://localhost:7433`)
- `PULPOD_TOKEN` (optional for `local`/`tailscale` bind mode, required for `public`)

## Example Index

### CLI Workflows

| Example | Description |
|---------|-------------|
| `cli/01-basic-spawn.sh` | Spawn a session, check status, view logs |
| `cli/02-spawn-and-detach.sh` | Spawn without attaching (for scripts/CI) |
| `cli/04-idle-threshold.sh` | Per-session idle control (never idle, custom threshold) |
| `cli/05-attach-and-input.sh` | Attach to a running session, send input |
| `cli/06-recovery.sh` | Resume lost/ready/stopped sessions after crash, reboot, or stop |
| `cli/09-scheduled-sessions.sh` | Cron-based recurring agent runs |
| `cli/10-batch-spawn.sh` | Spawn multiple sessions in parallel |

### API Examples

| Example | Description |
|---------|-------------|
| `api/health.sh` | Health check |
| `api/spawn.sh` | Create a session via REST API |
| `api/events.sh` | Stream SSE events |

### Config Examples

| Example | Description |
|---------|-------------|
| `config/minimal.toml` | Zero-config local setup |
| `config/watchdog.toml` | Watchdog tuning (idle, memory, patterns) |
| `config/public-with-auth.toml` | Expose pulpod on the network with a bearer token |

For more workflows (worktrees, handoff, private-infra credentials, webhooks), see the
[docs site](https://pulpo.darioblanco.com) and
[Examples in the docs](https://pulpo.darioblanco.com/getting-started/quickstart).
