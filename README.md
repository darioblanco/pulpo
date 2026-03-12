<p align="center">
  <img src="web/public/logo.png" alt="Pulpo" width="128" height="128" />
</p>

<h1 align="center">Pulpo</h1>

<p align="center">
  <strong>Self-hosted control plane for coding agents across your machines.</strong>
</p>

<p align="center">
  <a href="https://github.com/darioblanco/pulpo/actions/workflows/ci.yml"><img src="https://github.com/darioblanco/pulpo/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/darioblanco/pulpo/actions/workflows/docker-images.yml"><img src="https://github.com/darioblanco/pulpo/actions/workflows/docker-images.yml/badge.svg" alt="Docker Images"></a>
  <a href="https://pulpo.darioblanco.com"><img src="https://img.shields.io/badge/docs-pulpo.darioblanco.com-blue" alt="Docs"></a>
  <a href="https://github.com/darioblanco/pulpo/releases"><img src="https://img.shields.io/github/v/release/darioblanco/pulpo?display_name=tag" alt="Latest Release"></a>
  <a href="https://github.com/darioblanco/pulpo/releases"><img src="https://img.shields.io/github/release-date/darioblanco/pulpo" alt="Release Date"></a>
  <a href="https://hub.docker.com/r/darioblanco/pulpo-base"><img src="https://img.shields.io/docker/pulls/darioblanco/pulpo-base" alt="Docker Hub: pulpo-base"></a>
  <a href="https://hub.docker.com/r/darioblanco/pulpo-agents"><img src="https://img.shields.io/docker/pulls/darioblanco/pulpo-agents" alt="Docker Hub: pulpo-agents"></a>
  <a href="https://hub.docker.com/r/darioblanco/pulpo-discord-bot"><img src="https://img.shields.io/docker/pulls/darioblanco/pulpo-discord-bot" alt="Docker Hub: pulpo-discord-bot"></a>
  <a href="https://github.com/darioblanco/pulpo#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="License: MIT OR Apache-2.0"></a>
</p>

> **Experimental** — Pulpo is in early development. APIs, config format, and behavior may change between releases. Feedback and contributions are welcome.

Pulpo runs as a daemon (`pulpod`) with a CLI (`pulpo`) and embedded web UI. It manages coding agent sessions (Claude Code, Codex, Gemini CLI, OpenCode) over `tmux`, persists lifecycle state in SQLite, and supports recovery after restarts and reboots.

## Why Pulpo

Coding agents are powerful, but running them across multiple machines is operationally painful. Pulpo is infrastructure that makes agent execution **reliable**, **observable**, and **controllable** — without replacing the agents themselves.

- **Session lifecycle** — explicit states (`active`, `idle`, `finished`, `killed`, `lost`) with resume semantics for crash recovery.
- **Cross-node operations** — manage agents on multiple machines from a single API/CLI/web surface, with Tailscale, mDNS, or seed-based discovery.
- **Watchdog interventions** — memory pressure detection, idle handling, and configurable kill policies with audit trails.
- **Provider-agnostic** — Claude Code, Codex, Gemini CLI, OpenCode, or bare shell. Same lifecycle, same controls.
- **Collective culture** — agents write learnings back to a shared AGENTS.md-based knowledge base. Culture syncs across nodes, so agents improve from each other's sessions over time.
- **Self-hosted and open source** — your machines, your data, your control.

## What Works Today

- `pulpod` daemon with REST API + embedded web UI
- `pulpo` CLI for local and remote node control
- Session lifecycle: `creating`, `active`, `idle`, `finished`, `killed`, `lost`
- Resume flow for `lost` and `finished` sessions (`pulpo resume <name>`)
- Watchdog interventions (memory pressure, idle detection, finished TTL cleanup)
- Multi-node support (Tailscale, mDNS, seed, manual peers)
- Culture system: agent write-back, cross-node sync, lifecycle pruning, deduplication
- Inks (`[inks.<name>]` in config) for reusable agent role definitions
- Schedule management via crontab wrapper (`pulpo schedule ...`)
- SSE event stream: `GET /api/v1/events`
- MCP server mode: `pulpod mcp`
- Discord integration in [`contrib/discord-bot/`](contrib/discord-bot/README.md)
- Docker images for containerized deployment

## Quickstart

### 1. Install

macOS (Homebrew):

```bash
brew install darioblanco/tap/pulpo
```

From source (Rust 1.82+, Node.js 22+, tmux 3.2+):

```bash
git clone https://github.com/darioblanco/pulpo.git
cd pulpo
make setup
make build
make install
```

### 2. Configure provider

Claude Code example:

```bash
npm install -g @anthropic-ai/claude-code
claude login
```

Codex, Gemini CLI, and OpenCode also work if installed/authenticated in your environment.

### 3. Start daemon

```bash
pulpod
```

### 4. Spawn and inspect a session

