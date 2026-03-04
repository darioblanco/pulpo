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

- Guard presets (standard/strict/unrestricted) with per-provider flags
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

- Token authentication for network-accessible deployments
- QR code pairing for mobile clients
- mDNS discovery (`_pulpo._tcp.local.`) — auto-detects peers in `public` bind mode
- Connection management (saved connections, base URL routing)
- Bind mode rename: `lan` → `public` (clearer risk signal) + `container` mode (0.0.0.0 without auth)

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
- Workdir and provider binary validation before spawning (clear errors instead of silent tmux death)
- Web UI surfaces API error messages in session creation form

### Web UI: SSE + Visual Redesign

Dashboard overhaul — real-time SSE updates, brand-system visual refresh, full form coverage.

**Shipped:**
- SSE event store with auto-reconnect (replaces 5s polling, exponential backoff)
- KPI summary tiles (Running/Idle/Done/Dead counts) at top of dashboard
- Session cards: status-colored left borders, duration, waiting-for-input quick-send row
- NodeCard: brand-styled cards with seafoam/red status dots
- Persona dropdown in NewSessionForm (fetches from `GET /api/v1/personas`, pre-fills fields)
- Advanced options section (model, max_turns, max_budget_usd, output_format) in form
- 2-column responsive grid layout for form fields (mobile-first)
- Brand palette (navy/blue/aqua/seafoam) with Space Grotesk/Inter/JetBrains Mono fonts
- Responsive heights: ChatView `max-h-[60vh]`, Terminal `h-[40vh] min-h-[200px]`

## Next Steps

### Config: Full UI Coverage

Several config sections are TOML-only with no API endpoint or UI. Users shouldn't have to SSH in and edit files for basic settings.

**Expose watchdog settings**
- Add `GET/PUT /api/v1/watchdog` endpoint (memory threshold, idle timeout, idle action)
- Add Watchdog tab to Settings page

**Expose notification settings**
- Add `GET/PUT /api/v1/notifications` endpoint (Discord webhook URL, event filters)
- Add Notifications tab to Settings page

**Complete guard defaults in API**
- Extend `UpdateConfigRequest` to include max_turns, max_budget_usd, output_format (already in TOML config, just not exposed via API)

**Bind mode in Settings**
- Add Local/Public/Container toggle to Node section (flags restart_required like port does)

### Discord Bot (contrib): Multi-Node and Production Readiness

The contrib bot works for single-node control but is not yet a full control-plane interface.

**Multi-node control**
- Add node registry support (`node_name -> base_url + token`)
- Add `/nodes` command and `node` option on existing commands
- Add per-node default/fallback routing in config

**Schedule feature parity**
- Add `/schedule` command group (`create`, `list`, `get`, `run`, `pause`, `resume`, `history`)
- Add schedule execution rendering in embeds

**SSE parity**
- Handle both `session` and `schedule` events from `/api/v1/events`
- Route schedule events to the same channel mapping strategy as session events

**Reliability and ops**
- Distinguish reconnect vs hard-error logs for SSE
- Add event dedupe/rate limiting to avoid flood during state storms
- Add command-level integration tests (mock Discord interactions + mocked pulpod API)

**Distribution**
- Add first-class Docker packaging for the bot (compose profile + env contract)
- Document production deployment on a dedicated control node

## Not Planned

- **Docker/bubblewrap sandboxing** — guard presets handle the security layer; container overhead isn't justified for the target use case
- **Multi-user / team features** — single-user tool, your Tailnet is your trust boundary

## Architectural Principles

- Infrastructure/runtime layer, not an agent framework
- Provider-agnostic: adapters for fast-moving AI CLI tools
- Single-node-first, multi-node-ready
- Zero-config defaults, full customization via config.toml
