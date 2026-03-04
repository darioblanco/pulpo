# Pulpo Roadmap

Project sequencing and strategic direction. Not a changelog — a living document of what's shipped, what's next, and why.

## Mission

Infrastructure layer for autonomous coding agents. Pulpo manages **where and how agents run** — daemon, API, multi-node, persistence, observability — while staying provider-agnostic and out of the way of agent frameworks.

## Shipped

### Phase 1-3: Core Platform

- Single-node daemon (`pulpod`) with REST API and tmux backend
- CLI client (`pulpo`) for all session operations
- Embedded web UI (Svelte 5 + Konsta UI, served by pulpod)
- SQLite persistence — sessions survive daemon restarts and reboots
- Live terminal streaming (WebSocket + xterm.js)
- Session resume after reboot (STALE → RUNNING)
- Multi-node peer config with aggregated dashboard
- Remote session spawning from any node's UI
- Output capture via `tmux capture-pane`

### Phase 4: Guard System + Multi-Provider

- Guard presets (standard/strict/yolo) with per-provider flags
- Environment variable sanitization (allow/deny patterns)
- Claude Code and OpenAI Codex providers
- Autonomous mode (fire-and-forget spawning)

### Phase 5: Web UI + API Surface

- Konsta UI migration (iOS-native look, responsive phone/tablet/desktop)
- Config API (`GET/PUT /api/v1/config`) with hot-reload
- Settings view (Node, Guards, Peers tabs)
- Session history with search/filter/sort
- Output download endpoint
- In-app toast + desktop Notification API
- Peer management API and UI (add/remove/status)

### Phase 6: Auth + Discovery

- Token authentication for LAN-accessible deployments
- QR code pairing for mobile clients
- mDNS discovery (`_pulpo._tcp.local.`) — auto-detects peers in `lan` bind mode
- Connection management (saved connections, base URL routing)

### Phase 7: Voice Commands (experimental)

- iOS Siri Shortcuts integration
- Android App Actions via Google Assistant

### Phase 8: Control Plane + Discord

- Flexible session model (model, allowed_tools, system_prompt, metadata)
- Persona config (`[personas.name]` in config.toml, `GET /api/v1/personas`)
- SSE event stream (`GET /api/v1/events`, broadcast channel)
- MCP server (session management as MCP tools for agent-to-agent orchestration)
- Discord webhook notifications (`[notifications.discord]` config)
- Discord bot (`contrib/discord-bot/`) — slash commands + SSE listener

### Core Cleanup (post-Phase 8)

- Deleted dead code (state machine, output placeholder)
- Removed detection events subsystem (watchdog acts directly, no storage overhead)
- Removed watchdog auto-recovery (users prefer explicit `pulpo resume`)
- Simplified guard system (preset-only config, free functions instead of trait)
- Deduplicated stale detection, removed dead SSE client code
- Replaced peer health polling with lazy on-demand probing (60s TTL cache)
- Replaced internal scheduling engine (~3,400 lines) with crontab wrapper (~150 lines)

## Next Steps (in order)

### 1. Minimal `pulpo deploy` (immediate user value)

Ship a deploy workflow that gets pulpod running on a remote node in one command.
Goal: `pulpo deploy user@host` copies binary, installs systemd/launchd service, verifies health.

### 2. Provider adapter/registry

Replace the hardcoded Provider enum with a trait-based adapter registry.
Each provider (Claude, Codex, future) implements a `ProviderAdapter` trait that knows how to build CLI commands, parse output, and detect status.
Do this before adding any new providers.

### 3. Per-process kill

Kill runaway child processes (e.g., infinite test loops) without killing the agent session itself.
Requires walking the process tree inside the tmux session.

### 4. Tailscale API discovery

Automatic peer detection via the Tailscale local API.
Complements mDNS (LAN-only) with Tailnet-wide discovery for machines not on the same subnet.

### 5. Tiered coverage + nightly smoke matrix

- Define module-level coverage tiers (core=100%, adapters=95%, contrib=best-effort)
- Add a nightly CI job that runs smoke tests against real tmux (not mocked)
- Keep local pre-commit at 100% for core, relax for adapter crates

### 6. Freeze core contract

Lock the core API surface: session lifecycle, guard semantics, persistence schema, event types.
Document the contract in SPEC.md with a stability guarantee.
Enforce in PR review: changes to core contract require explicit approval + migration plan.

## Not Planned

- **Docker/bubblewrap sandboxing** — guard presets handle the security layer; container overhead isn't justified for the target use case
- **Multi-user / team features** — single-user tool, your Tailnet is your trust boundary

## Architectural Principles

- Infrastructure/runtime layer, not an agent framework
- Provider-agnostic: adapters for fast-moving AI CLI tools
- Single-node-first, multi-node-ready
- Zero-config defaults, full customization via config.toml