```bash
pulpo spawn --workdir ~/repos/my-api "Fix failing auth tests"
pulpo list
pulpo logs my-api --follow
```

### 5. Open UI and event stream

```bash
open http://localhost:7433
curl -N http://localhost:7433/api/v1/events
```

## Typical Recovery Flow

```bash
pulpo list
# my-api   lost   ...

pulpo resume my-api
pulpo logs my-api --follow
```

`resume` works for `lost` (tmux gone) and `finished` (agent exited) sessions. `killed` sessions require a new `spawn`.

## CLI At A Glance

```bash
pulpo spawn [OPTIONS] [PROMPT...]
pulpo list
pulpo logs <NAME> [--follow]
pulpo attach <NAME>
pulpo input <NAME> [TEXT]
pulpo kill <NAME>
pulpo resume <NAME>
pulpo interventions <NAME>
pulpo culture [--context] [--get ID] [--delete ID] [--push]
pulpo nodes
pulpo schedule <install|list|pause|resume|remove>
pulpo ui
```

For full options, run `pulpo --help` and `pulpo <command> --help`.

### Worktree isolation

When running multiple agents on the same repo, use `--worktree` to give each session its own git worktree (Claude only):

```bash
pulpo spawn --worktree --workdir ~/myproject "add caching layer"
pulpo spawn --worktree --workdir ~/myproject "refactor auth module"
```

Each agent works on an isolated branch at `<repo>/.claude/worktrees/<session-name>`. Other providers can use a worktree created by Claude via `--workdir`:

```bash
pulpo spawn --provider codex --workdir ~/myproject/.claude/worktrees/<session-name> "write tests"
```

Without `--worktree`, agents work directly in your working tree (fine for single sessions).

## Configuration

Default config path: `~/.pulpo/config.toml`

Example:

```toml
[node]
name = "mac-mini"

[inks.reviewer]
provider = "claude"
unrestricted = false
instructions = "You are a senior reviewer focused on correctness and security."
```

See [SPEC.md](SPEC.md#configuration) for all supported config sections.

## Peer Discovery

Pulpo nodes find each other automatically. The discovery method is derived from the `bind` mode in `[node]` — no separate `[discovery]` section needed.

### Tailscale (recommended)

Discovers peers across your Tailscale network and serves the dashboard over HTTPS via `tailscale serve`. Accessible at `https://<machine-name>.<tailnet>.ts.net` from any device on your tailnet — no port forwarding, tokens, or manual setup needed.

```toml
[node]
name = "mac-mini"
bind = "tailscale"
tag = "pulpo"          # optional: only discover nodes with this ACL tag
```

On startup, pulpod automatically runs `tailscale serve` to proxy the local port over HTTPS on port 443. On shutdown, the serve rule is cleaned up. Stale rules from crashes are cleared on next startup. Tag filtering uses Tailscale ACL tags (e.g., `tag:pulpo`). If `tag` is omitted, all online nodes in the tailnet are probed.

### mDNS

Zero-config discovery on the local network. Activates when `bind = "public"` and no `seed` is set.

```toml
[node]
bind = "public"

[auth]
# token is auto-generated on first run
```

Nodes broadcast themselves via `_pulpo._tcp.local.` and automatically discover peers on the same LAN.

### Seed

Bootstrap from a single known peer, then discover its peers transitively. Activates when `bind = "public"` and `seed` is set.

```toml
[node]
bind = "public"
seed = "10.0.0.5:7433"

[auth]
# token is auto-generated on first run
```

The node fetches the seed's peer list via `GET /api/v1/peers` and announces itself back. Works on any network.

### Local / Container

No discovery. `bind = "local"` (default) binds to `127.0.0.1`. `bind = "container"` binds to `0.0.0.0` without auth (trusts container network isolation).

### Manual peers

You can always add peers manually alongside any discovery method:

```toml
[peers]
mac = "10.0.0.1:7433"

[peers.linux]
address = "10.0.0.2:7433"
token = "secret"        # optional: auth token for this peer
```

Manual peers are never overwritten by automatic discovery.

## Docker

Pulpo includes Docker scaffolding and CI-built images for:

- `pulpo-base`
- `pulpo-agents`

See [docker/README.md](docker/README.md) for local build/run and runtime env contract.

## Project Docs

- [pulpo.darioblanco.com](https://pulpo.darioblanco.com) — full documentation site
- [MISSION.md](MISSION.md) — mission and non-goals
- [SPEC.md](SPEC.md) — architecture, lifecycle, API, recovery semantics
- [ROADMAP.md](ROADMAP.md) — shipped work and next steps
- [CONTRIBUTING.md](CONTRIBUTING.md) — local development workflow
- [examples/README.md](examples/README.md) — copy-paste config/API/CLI examples

## Contributing

Contributions are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md), open an issue/discussion for design-level changes, and submit focused PRs with tests.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, contributions are dual-licensed as above.
