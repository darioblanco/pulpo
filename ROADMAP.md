# Pulpo Roadmap

Strategic direction for Pulpo as the runtime infrastructure for coding agents.

## Mission

Pulpo is the **universal agent runtime** — the place where coding agents run, regardless of where "run" means. Your laptop, your server, a Docker container, a Kubernetes cluster. Same lifecycle, same controls, same dashboard.

It is not an agent framework, not a prompt tool, not an orchestration layer. It is the infrastructure that makes agents reliable, observable, and manageable when you stop watching them.

## Backend Progression

Each backend serves a different scale and isolation need. The session lifecycle, watchdog, scheduler, and fleet dashboard work identically across all of them.

| Backend | When to use | Status |
|---------|------------|--------|
| **tmux** | Your laptop, your servers. Zero infrastructure. | Shipped |
| **Docker** | Same machines but isolated. Safe for `--dangerously-skip-permissions`. | Shipped |
| **Kubernetes** | Scale to a cluster. Teams with shared infrastructure. | Future |
| **Cloud VMs** | Ephemeral machines. Spin up for a task, tear down when done. | Future |

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
- `pulpo` CLI with attach, spawn, resume, stop, logs, schedule
- SQLite-backed session persistence with full lifecycle state machine
- Session statuses: `creating`, `active`, `idle`, `ready`, `stopped`, `lost`
- Resume from `lost` and `ready` states (with workdir validation)
- Watchdog: memory pressure intervention, idle detection, ready TTL cleanup
- Auto-adopt: discovers external tmux sessions and brings them under management
- Command-agnostic sessions (any CLI tool, any command)
- Inks: reusable command templates with description + command
- Multi-node: Tailscale, mDNS, seed-based peer discovery
- SSE event stream, MCP server, Discord bot, webhook notifications
- Ocean gamification UI with canvas rendering
- Homebrew distribution (`brew install darioblanco/tap/pulpo`)
- PWA: installable app with service worker, offline shell caching
- Web Push notifications for session events (ready, stopped, intervention)
- Configurable idle threshold (default 60s) + per-session `--idle-threshold`
- Expanded waiting patterns (31 built-in for Claude Code, Codex, Gemini, Aider, Amazon Q, SSH, sudo) + user-configurable extras
- tmux `$N` session ID rework (ghost fix, startup migration)
- Full command capture for adopted sessions
- Optimized `follow_logs` (reduced HTTP polling)
- Default-to-shell spawn: `pulpo spawn my-session` with no command opens `$SHELL`
- Docker runtime: `pulpo spawn --runtime docker` runs sessions in isolated Docker containers, `pulpo attach` uses `docker exec`
- Runtime enum: extensible `Runtime` type (tmux/docker) replacing the old boolean sandbox flag
- CLI node name resolution: `pulpo --node mac-mini spawn` resolves peer names via registry
- Token forwarding from peer config entries
- Fleet sessions endpoint (`GET /api/v1/fleet/sessions`) — server-side aggregation
- Fleet dashboard: "All" tab showing sessions across all nodes in a unified table
- Smart node selection: `pulpo spawn --auto` picks least-loaded online peer
- Git worktrees: `pulpo spawn --worktree` creates isolated `pulpo/<name>` branch in `~/.pulpo/worktrees/<name>/`. Auto-cleanup on stop with logging. Branch badge in CLI (`[wt]`) and dashboard.
- Built-in scheduler: DB-backed schedules with cron expressions, CRUD API (`/api/v1/schedules`), CLI (`pulpo schedule add/list/pause/resume/remove`), scheduler loop firing every 60s
- Schedule dashboard: create/edit dialog with cron presets, next-run calculation, status filtering, expandable run history per schedule (`/api/v1/schedules/{id}/runs`)
- PR/branch detection: watchdog scans session output for GitHub/GitLab/Bitbucket PR URLs and git branch pushes, stores in session metadata, surfaces as clickable badges in dashboard and `[PR]` marker in CLI
- `pulpo ls` shows live sessions by default (`-a` for all), with ID, RUNTIME, and worktree/PR indicators
- Session liveness check: CLI polls session status with retries before attach on spawn/resume
- Secret store: encrypted-at-rest secrets (`pulpo secret set/list/delete`) with `--env` override for env var mapping, `--secret` flag on spawn for injection via temp files (tmux) or `-e` flags (Docker). Secrets never in command strings, `ps` output, or logs.
- Ink blueprints: inks support `secrets` and `runtime` fields, making them full session blueprints. Ink + request secrets are merged, request `--runtime` overrides ink default.
- Docker auth volumes: auto-mount `~/.claude`, `~/.codex`, `~/.gemini` (read-only) for OAuth/subscription auth. macOS Keychain extraction for Claude Code. Configurable via `[docker] volumes`.

