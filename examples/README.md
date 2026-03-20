# Examples

Runnable examples for common Pulpo workflows.

## What is Pulpo?

Pulpo is an **agent session runtime**. It runs coding agents in tmux sessions or Docker containers, with lifecycle management, crash recovery, multi-node operations, and watchdog supervision — designed for coding agents but flexible enough for any terminal work.

**The problem**: You have machines (Mac Mini, Linux server, cloud VM) connected via Tailscale. You want to spawn agents, check on them from your phone, and not lose work when machines reboot. Today that means SSH → tmux attach → navigate windows — too many layers, no visibility, no recovery.

**What makes Pulpo unique**:

- **Multi-node native** — sessions span your machine fleet, not just localhost
- **Session lifecycle** — explicit states (active, idle, ready, killed, lost) with resume semantics
- **Watchdog supervision** — memory pressure, idle detection, agent exit detection, configurable policies
- **Command-agnostic** — runs Claude Code, Codex, Gemini CLI, Aider, shell scripts, anything
- **Adopts existing tmux sessions** — start tmux however you want, pulpo discovers and manages it
- **Mobile-first web UI** — PWA with push notifications, manage from your phone
- **6 control surfaces** — CLI, web UI, REST API, SSE, MCP, Discord bot

No other tool combines multi-node tmux orchestration with agent-aware lifecycle management. Tools like tmuxinator manage layouts, overmind runs Procfiles, cmux wraps Claude — Pulpo is the infrastructure layer that makes any terminal session durable, observable, and manageable across machines.

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

- `PULPOD_URL` (default: `http://localhost:7433`)
- `PULPOD_TOKEN` (optional for `local` bind mode, required for `public` bind mode)

## Example Index

### CLI Workflows

| Example | Description |
|---------|-------------|
| `cli/01-basic-spawn.sh` | Spawn a session, check status, view logs |
| `cli/02-spawn-and-detach.sh` | Spawn without attaching (for scripts/CI) |
| `cli/03-spawn-with-ink.sh` | Use ink presets for reusable commands |
| `cli/04-idle-threshold.sh` | Per-session idle control (never idle, custom threshold) |
| `cli/05-attach-and-input.sh` | Attach to a running session, send input |
| `cli/06-recovery.sh` | Resume lost/ready sessions after crash or reboot |
| `cli/07-adopt-tmux.sh` | Let pulpo discover and manage external tmux sessions |
| `cli/08-multi-node.sh` | Spawn and manage sessions across machines |
| `cli/09-scheduled-sessions.sh` | Cron-based recurring agent runs |
| `cli/10-batch-spawn.sh` | Spawn multiple sessions in parallel |
| `cli/11-docker-sandbox.sh` | Run agents in isolated Docker containers |

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
| `config/inks.toml` | Ink presets for different agent workflows |
| `config/watchdog.toml` | Watchdog tuning (idle, memory, patterns) |
| `config/multi-node-tailscale.toml` | Multi-node with Tailscale discovery |
| `config/multi-node-public.toml` | Multi-node with auth + mDNS |
| `config/sandbox.toml` | Docker runtime for isolated agent execution |
