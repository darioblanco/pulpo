# Examples

Runnable examples for common Pulpo workflows.

## What is Pulpo?

Pulpo is an **agent session runtime**. It runs coding agents in tmux sessions or Docker containers, with lifecycle management, crash recovery, and watchdog supervision — designed for coding agents but flexible enough for any terminal work.

**The problem**: You have a machine (Mac Mini, Linux server, cloud VM), optionally reachable over Tailscale. You want to spawn agents, check on them from your phone, and not lose work when the machine reboots. Today that means SSH → tmux attach → navigate windows — too many layers, no visibility, no recovery.

**What makes Pulpo unique**:

- **Session lifecycle** — explicit states (active, idle, ready, killed, lost) with resume semantics
- **Watchdog supervision** — memory pressure, idle detection, agent exit detection, configurable policies
- **Command-agnostic** — runs Claude Code, Codex, Gemini CLI, Aider, shell scripts, anything
- **Mobile-first web UI** — PWA with push notifications, manage from your phone
- **4 control surfaces** — CLI, web UI, REST API, SSE

No other tool combines tmux orchestration with agent-aware lifecycle management. Tools like tmuxinator manage layouts, overmind runs Procfiles, cmux wraps Claude — Pulpo is the infrastructure layer that makes any terminal session durable, observable, and manageable.

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
| `cli/04-idle-threshold.sh` | Per-session idle control (never idle, custom threshold) |
| `cli/05-attach-and-input.sh` | Attach to a running session, send input |
| `cli/06-recovery.sh` | Resume lost/ready sessions after crash or reboot |
| `cli/09-scheduled-sessions.sh` | Cron-based recurring agent runs |
| `cli/10-batch-spawn.sh` | Spawn multiple sessions in parallel |
| `cli/11-docker-runtime.sh` | Run agents in isolated Docker containers |

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
| `config/docker.toml` | Docker runtime for isolated agent execution |