## What's Next

### Phase 3: Multi-node Scheduling

**P3.2 — Multi-node scheduling**
- `target_node` field: `NULL` = local, `"mac-mini"` = specific node, `"auto"` = least-loaded peer
- Local schedules fire via `session_manager.create_session()` directly
- Remote schedules fire via HTTP POST to the target node's `/api/v1/sessions`
- Auto schedules use `select_best_node` logic at fire time
- SSE events for schedule fires and failures

### Phase 5: Background Agent Operations

Make agents reliable when nobody is watching.

**P5.1 — Session cost tracking**
- Track wall-clock time per session (already have created_at/updated_at)
- Configurable cost-per-hour estimate (user sets API cost rate)
- Dashboard shows cumulative cost per session, per day, per node
- Budget alerts via notifications
- Stretch: parse Claude Code transcript JSONL for real token costs

**P5.2 remaining — Configurable output matchers**
- Detect error patterns → auto-alert
- Configurable output matchers (regex → action) in config
- PR/branch detection is shipped; this extends it to user-defined patterns

### Phase 6: Team Readiness

When it's not just you anymore.

**P6.1 — Session ownership and audit**
- Track who spawned each session (user identity from token)
- Audit log: who did what, when, on which node
- Read-only dashboard access for observers

**P6.2 — Resource policies**
- Per-node session limits (max 5 concurrent agents)
- Memory reservation per session
- Auto-stop sessions exceeding time limits

**P6.3 — Shared ink library**
- Sync inks across nodes automatically
- Ink versioning (so a team agrees on standard workflows)

## Parked

Revisit when demanded by real usage, not by speculation.

- **Agent-to-agent communication** — orchestration frameworks (Gas Town) handle this better. Pulpo is infrastructure, not workflow.
- **MCP server expansion** — the existing STDIO server (12 tools, 4 resources) works. REST APIs are winning over MCP for integration. Keep as-is.
- **Multi-user auth** — only if team adoption materializes.
- **Kubernetes backend** — implement when team adoption or cluster-scale demand materializes. The Backend trait is ready.
- **Cloud VM backend** — ephemeral machines (Hetzner, AWS, GCP). Spin up for a task, tear down when done. Requires provider-specific APIs.
- **Node labels/scheduling constraints** — useful at fleet scale, premature now.
- **SLO metrics / Prometheus endpoint** — observability for its own sake; dashboard shows what matters.
- **Worktree merge/PR action** — agents create PRs themselves; a pulpo-level merge button would duplicate agent functionality.

## Removed

- ~~Kubernetes-lite framing~~ — the "universal agent runtime" vision is the right framing now that we have multiple backends.
- ~~Docker runtime backend~~ — shipped as `--runtime docker` flag.
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
- Nightly code reviews and security scans run themselves, you just check results in the morning

## Architectural Principles

- Infrastructure layer, not intelligence layer
- Command-agnostic: runs any agent, any command
- Multi-node native: sessions are not tied to localhost
- Mobile-first web UI: the phone is the primary management surface
- Explicit failure semantics: every state transition is observable and auditable
- Zero-config local start, progressive operational depth
