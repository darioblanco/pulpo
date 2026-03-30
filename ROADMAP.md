# Pulpo Roadmap

Strategic direction for Pulpo as the self-hosted control plane for background
coding agents.

## Mission

Pulpo is the place where coding agents run when the runtime needs to stay under
your control.

Your laptop, your server, a Docker container, eventually a cluster. Same session
model, same lifecycle semantics, same control surfaces.

It is not an agent framework, not a prompt tool, not an orchestration layer. It
is the infrastructure that makes agents reliable, observable, and manageable
when you stop watching them.

## Positioning

Pulpo is a self-hosted control plane for background coding agents.

The category matters. The market now has:

- hosted coding agents with vendor-owned sandboxes and PR workflows
- local agent managers focused on terminal UX
- orchestration frameworks that decide how multiple agents collaborate

Pulpo sits below and beside those categories. It focuses on where agents run,
how they are supervised, and what happens when they fail.

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

The market has moved quickly toward async and background execution:

- **Hosted coding agents** now offer cloud execution, PR-native delegation, and
  managed sandboxes.
- **Execution infrastructure** vendors are starting to market sandbox and
  runtime layers for agents.
- **Local command centers** manage multiple sessions well, but stay focused on
  terminal UX and single-machine workflows.

That makes Pulpo more relevant, not less relevant. But it sharpens the wedge.

**Hosted coding agents**: Strongest at managed cloud convenience and provider
integration. Weakest where users need self-hosting, private-network access,
bring-your-own-agent flexibility, and daemon-owned recovery semantics.

**Local agent managers**: Strong at terminal-centric multi-session workflows.
Weaker at multi-node control, durable remote supervision, and policy-driven
recovery.

**Orchestration frameworks**: Complementary rather than competitive. They decide
what agents should do. Pulpo decides where sessions run and how they are
supervised.

**Pulpo's wedge**: self-hosted execution + multi-node supervision +
command-agnostic agent support + explicit session lifecycle.

## Product Thesis

Pulpo should feel like:

- the private control plane for agent fleets
- the durable session layer under agent work
- the operational bridge between local agents and background infrastructure

Pulpo should not be framed primarily as:

- a tmux abstraction
- a generic "universal runtime"
- an orchestration framework
- a replacement for hosted coding-agent products

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
- Git worktrees: `pulpo spawn --worktree` creates isolated branch (named after session) in `~/.pulpo/worktrees/<name>/`. Auto-cleanup on stop (removes worktree dir, prunes refs, deletes branch). `--worktree-base <branch>` forks from a specific branch (implies `--worktree`). `pulpo worktree list` / `pulpo wt ls` lists worktree sessions. Stale branches auto-cleaned on retry. `worktree_branch` field in Session/API responses. Branch badge in CLI (`[wt]`) and dashboard.
- Built-in scheduler: DB-backed schedules with cron expressions, CRUD API (`/api/v1/schedules`), CLI (`pulpo schedule add/list/pause/resume/remove`), scheduler loop firing every 60s
- Schedule dashboard: create/edit dialog with cron presets, next-run calculation, status filtering, expandable run history per schedule (`/api/v1/schedules/{id}/runs`)
- PR/branch detection: watchdog scans session output for GitHub/GitLab/Bitbucket PR URLs and git branch pushes, stores in session metadata, surfaces as clickable badges in dashboard and `[PR]` marker in CLI
- Watchdog telemetry: git branch/commit tracking, diff stats (+N/-N files), commits ahead of remote, error/failure detection (10+ patterns), token usage parsing, rate limit detection — all updated per tick, surfaced in CLI (REPO column) and web UI (badges + detail)
- Worktree lifecycle: stop preserves worktree for resume, purge cleans up. `--worktree-base` forks from a specific branch. Branch cleanup on stop. Stale branch auto-recovery.
- Unified sessions page: merged history into sessions with multi-select status filter chips. Dropped separate history page.
- Mobile PWA fixes: bottom nav visibility, horizontal scroll prevention, scrollable node tabs
- `pulpo ls` shows live sessions by default (`-a` for all), with dynamic-width ID, NAME, STATUS, BRANCH (with diff stats +N/-N and ↑ahead), COMMAND columns. Worktree/PR/error indicators on session names.
- Enriched notifications: Discord embeds, web push, and webhooks carry git branch/commit, diff stats, PR URL, and error status. Example: "Session portal is now ready — created PR (+42/-7, 3 files) on branch fix-auth"
- CLI auto-start daemon: `pulpo spawn` auto-starts `pulpod` via brew services / systemd / direct spawn if not running on localhost
- UI feature parity: new session dialog supports worktree-base, runtime (docker), idle threshold. Cleanup button for batch-removing stopped/lost sessions. Session card badges wrap on mobile.
- Stale backend ID recovery: stop and resume fall back to session name when the tmux `$N` ID is stale
- Browser suppression: sessions set `BROWSER=true` and wrap `open()` to block URL opens while passing file/dir opens through
- iOS PWA reliability: service worker with skipWaiting + clientsClaim, SPA navigation fallback, SSE always reconnects on visibility change (iOS kills connections silently in background)
- Session liveness check: CLI polls session status with retries before attach on spawn/resume
- Secret store: encrypted-at-rest secrets (`pulpo secret set/list/delete`) with `--env` override for env var mapping, `--secret` flag on spawn for injection via temp files (tmux) or `-e` flags (Docker). Secrets never in command strings, `ps` output, or logs.
- Ink blueprints: inks support `secrets` and `runtime` fields, making them full session blueprints. Ink + request secrets are merged, request `--runtime` overrides ink default.
- Docker auth volumes: auto-mount `~/.claude`, `~/.codex`, `~/.gemini` (read-only) for OAuth/subscription auth. macOS Keychain extraction for Claude Code. Configurable via `[docker] volumes`.

