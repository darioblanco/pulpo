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
  <a href="https://github.com/darioblanco/pulpo/releases"><img src="https://img.shields.io/github/v/release/darioblanco/pulpo?display_name=tag" alt="Latest Release"></a>
  <a href="https://github.com/darioblanco/pulpo/releases"><img src="https://img.shields.io/github/release-date/darioblanco/pulpo" alt="Release Date"></a>
  <a href="https://hub.docker.com/r/darioblanco/pulpo-base"><img src="https://img.shields.io/docker/pulls/darioblanco/pulpo-base" alt="Docker Hub: pulpo-base"></a>
  <a href="https://hub.docker.com/r/darioblanco/pulpo-agents"><img src="https://img.shields.io/docker/pulls/darioblanco/pulpo-agents" alt="Docker Hub: pulpo-agents"></a>
  <a href="https://hub.docker.com/r/darioblanco/pulpo-discord-bot"><img src="https://img.shields.io/docker/pulls/darioblanco/pulpo-discord-bot" alt="Docker Hub: pulpo-discord-bot"></a>
  <a href="https://github.com/darioblanco/pulpo#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="License: MIT OR Apache-2.0"></a>
</p>

> **Experimental** — Pulpo is in early development. APIs, config format, and behavior may change between releases.

Coding agents are powerful, but running them across multiple machines is operationally painful. Pulpo is infrastructure that makes agent execution **reliable**, **observable**, and **controllable** — without replacing the agents themselves.

- **Session lifecycle** — explicit states (`active`, `idle`, `finished`, `killed`, `lost`) with resume semantics for crash recovery.
- **Cross-node operations** — manage agents on multiple machines from a single API/CLI/web surface, with Tailscale, mDNS, or seed-based discovery.
- **Watchdog interventions** — memory pressure detection, idle handling, and configurable kill policies with audit trails.
- **Provider-agnostic** — Claude Code, Codex, Gemini CLI, OpenCode, or bare shell. Same lifecycle, same controls.
- **Collective culture** — agents write learnings back to a shared AGENTS.md-based knowledge base. Culture syncs across nodes, so agents improve from each other's sessions over time.
- **Self-hosted and open source** — your machines, your data, your control.

## Get Started

```bash
# Install (macOS)
brew install darioblanco/tap/pulpo

# Start daemon
pulpod

# Spawn a session
pulpo spawn --workdir ~/repos/my-api "Fix failing auth tests"

# Watch progress
pulpo logs my-api --follow

# Open web UI
open http://localhost:7433
```

<h3 align="center">
  <a href="https://pulpo.darioblanco.com/getting-started/install">Install</a>
  <span> · </span>
  <a href="https://pulpo.darioblanco.com/getting-started/quickstart">Quickstart</a>
  <span> · </span>
  <a href="https://pulpo.darioblanco.com">Documentation</a>
  <span> · </span>
  <a href="CONTRIBUTING.md">Contributing</a>
</h3>

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
