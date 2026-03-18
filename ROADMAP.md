# Pulpo Roadmap

Strategic direction for Pulpo as the runtime infrastructure for coding agents.

## Mission

Pulpo is the place where coding agents run — across your machines, in the background, managed from your phone.

It is not an agent framework, not a prompt tool, not an orchestration layer. It is the infrastructure that makes agents reliable, observable, and manageable when you stop watching them.

## The Shift We're Building For

Coding agents are evolving from **pair programmers** (you watch, they code) to **background workers** (you describe, walk away, come back to a PR). This shift creates infrastructure needs that no agent tool solves:

- **Where do agents run?** Not your laptop — it sleeps, runs out of battery, you close the lid. They need servers, and you need to manage those servers.
- **How do you know what's happening?** You're at dinner. Is the migration agent stuck? Did the refactor finish? You need visibility without being attached.
- **What happens when things go wrong?** Memory pressure, stuck agents, crashed machines. Someone (or something) needs to supervise.
- **Who ran what, when?** As agents produce more code, audit trails matter.

This is the gap between "run an agent in your terminal" and "run agents as infrastructure." Pulpo fills it.

## Competitive Landscape (March 2026)

**Agent TUI managers** (Agent Deck 1.6k stars, NTM 191 stars): Manage multiple agent sessions in a terminal. Single-machine, no API, no remote access. Pulpo's multi-node + web UI + API are the differentiators.

**Orchestration frameworks** (Gas Town 12.4k stars): Multi-agent coordination — decompose work, assign to agents, track progress. Complementary to Pulpo, not competitive. Gas Town doesn't care where agents run; Pulpo doesn't care how they coordinate.

**Agent tools** (Claude Code, Codex, Aider, OpenHands): The agents themselves. Pulpo runs them all, competes with none.

**Pulpo's wedge: multi-node runtime + mobile management + API surface.** Nobody else lets you spawn an agent on a remote server from your phone and get notified when it's done.

## Shipped

- `pulpod` daemon + REST API + embedded web UI
- `pulpo` CLI with attach, spawn, resume, kill, logs, schedule
- SQLite-backed session persistence with full lifecycle state machine
- Session statuses: `creating`, `active`, `idle`, `ready`, `killed`, `lost`
- Resume from `lost` and `ready` states
- Watchdog: memory pressure intervention, idle detection, ready TTL cleanup
- Auto-adopt: discovers external tmux sessions and brings them under management
- Command-agnostic sessions (any CLI tool, any command)
- Inks: reusable command templates with description + command
- Multi-node: Tailscale, mDNS, seed-based peer discovery
- SSE event stream, MCP server, Discord bot, webhook notifications
- Crontab-based scheduling
- Ocean gamification UI with canvas rendering
- Homebrew distribution (`brew install darioblanco/tap/pulpo`)
- PWA: installable app with service worker, offline shell caching
- Web Push notifications for session events (ready, killed, intervention)
- Configurable idle threshold (default 60s) + per-session `--idle-threshold`
- Expanded waiting patterns (31 built-in for Claude Code, Codex, Gemini, Aider, Amazon Q, SSH, sudo) + user-configurable extras
- tmux `$N` session ID rework (ghost fix, startup migration)
- Full command capture for adopted sessions
- Optimized `follow_logs` (reduced HTTP polling)
- Default-to-shell spawn: `pulpo spawn my-session` with no command opens `$SHELL`
- CLI node name resolution: `pulpo --node mac-mini spawn` resolves peer names via registry
- Token forwarding from peer config entries
- Fleet sessions endpoint (`GET /api/v1/fleet/sessions`) — server-side aggregation
- Fleet dashboard: "All" tab showing sessions across all nodes in a unified table
- Smart node selection: `pulpo spawn --auto` picks least-loaded online peer

## What's Next

### Phase 3: Background Agent Operations

Make agents reliable when nobody is watching.

**P3.1 — Session cost tracking**
- Track wall-clock time per session
- Configurable cost-per-hour estimate (user sets their API cost rate)
- Dashboard shows cumulative cost per session, per day, per node
- Budget alerts via notifications

**P3.2 — Enhanced scheduling**
- `pulpo schedule` with node targeting: run nightly jobs on the beefy server
- Schedule status in dashboard (next run, last run, last result)
- Retry on failure with configurable backoff

**P3.3 — Output-based completion detection**
- Detect PR URLs in agent output → link in dashboard
- Detect error patterns → auto-alert
- Configurable output matchers (regex → action)

### Phase 4: Team Readiness

When it's not just you anymore.

**P4.1 — Session ownership and audit**
- Track who spawned each session (user identity from token)
- Audit log: who did what, when, on which node
- Read-only dashboard access for observers

**P4.2 — Resource policies**
- Per-node session limits (max 5 concurrent agents)
- Memory reservation per session
- Auto-kill sessions exceeding time limits

**P4.3 — Shared ink library**
- Sync inks across nodes automatically
- Ink versioning (so a team agrees on standard workflows)

## Parked

Revisit when demanded by real usage, not by speculation.

- **Agent-to-agent communication** — orchestration frameworks (Gas Town) handle this better. Pulpo is infrastructure, not workflow.
- **MCP server expansion** — the existing STDIO server (12 tools, 4 resources) works. REST APIs are winning over MCP for integration. Keep as-is.
- **Multi-user auth** — only if team adoption materializes.
- **Docker runtime backend** — only if container-based agent execution shows demand.
- **Node labels/scheduling constraints** — useful at fleet scale, premature now.
- **SLO metrics / Prometheus endpoint** — observability for its own sake; dashboard shows what matters.

## Removed

- Kubernetes-lite framing — too grandiose for the actual product. Pulpo is infrastructure, not a platform.
- Voice-command surfaces
- IDE-native UX competition
- Event replay/export endpoint
- Adapter contract tests against provider binaries

## Success Criteria

Pulpo is succeeding if:

- You spawn agents on remote machines without SSH
- You check agent status from your phone while away from your desk
- Watchdog catches runaway agents before they burn through your API budget
- Sessions survive machine reboots and you resume them without losing context
- Multiple agents run overnight and you wake up to PRs, not crashed terminals

## Architectural Principles

- Infrastructure layer, not intelligence layer
- Command-agnostic: runs any agent, any command
- Multi-node native: sessions are not tied to localhost
- Mobile-first web UI: the phone is the primary management surface
- Explicit failure semantics: every state transition is observable and auditable
- Zero-config local start, progressive operational depth
