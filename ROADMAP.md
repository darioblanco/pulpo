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
- Docker runtime: `pulpo spawn --runtime docker` runs sessions in isolated Docker containers
- CLI node name resolution: `pulpo --node mac-mini spawn` resolves peer names via registry
- Token forwarding from peer config entries
- Fleet sessions endpoint (`GET /api/v1/fleet/sessions`) — server-side aggregation
- Fleet dashboard: "All" tab showing sessions across all nodes in a unified table
- Smart node selection: `pulpo spawn --auto` picks least-loaded online peer
- Git worktrees: `pulpo spawn --worktree` creates isolated worktrees for parallel agents on the same repo. Auto-cleanup on kill/delete. Worktree badge in CLI and dashboard.

## What's Next

### Phase 3: Built-in Scheduler

Replace the crontab wrapper with a first-class scheduler inside `pulpod`. Schedules are DB-backed, visible in the dashboard, and support multi-node targeting.

**P3.1 — Scheduler engine**
- `schedules` SQLite table: name, cron, command, workdir, target_node, ink, enabled, last_run_at, last_session_id
- Scheduler loop (like watchdog): ticks every 60s, checks which schedules are due, calls `session_manager.create_session()`
- Migrate existing crontab wrapper: `pulpo schedule` CRUD talks to the DB, not crontab
- Run history: link spawned sessions back to their schedule

**P3.2 — Multi-node scheduling**
- `target_node` field: `NULL` = local, `"mac-mini"` = specific node, `"auto"` = least-loaded peer
- Local schedules fire via `session_manager.create_session()` directly
- Remote schedules fire via HTTP POST to the target node's `/api/v1/sessions`
- Auto schedules use `select_best_node` logic at fire time

**P3.3 — Schedule API + CLI**
- REST CRUD: `POST/GET/PUT/DELETE /api/v1/schedules`, `GET /api/v1/schedules/:id/runs`
- CLI: `pulpo schedule add nightly-review "0 3 * * *" --node gpu-box -- claude -p "review"`
- CLI: `pulpo schedule list`, `pulpo schedule pause <name>`, `pulpo schedule remove <name>`
- SSE events for schedule fires and failures

**P3.4 — Schedule dashboard**
- Schedule list: name, cron, next run, last run, target node, enabled toggle
- Create/edit schedule dialog with cron builder
- Run history per schedule (links to spawned sessions)
- Schedule notifications: fire, success, failure

### Phase 5: Background Agent Operations

Make agents reliable when nobody is watching.

**P5.1 — Session cost tracking**
- Track wall-clock time per session (already have created_at/updated_at)
- Configurable cost-per-hour estimate (user sets API cost rate)
- Dashboard shows cumulative cost per session, per day, per node
- Budget alerts via notifications
- Stretch: parse Claude Code transcript JSONL for real token costs

**P5.2 — Output-based completion detection**
- Detect PR URLs in agent output → link in dashboard
- Detect error patterns → auto-alert
- Configurable output matchers (regex → action)

### Phase 6: Team Readiness

When it's not just you anymore.

**P6.1 — Session ownership and audit**
- Track who spawned each session (user identity from token)
- Audit log: who did what, when, on which node
- Read-only dashboard access for observers

**P6.2 — Resource policies**
- Per-node session limits (max 5 concurrent agents)
- Memory reservation per session
- Auto-kill sessions exceeding time limits

**P6.3 — Shared ink library**
- Sync inks across nodes automatically
- Ink versioning (so a team agrees on standard workflows)

## Parked

Revisit when demanded by real usage, not by speculation.

- **Agent-to-agent communication** — orchestration frameworks (Gas Town) handle this better. Pulpo is infrastructure, not workflow.
- **MCP server expansion** — the existing STDIO server (12 tools, 4 resources) works. REST APIs are winning over MCP for integration. Keep as-is.
- **Multi-user auth** — only if team adoption materializes.
- ~~**Docker runtime backend**~~ — shipped as `--runtime docker` flag.
- **Kubernetes backend** — implement when team adoption or cluster-scale demand materializes. The Backend trait is ready.
- **Cloud VM backend** — ephemeral machines (Hetzner, AWS, GCP). Spin up for a task, tear down when done. Requires provider-specific APIs.
- **Node labels/scheduling constraints** — useful at fleet scale, premature now.
- **SLO metrics / Prometheus endpoint** — observability for its own sake; dashboard shows what matters.

## Removed

- ~~Kubernetes-lite framing~~ — the "universal agent runtime" vision is the right framing now that we have multiple backends.
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
