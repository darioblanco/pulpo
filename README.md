<p align="center">
  <img src="web/public/logo.png" alt="Pulpo" width="128" height="128" />
</p>
<h1 align="center">Pulpo</h1>

<p align="center">
  <strong>The self-hosted control plane for background coding agents.</strong><br />
  Run Claude Code, Codex, Gemini CLI, Aider, and any terminal agent on your own machines with durable sessions, watchdog supervision, and remote control.
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
# Spawn an agent on infrastructure you control
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

## Why This Exists

Coding agents are turning into background workers.

That creates an infrastructure problem:

- your laptop is a bad place for long-running agent work
- SSH + tmux is not a control plane
- when a machine reboots, the session state should not disappear
- when an agent is waiting, stuck, finished, or lost, you should know without attaching
- when multiple agents work on one repo, they should not step on each other

Pulpo is built for that gap. It runs agents on your own machines, keeps session
state durable, supervises execution, and gives you CLI, API, and phone-friendly
web control over the same underlying sessions.

## What Pulpo Does

Pulpo treats every agent run as a managed session:

1. You start a command as a session
2. `pulpod` runs it on a runtime backend (`tmux` or `docker`)
3. Pulpo tracks lifecycle, output, git state, and intervention history
4. You inspect, resume, stop, schedule, or redirect it from anywhere

That model works for Claude Code, Codex, Gemini CLI, Aider, shell scripts, and
other terminal tools.

```bash
# Parallel agents on the same repo - each gets its own worktree
pulpo spawn frontend --workdir ~/repo --worktree -- claude -p "redesign sidebar"
pulpo spawn backend  --workdir ~/repo --worktree -- codex "optimize queries"

# Spawn on a remote machine by name
pulpo --node mac-mini spawn review -- claude -p "security audit"

# Schedule nightly runs on the right machine
pulpo schedule add nightly "0 3 * * *" --workdir ~/repo -- claude -p "review code"

# Run in Docker when the task needs stronger isolation
pulpo spawn risky --runtime docker -- claude --dangerously-skip-permissions -p "refactor"
```

## Who It Is For

Pulpo is for developers and small teams who:

- want coding agents to run on servers or always-on machines, not laptops
- need private-network access, self-hosting, or vendor independence
- use more than one agent tool and do not want to standardize on one vendor
- care about recovery, auditability, and remote supervision

## Why Pulpo Instead Of The Alternatives

Hosted agent products are improving quickly, but they optimize for vendor-owned
cloud workflows.

Pulpo is the opposite bet:

```bash
# your machines
# your network access
# your sessions
# your policies
# your choice of agent
```

It is strongest when you need:

- self-hosted execution (sovereign by architecture, not by contract)
- remote supervision across multiple machines
- explicit recovery semantics after failure or reboot
- worktree isolation for parallel agent work
- Docker isolation for higher-risk sessions
- command-agnostic support instead of one vendor workflow

## Core Capabilities

- **Durable sessions**: explicit lifecycle states (`creating`, `active`, `idle`, `ready`, `stopped`, `lost`) with resume and stored output.
- **Watchdog supervision**: idle detection, memory-pressure intervention, ready cleanup, error patterns, token tracking, and git telemetry.
- **Multi-node fleet control**: Tailscale discovery + manual peer config. Manage sessions across machines from one dashboard or CLI.
- **Execution isolation**: use worktrees for parallel repo work and Docker for sandboxed runs.
- **Operational surfaces**: CLI, web UI/PWA, REST API, SSE, MCP server, Discord bot, and notifications.
- **Command-agnostic execution**: Claude Code, Codex, Gemini CLI, Aider, shell scripts, and other terminal commands.

## How It Works

The product contract is simple:

1. A session is a first-class object with durable state
2. A runtime backend executes that session on infrastructure you control
3. Lifecycle transitions are explicit and inspectable
4. Recovery and intervention behavior are daemon-owned, not ad hoc shell state

Everything else builds on top of that core.

## Comparison

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

Hosted coding agents are a different category. They generally win on managed
cloud convenience. Pulpo is for cases where the runtime itself needs to live on
infrastructure you control.

<h3 align="center">
  <a href="https://pulpo.darioblanco.com/getting-started/quickstart">Quickstart</a>
  <span> · </span>
  <a href="https://pulpo.darioblanco.com">Documentation</a>
  <span> · </span>
  <a href="POSITIONING.md">Positioning Memo</a>
  <span> · </span>
  <a href="CONTRIBUTING.md">Contributing</a>
</h3>

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