## What's Next

### Positioning, Distribution, and Proof

Pulpo's main near-term risk is not missing features. It is being understood as a
nice session manager instead of a category-defining control plane.

**Category clarity**
- Keep leading docs, README, and site language centered on self-hosted
  background agents, not tmux internals
- Show concrete "why not hosted" and "why not local-only" comparisons
- Publish a clear positioning page and architecture overview aimed at operators,
  not just hackers

**Proof of value**
- Demo video showing: remote spawn, overnight run, phone check-in, recovery, PR
  result
- Real-world examples: nightly review, migration rehearsal, parallel worktree
  implementation, security scan, docs generation
- 5-minute quickstart that reaches a visible result fast

**Distribution**
- Landing page tuned to the control-plane category
- Docs information architecture optimized for "why Pulpo" before exhaustive
  reference detail
- Homebrew-core submission once adoption and packaging criteria make sense

### Reliability and Policy Depth

These reinforce the control-plane position directly.

**Notification digest**
- Daily/weekly summary of agent activity (sessions completed, PRs created, errors encountered)
- Enriched per-event notifications are shipped; this adds aggregation

**Configurable output matchers (P5.2)**
- User-defined regex to action rules in `config.toml`
- Extends hardcoded error/PR/rate-limit detection to custom operational patterns
- Build when users ask for patterns we do not cover

**Session cost dashboard (P5.1)**
- Token tracking from output is shipped; cost = tokens x rate
- Configurable cost rates, cumulative cost per session/day/node, budget alerts
- Build when cost visibility becomes a real pain point

### Distribution Milestones

**Homebrew-core submission**
- Requires ≥75 GitHub stars
- Source build, `brew audit` compliance, no auto-restart on upgrade

### Parked Features (build when demanded)

**Multi-node scheduling (P3.2)**
- `target_node` on schedules: `NULL` = local, `"mac-mini"` = specific, `"auto"` = least-loaded
- Remote dispatch via HTTP POST to target node's API
- Build when there are real users with multi-node fleets running scheduled agents

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
- **Multi-node scheduling** — moved to Parked Features in What's Next. Build when real fleet usage demands it.

## Removed

- ~~Universal runtime as the primary framing~~ — the sharper framing is
  self-hosted control plane for background coding agents.
- ~~Docker runtime backend~~ — shipped as `--runtime docker` flag.
- Voice-command surfaces
- IDE-native UX competition
- Event replay/export endpoint
- Adapter contract tests against provider binaries

## Success Criteria

Pulpo is succeeding if:

- users understand it as infrastructure for unattended agent work, not as a tmux helper
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
- Self-hosted first: the runtime belongs on infrastructure the user controls
- Mobile-first web UI: the phone is the primary management surface
- Explicit failure semantics: every state transition is observable and auditable
- Zero-config local start, progressive operational depth
