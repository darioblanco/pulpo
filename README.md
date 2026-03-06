# Pulpo

[![CI](https://github.com/darioblanco/pulpo/actions/workflows/ci.yml/badge.svg)](https://github.com/darioblanco/pulpo/actions/workflows/ci.yml)
[![Docker Images](https://github.com/darioblanco/pulpo/actions/workflows/docker-images.yml/badge.svg)](https://github.com/darioblanco/pulpo/actions/workflows/docker-images.yml)
[![Latest Release](https://img.shields.io/github/v/release/darioblanco/pulpo?display_name=tag)](https://github.com/darioblanco/pulpo/releases)
[![Release Date](https://img.shields.io/github/release-date/darioblanco/pulpo)](https://github.com/darioblanco/pulpo/releases)
[![Docker Hub: pulpo-base](https://img.shields.io/docker/pulls/darioblanco/pulpo-base)](https://hub.docker.com/r/darioblanco/pulpo-base)
[![Docker Hub: pulpo-agents](https://img.shields.io/docker/pulls/darioblanco/pulpo-agents)](https://hub.docker.com/r/darioblanco/pulpo-agents)
[![Docker Hub: pulpo-discord-bot](https://img.shields.io/docker/pulls/darioblanco/pulpo-discord-bot)](https://hub.docker.com/r/darioblanco/pulpo-discord-bot)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/darioblanco/pulpo#license)

> **⚠️ Experimental** — Pulpo is in early development. APIs, config format, and behavior may change between releases. Feedback and contributions are welcome.

Self-hosted control plane for coding agents on your own machines.

Pulpo runs as a daemon (`pulpod`) with a CLI (`pulpo`) and embedded web UI. It manages agent sessions (Claude Code, Codex) over `tmux`, persists lifecycle state in SQLite, and supports recovery after restarts/reboots.

## Why Pulpo

- Manage agents across machines without SSH/tmux juggling.
- Recover from failures with explicit session states and resume flow.
- Apply guard presets for safer default execution.
- Observe behavior through logs, interventions, and SSE events.
- Stay in control: self-hosted, API-first, open source.

## What Works Today

- `pulpod` daemon with REST API + embedded web UI
- `pulpo` CLI for local and remote node control
- Session lifecycle: `creating`, `running`, `completed`, `dead`, `stale`
- Resume flow for stale sessions (`pulpo resume <name>`)
- Watchdog interventions (memory pressure, idle handling)
- Multi-node support (manual peers + mDNS discovery in `public` bind mode)
- Persona support (`[personas.<name>]` in config)
- Schedule management via crontab wrapper (`pulpo schedule ...`)
- SSE stream: `GET /api/v1/events`
- MCP server mode: `pulpod mcp`
- Optional Discord integration in [`contrib/discord-bot/`](contrib/discord-bot/README.md)

## Quickstart

### 1. Install

macOS (Homebrew):

```bash
brew install darioblanco/tap/pulpo tmux
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

Codex also works if installed/authenticated in your environment.

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
# my-api   stale   ...

pulpo resume my-api
pulpo logs my-api --follow
```

`resume` is for `stale` sessions (record exists, tmux process gone). `dead` sessions require a new `spawn`.

## CLI At A Glance

```bash
pulpo spawn --workdir <PATH> [PROMPT...]
pulpo list
pulpo logs <NAME> [--follow]
pulpo attach <NAME>
pulpo input <NAME> [TEXT]
pulpo kill <NAME>
pulpo resume <NAME>
pulpo interventions <NAME>
pulpo nodes
pulpo schedule <install|list|pause|resume|remove>
pulpo ui
```

For full options, run `pulpo --help` and `pulpo <command> --help`.

## Configuration

Default config path: `~/.pulpo/config.toml`

Example:

```toml
[node]
name = "mac-mini"

[personas.reviewer]
provider = "claude"
model = "sonnet"
guard_preset = "strict"
system_prompt = "You are a senior reviewer focused on correctness and security."
```

See [SPEC.md](SPEC.md#configuration) for all supported config sections.

## Docker

Pulpo includes Docker scaffolding and CI-built images for:

- `pulpo-base`
- `pulpo-agents`

See [docker/README.md](docker/README.md) for local build/run and runtime env contract.

## Project Docs

- [MISSION.md](MISSION.md): mission and non-goals
- [SPEC.md](SPEC.md): architecture, lifecycle, API, recovery semantics
- [ROADMAP.md](ROADMAP.md): shipped work and next steps
- [CONTRIBUTING.md](CONTRIBUTING.md): local development workflow
- [examples/README.md](examples/README.md): copy-paste config/API/CLI examples

## Contributing

Contributions are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md), open an issue/discussion for design-level changes, and submit focused PRs with tests.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, contributions are dual-licensed as above.
