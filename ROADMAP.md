# Pulpo Roadmap

Strategic direction for Pulpo as agent-agnostic infrastructure you own.

## Mission

Pulpo is the **runtime infrastructure for coding agents** — the place where agents run reliably as background workers on your machines. Your laptop, your server, a Docker container. Same lifecycle, same controls, same dashboard.

It is not an agent framework, not a prompt tool, not an orchestration layer. It is the infrastructure that makes agents reliable, observable, and manageable when you stop watching them. It works with any command-line agent: Claude Code, Codex, Aider, Goose, OpenCode, or anything that runs in a terminal.

## The Shift We're Building For

Coding agents are evolving from **pair programmers** (you watch, they code) to **background workers** (you describe, walk away, come back to a PR). This shift creates infrastructure needs that no agent tool solves:

- **Where do agents run?** Not your laptop — it sleeps, runs out of battery, you close the lid. They need servers, and you need to manage those servers.
- **How do you know what's happening?** You're at dinner. Is the migration agent stuck? Did the refactor finish? You need visibility without being attached.
- **What happens when things go wrong?** Memory pressure, stuck agents, crashed machines. Someone (or something) needs to supervise.
- **What did it cost?** Five agents running overnight can burn through API budgets. You need per-session cost visibility and budget guardrails.
- **Who ran what, when?** As agents produce more code, audit trails matter — especially in regulated environments.

This is the gap between "run an agent in your terminal" and "run agents as infrastructure." Pulpo fills it.

## Sovereignty by Architecture

Pulpo runs on your machines. Code never leaves your infrastructure — not "we promise we won't look," but architecturally guaranteed. No cloud dependency, no data transit to foreign jurisdictions.

This matters especially in Europe:

