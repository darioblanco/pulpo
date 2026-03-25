<p align="center">
  <img src="web/public/logo.png" alt="Pulpo" width="128" height="128" />
</p>
<h1 align="center">Pulpo</h1>

<p align="center">
  <strong>Run coding agents on your servers. Check from your phone.</strong><br />
  Session lifecycle, watchdog supervision, multi-node fleet — for Claude Code, Codex, Aider, and any CLI tool.
</p>

<p align="center">
  <a href="https://github.com/darioblanco/pulpo/actions/workflows/ci.yml"><img src="https://github.com/darioblanco/pulpo/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/darioblanco/pulpo/releases"><img src="https://img.shields.io/github/v/release/darioblanco/pulpo?display_name=tag" alt="Latest Release"></a>
  <a href="https://hub.docker.com/r/darioblanco/pulpo-agents"><img src="https://img.shields.io/docker/pulls/darioblanco/pulpo-agents" alt="Docker Hub"></a>
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

Download binaries from [GitHub Releases](https://github.com/darioblanco/pulpo/releases). Windows uses Docker runtime (no tmux required).
</details>

## Quick Start

```bash
# Spawn an agent — pulpod starts automatically
pulpo spawn my-api --workdir ~/repos/my-api -- claude -p "Fix failing auth tests"

# Check status
pulpo ls

# Open the dashboard (installable as PWA on your phone)
pulpo ui
```

```
ID        NAME          STATUS    BRANCH                    COMMAND
a1b2c3d4  my-api [PR]   idle      fix-auth +42/-7 ↑1        claude -p "Fix failing auth tests"
```

## Why Pulpo

You have coding agents. You want them to run on your servers while you go to dinner.

Today that means: SSH in, start tmux, launch the agent, hope nothing crashes. If it does, you lose the session. If you want to check from your phone, you can't. If you want multiple agents on the same repo, they step on each other.

Pulpo fixes all of that:

```bash
# Parallel agents on the same repo — each gets its own worktree
pulpo spawn frontend --workdir ~/repo --worktree -- claude -p "redesign sidebar"
pulpo spawn backend  --workdir ~/repo --worktree -- codex "optimize queries"

# Spawn on a remote machine by name
pulpo --node mac-mini spawn review -- claude -p "security audit"

# Schedule nightly runs
pulpo schedule add nightly "0 3 * * *" --workdir ~/repo -- claude -p "review code"

# Run in Docker (safe for --dangerously-skip-permissions)
pulpo spawn risky --runtime docker -- claude --dangerously-skip-permissions -p "refactor"
```

## Features

- **Session lifecycle** — 6 states (`active`, `idle`, `ready`, `stopped`, `lost`) with resume. Agents survive reboots.
- **Watchdog** — memory pressure intervention, idle detection (31+ patterns), error detection, git tracking (branch, diff stats, commits ahead), token usage, rate limit alerts.
- **Multi-node fleet** — Tailscale, mDNS, or seed discovery. Fleet dashboard shows all sessions across all nodes.
- **Git worktrees** — `--worktree` isolates each agent. `--worktree-base main` forks from a specific branch. Stop preserves the worktree for resume.
- **Scheduler** — cron-based schedules with node targeting. `pulpo schedule add nightly "0 3 * * *" -- claude -p "review"`.
- **Docker runtime** — `--runtime docker` for sandboxed execution.
- **Adopts existing tmux** — start tmux however you want, pulpo discovers and manages it.
- **Command-agnostic** — Claude Code, Codex, Gemini CLI, Aider, OpenCode, shell scripts, anything.
- **6 interfaces** — CLI, web UI (PWA + push notifications), REST API, SSE events, MCP server, Discord bot.
- **Smart notifications** — "agent finished — created PR with +200 lines touching auth on branch fix-auth" via Discord, web push, or webhooks.
- **Self-hosted** — your machines, your data. MIT/Apache-2.0.

## How It Works

1. You start a command as a **session**
2. `pulpod` runs it on a **runtime** (tmux or Docker)
3. The **watchdog** tracks lifecycle, git state, errors, and resource usage
4. You control it from CLI, web UI, or API — from anywhere

### Comparison

| | Pulpo | agent-deck | cmux | NTM |
|---|---|---|---|---|
| Multi-node fleet | Yes | No | No | No |
| Session lifecycle + resume | 6 states | TUI only | No | Status only |
| Watchdog (memory, idle, errors) | Yes | No | No | No |
| Git tracking (branch, diff, ahead) | Yes | No | No | No |
| Worktrees | Any agent | Yes | Claude only | No |
| Scheduling | Built-in cron | No | No | No |
| Docker runtime | Yes | Yes | No | No |
| Adopts external tmux | Yes | No | No | No |
| Command-agnostic | Any command | Generic | Claude only | 3 agents |
| Web UI + mobile PWA | Yes | Web | No | Dashboard |

<h3 align="center">
  <a href="https://pulpo.darioblanco.com/getting-started/quickstart">Quickstart</a>
  <span> · </span>
  <a href="https://pulpo.darioblanco.com">Documentation</a>
  <span> · </span>
  <a href="CONTRIBUTING.md">Contributing</a>
</h3>

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
