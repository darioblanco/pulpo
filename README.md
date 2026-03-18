<p align="center">
  <img src="web/public/logo.png" alt="Pulpo" width="128" height="128" />
</p>
<h1 align="center">Pulpo</h1>

<p align="center">
  <strong>Agent session runtime. Durable sessions across your machines.</strong><br />
  tmux or Docker sandbox, multi-node fleet, watchdog supervision — managed from your phone.
</p>

<p align="center">
  <a href="https://github.com/darioblanco/pulpo/actions/workflows/ci.yml"><img src="https://github.com/darioblanco/pulpo/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/darioblanco/pulpo/actions/workflows/docker-images.yml"><img src="https://github.com/darioblanco/pulpo/actions/workflows/docker-images.yml/badge.svg" alt="Docker Images"></a>
  <a href="https://github.com/darioblanco/pulpo/releases"><img src="https://img.shields.io/github/v/release/darioblanco/pulpo?display_name=tag" alt="Latest Release"></a>
  <a href="https://github.com/darioblanco/pulpo/releases"><img src="https://img.shields.io/github/release-date/darioblanco/pulpo" alt="Release Date"></a>
  <a href="https://hub.docker.com/r/darioblanco/pulpo-base"><img src="https://img.shields.io/docker/pulls/darioblanco/pulpo-base" alt="Docker Hub: pulpo-base"></a>
  <a href="https://hub.docker.com/r/darioblanco/pulpo-agents"><img src="https://img.shields.io/docker/pulls/darioblanco/pulpo-agents" alt="Docker Hub: pulpo-agents"></a>
  <a href="https://hub.docker.com/r/darioblanco/pulpo-discord-bot"><img src="https://img.shields.io/docker/pulls/darioblanco/pulpo-discord-bot" alt="Docker Hub: pulpo-discord-bot"></a>
  <a href="https://github.com/darioblanco/pulpo#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="License: MIT OR Apache-2.0"></a>
</p>

> **Experimental** — Pulpo is in early development. APIs, config format, and behavior may change between releases.

## The Problem

You have agents — Claude Code, Codex, Aider, Gemini CLI — and you want them to run on your servers while you go to dinner. Today that means SSH into a machine, start tmux, launch the agent, and hope nothing crashes. If it does, you lose the session. If you want to check from your phone, you can't. If you want multiple agents on the same repo, they step on each other.

## What Pulpo Does

Pulpo is an **agent session runtime** — it runs coding agents in tmux sessions or Docker containers, with lifecycle management, crash recovery, and watchdog supervision. Designed for coding agents, flexible enough for any terminal work.

```bash
# Spawn an agent on a remote machine by name
pulpo --node mac-mini spawn auth-fix --workdir ~/repos/api -- claude -p "fix auth tests"

# Spawn two agents on the same repo without conflicts
pulpo spawn frontend --workdir ~/repo --worktree -- claude -p "redesign UI"
pulpo spawn backend  --workdir ~/repo --worktree -- codex "optimize queries"

# Schedule nightly reviews on the beefy server
pulpo schedule add nightly "0 3 * * *" --node gpu-box -- claude -p "review code"

# Auto-select the least loaded machine
pulpo spawn review --auto -- claude -p "security audit"

# Run in a Docker sandbox (safe for --dangerously-skip-permissions)
pulpo spawn risky-task --sandbox -- claude --dangerously-skip-permissions -p "refactor everything"

# Check from your phone
open http://localhost:7433  # PWA with push notifications
```

### Key Features

- **Session lifecycle** — explicit states (`active`, `idle`, `ready`, `killed`, `lost`) with resume semantics. Agents survive reboots.
- **Multi-node fleet** — spawn and manage sessions across machines. Tailscale, mDNS, or seed discovery. Fleet dashboard shows all sessions across all nodes.
- **Watchdog supervision** — memory pressure kills, idle detection (31 built-in patterns for Claude Code, Codex, Gemini, Aider, Amazon Q), configurable per-session thresholds.
- **Git worktrees** — `--worktree` isolates each agent in its own worktree. Multiple agents work on the same repo without conflicts. Works with any agent.
- **Built-in scheduler** — cron-based schedules with multi-node targeting. Run nightly reviews on the beefy server, auto-select least loaded node.
- **Docker sandbox** — `--sandbox` runs sessions in Docker containers. Safe for `--dangerously-skip-permissions` — the agent can't touch your host. Configure the image in `[sandbox]` config.
- **Adopts existing tmux** — start tmux however you want, pulpo discovers it and brings it under management. No migration needed.
- **Command-agnostic** — Claude Code, Codex, Gemini CLI, Aider, shell scripts, anything. Same lifecycle, same controls.
- **6 control surfaces** — CLI, web UI (PWA + push notifications), REST API, SSE events, MCP server, Discord bot.
- **Self-hosted** — your machines, your data. MIT/Apache-2.0 licensed.

### How It's Different

| | Pulpo | tmuxinator | cmux | agent-deck | NTM |
|---|---|---|---|---|---|
| Multi-node | Fleet with discovery | No | No | No | No |
| Session lifecycle | 6 states + resume | No | No | TUI only | Status only |
| Watchdog | Memory + idle + patterns | No | No | No | No |
| Worktrees | Any agent | No | Claude only | Yes | No |
| Scheduling | Built-in cron + node targeting | No | No | No | No |
| Docker sandbox | Yes | No | No | Yes | No |
| Adopts external tmux | Yes | No | No | No | No |
| Command-agnostic | Any command | N/A | Claude only | Generic | 3 agents |
| Web UI + mobile | PWA + push | No | No | TUI + Web | Dashboard |

## Get Started

```bash
# Install (macOS/Linux)
brew install darioblanco/tap/pulpo

# Start daemon
pulpod

# Spawn a session
pulpo spawn my-api --workdir ~/repos/my-api -- claude -p "Fix failing auth tests"

# Watch progress
pulpo logs my-api --follow

# Open web UI
open http://localhost:7433
```

No agent is required — `pulpo spawn my-shell` opens a managed shell session.

<h3 align="center">
  <a href="https://pulpo.darioblanco.com/getting-started/install">Install</a>
  <span> · </span>
  <a href="https://pulpo.darioblanco.com/getting-started/quickstart">Quickstart</a>
  <span> · </span>
  <a href="https://pulpo.darioblanco.com">Documentation</a>
  <span> · </span>
  <a href="https://github.com/darioblanco/pulpo/tree/main/examples">Examples</a>
  <span> · </span>
  <a href="CONTRIBUTING.md">Contributing</a>
</h3>

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