- **CLOUD Act**: US authorities can compel US cloud providers to hand over data regardless of where it's stored. Cloud-based agent VMs (Cursor Background Agents, Devin) are structurally exposed.
- **EU AI Act**: High-risk system compliance deadline is August 2026. Autonomous code-generating agents operating on production codebases will likely qualify.
- **GDPR**: The AEPD (Spain's DPA) published a 71-page framework for agentic AI in February 2026 — the controller remains legally responsible for what agents store and process.
- **SEAL Certification**: The EU's sovereign cloud certification physically excludes US hyperscalers from the highest tier.

Pulpo is sovereign by architecture, not by contract. Self-hosted means the question of "where does my code go?" has a simple answer: nowhere.

## Competitive Landscape (March 2026)

The ecosystem has 100+ agent orchestrators. Almost all are scripts or TUI wrappers. None are infrastructure-grade, and none are sovereign-first.

**Agent TUI managers** (Claude Squad 5.6k stars, AMUX, NTM 191 stars): Manage multiple agent sessions in a terminal. Single-machine, no API, no remote access, no persistence across reboots. Claude Squad uses tmux + git worktrees. AMUX is a single Python file with a watchdog.

**Cloud agent VMs** (Cursor Background Agents, Devin): Managed cloud sandboxes with web dashboards. Requires sending code to US-hosted infrastructure. Cursor is backed by $2.3B in funding. Not self-hosted, not sovereign.

**Orchestration frameworks** (Gas Town 12.4k stars, ComposioHQ): Multi-agent coordination — decompose work, assign to agents, track progress. Complementary to Pulpo, not competitive. They don't care where agents run; Pulpo doesn't care how they coordinate.

**k8s Agent Sandbox** (Kubernetes SIG Apps): gVisor-isolated pods for agent execution. Infrastructure-grade but requires Kubernetes. Google-driven.

**Agent tools** (Claude Code, Codex, Aider, Goose): The agents themselves. Claude Code now has Agent Teams for multi-agent on one machine. Codex has subagents. These handle within-session orchestration. Pulpo runs them all, competes with none.

**Pulpo's position: the only self-hosted, agent-agnostic infrastructure daemon** with session persistence, watchdog, scheduling, fleet visibility, and a mobile-first web UI. No cloud dependency. Works with any CLI agent.

## Backend Progression

Each backend serves a different scale and isolation need. The session lifecycle, watchdog, scheduler, and fleet dashboard work identically across all of them.

| Backend | When to use | Status |
|---------|------------|--------|
| **tmux** | Your laptop, your servers. Zero infrastructure. | Shipped |
| **Docker** | Same machines but isolated. Safe for `--dangerously-skip-permissions`. | Shipped |
| **Kubernetes** | Scale to a cluster. Teams with shared infrastructure. | Future (when demanded) |

## Shipped

Core infrastructure:
- `pulpod` daemon + REST API + embedded web UI (single binary)
- `pulpo` CLI with attach, spawn, resume, stop, logs, schedule, ink, secret, worktree
- SQLite-backed session persistence with full lifecycle state machine
- Session statuses: `creating`, `active`, `idle`, `ready`, `stopped`, `lost`
- Resume from `lost` and `ready` states
- Watchdog: memory pressure intervention, idle detection, ready TTL cleanup, error/failure detection
- Auto-adopt: discovers external tmux sessions and brings them under management
- Command-agnostic sessions (any CLI tool, any command)
- Inks: reusable session blueprints with command, description, secrets, and runtime defaults
- Multi-node (read-only): Tailscale peer discovery, manual peer config, fleet summary endpoint
- SSE event stream, MCP server, Discord bot, webhook notifications
- Web Push notifications for session events
- Homebrew distribution (`brew install darioblanco/tap/pulpo`)
- PWA: installable app with service worker, offline shell caching

Session features:
- Docker runtime: `pulpo spawn --runtime docker` for isolated containers
- Git worktrees: `pulpo spawn --worktree` for isolated branches per agent
- Secret store: encrypted-at-rest env vars injected into sessions
- Ink blueprints with secrets and runtime defaults
- Per-session idle threshold (`--idle-threshold`)
- Configurable waiting patterns (29 built-in + user-configurable)

Scheduling:
- DB-backed schedules with cron expressions (local timezone)
- CRUD API + CLI (`pulpo schedule add/list/pause/resume/remove`)
- Schedule execution fields: runtime, secrets, worktree, worktree_base
- Scheduler fires every 60s, creates timestamped sessions per run

Observability:
- PR/branch detection from session output (GitHub/GitLab/Bitbucket)
- Git branch/commit tracking, diff stats, commits ahead of remote
- Token and cost tracking: keyword-proximity parsing of agent output (input/output/cache tokens + cost), CLI USAGE column, web UI cost/token badges, accumulation across agent restarts
- Rate limit detection
- Enriched notifications with git state, PR URLs, error status, token/cost data
- Fleet sessions endpoint for cross-node aggregation

Developer experience:
- CLI auto-start daemon (brew services / systemd / direct spawn)
- CLI node name resolution (`pulpo --node mac-mini list`)
- Session liveness check before attach
- Ocean gamification UI

## What's Next

### Cost Tracking — Budget Limits (P5.1b)

Token and cost tracking is shipped: keyword-proximity parsing extracts tokens and dollar costs from any agent's terminal output, displayed in CLI (`pulpo list` USAGE column) and web UI (cost/token badges). Accumulation handles agent restarts within a session.

Remaining work when demanded:
- Per-session token/cost budget limits (watchdog auto-stops sessions exceeding thresholds)
- Cumulative cost per day/node
- Budget alerts via notifications

### Agent Completion Callbacks

Replace the 29 waiting-for-input patterns with a reliable signal. Inject `PULPO_CALLBACK_URL` as an environment variable into every session. Any agent (or wrapper script) can call it to signal "I'm done."

This is more reliable than pattern matching and works with any agent, including future ones we haven't seen yet.

### Distribution & Visibility

**Landing page & docs polish**
- Compelling landing page with demo video
- Real-world usage examples (nightly code review, parallel agents, scheduled migrations)
- "5-minute quickstart" that shows the value immediately

**Notification digest**
- Daily/weekly summary of agent activity (sessions completed, PRs created, costs incurred)

**Homebrew-core submission**
- Requires ≥75 GitHub stars
- Source build, `brew audit` compliance

### Multi-Node: Trim to Working Subset, Then Controller Mode

The current multi-node design is masterless — each node is independent, fleet visibility comes from on-demand HTTP polling. An honest audit revealed that the **infrastructure layer works** (Tailscale discovery, peer registry, health probing, fleet summary endpoint) but the **features built on top are broken** (web UI click-through to remote sessions 404s, CLI attach after remote spawn fails, `target_node` on schedules is ignored, cross-node auth is fragile).

**Root cause:** You can't build cross-node actions on top of independent nodes without a coordination layer. Polling gives you read-only snapshots, not the ability to interact with remote sessions.

**Phase 1 — Trim to honest working subset (immediate)**

Remove broken features that frustrate users. Keep what works:
- **Keep:** Tailscale discovery, peer registry, health probing, fleet summary endpoint
- **Keep:** CLI `--node` for read-only commands (list, logs, status) with `--detach` for remote spawn
- **Fix:** URL normalization bug (CLI `base_url` prepends `http://` to URLs with scheme)
- **Remove:** Per-peer session tabs in web UI (browser-to-peer auth/CORS broken)
- **Remove:** Fleet table click-through to remote session detail (404s)
- **Remove:** `target_node` field from schedule UI (scheduler ignores it)
- **Remove:** CLI attach after remote spawn (tmux is local-only)

**Phase 2 — Controller mode**

Inspired by Elasticsearch's cluster architecture: every node runs the same binary, one node is promoted to controller. The controller holds the session index (metadata), not the sessions themselves (which are tmux processes on managed nodes).

How it works:
- `controller.enabled = true` promotes a node to controller mode
- `controller.address = "https://..."` makes a node a managed node
- Managed nodes push session events to the controller over outbound HTTP and poll for commands (no inbound control ports needed on managed nodes)
- Controller maintains a unified session index in its SQLite
- Web UI connects to the controller only for fleet-wide visibility and cross-node actions
- Node UIs stay local-first: local sessions remain visible, but fleet-wide control belongs to the controller
- Managed nodes authenticate with per-node bearer tokens issued by the controller; `[peers]` remains routing metadata, not an authority source

Current implementation status:
- Controller-routed fleet reads, create, stop, resume, and scheduled dispatch are implemented
- Managed nodes push session lifecycle events and poll the controller for commands
- Node identity is bound to enrolled per-node tokens in both `public` and `tailscale` deployments
- Node enrollment is operable through `pulpo nodes enroll <name>` and `pulpo nodes enrolled`
- The controller session index is persisted and restored across restarts
- Node UIs expose local sessions plus a handoff to the controller, rather than a best-effort fleet view
- Distributed terminal attach remains intentionally out of scope; remote detail stays HTTP/log-oriented

Why this is simpler than Elasticsearch:
- No consensus protocol needed — losing the controller loses visibility, not data. Sessions keep running.
- No replication — sessions are ephemeral processes, not persistent data.
- Eventually consistent — 5-second delay in status propagation is fine.

Key property: **code never leaves the managed nodes.** The controller sees session metadata (names, statuses, tokens consumed) and can proxy log-style HTTP detail views, but it is not intended to become a distributed terminal multiplexer by default. This preserves Pulpo's sovereignty guarantee and keeps the control plane simpler.

### Parked Features (build when demanded)

**Configurable output matchers (P5.2)**
- User-defined regex → action rules in config.toml
- Extends hardcoded error/PR/rate-limit detection to custom patterns

**Batch manifests**
- `pulpo run manifest.yml` for coordinated overnight runs (8 agents across 4 repos with dependencies)
- Build when the scheduling use case matures

## Phase 6: Compliance & Governance

When agent infrastructure needs to be auditable.

**P6.1 — Session ownership and audit trail**
- Track who spawned each session (user identity from token)
- Audit log: who did what, when, on which node
- GDPR traceability: what agents stored, processed, and for how long

**P6.2 — Resource policies**
- Per-node session limits (max N concurrent agents)
- Memory reservation per session
- Auto-stop sessions exceeding time or cost limits

**P6.3 — Shared ink library**
- Sync inks across nodes
- Ink versioning for team-standard workflows

## Parked

Revisit when demanded by real usage, not by speculation.

- **Agent-to-agent communication** — orchestration frameworks handle this. Pulpo is infrastructure, not workflow.
- **MCP server expansion** — the existing server (12 tools, 4 resources) works. REST APIs are the primary integration surface.
- **Multi-user auth** — only if team adoption materializes.
- **Kubernetes backend** — when cluster-scale demand materializes. The Backend trait is ready.
- **Cloud VM backend** — ephemeral machines (Hetzner, AWS). Spin up for a task, tear down when done.
- **Voice-command surfaces** — parked indefinitely.

## Removed

- ~~mDNS discovery~~ — removed in v0.0.41. Near-zero usage; Tailscale discovery + manual peer config cover all real use cases.
- ~~Seed-based gossip discovery~~ — removed in v0.0.41. Same reasoning.
- ~~Provider-specific features~~ — agents handle their own capabilities.
- ~~Guard/safety rails~~ — agents have their own permission models.
- ~~Culture system~~ — agents read CLAUDE.md/AGENTS.md natively.
- ~~Per-peer session tabs in web UI~~ — browser-to-peer HTTP requires auth/CORS that the controllerless architecture can't provide. Replaced by fleet summary table (read-only).
- ~~Fleet click-through to remote session detail~~ — local API returns 404 for sessions on other nodes. Requires controller-mode proxying.
- ~~`target_node` on schedules~~ — field was stored but never read by the scheduler. Requires controller-mode job dispatch.
- ~~Smart node selection (`--auto`)~~ — scoring was naive and excluded the local node. Revisit in controller mode where the controller has real-time fleet state.

## Success Criteria

Pulpo is succeeding if:

- You spawn agents on your machines and they run reliably as background workers
- You check agent status from your phone while away from your desk
- Watchdog catches runaway agents before they burn through your API budget
- Sessions survive machine reboots and you resume them
- Multiple agents run overnight and you wake up to PRs, not crashed terminals
- Nightly code reviews and security scans run themselves
- You know exactly what each agent cost before you check your API dashboard
- Your code never leaves your infrastructure

## Architectural Principles

- Infrastructure layer, not intelligence layer
- Command-agnostic: runs any agent, any command
- Sovereign by architecture: self-hosted, no cloud dependency
- Single-node excellence first, multi-node via controller promotion
- Mobile-first web UI: the phone is the primary management surface
- Explicit failure semantics: every state transition is observable and auditable
- Zero-config local start, progressive operational depth
