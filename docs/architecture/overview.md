# Architecture Overview

Pulpo is a self-hosted meter and breaker box for background coding agents: it runs them
as durable workers, measures exactly what each session costs, and enforces budgets before
you blow a limit.

Architecturally, the shortest accurate description is:

- `pulpod` runs and tracks sessions
- each session is a command plus durable state
- sessions run on a `tmux` backend
- the watchdog drives lifecycle transitions and interventions

Everything else in the project exists to operate that core more conveniently.

## Start Here

If you only remember one mental model, use this:

```text
command -> session -> backend -> lifecycle -> control surfaces
```

- You provide a **command**
- Pulpo creates a managed **session**
- The session runs on a **backend**
- The watchdog and liveness checks maintain the **lifecycle**
- CLI, web UI, API, and scheduler are **control surfaces**

This separation matters because Pulpo is not:

- an agent framework
- a prompt library
- a workflow orchestrator
- a special wrapper around one model vendor

It is the layer that turns agent commands into durable infrastructure objects.

## Components

```
┌─────────┐      ┌──────────┐      ┌──────────────┐
│  pulpo   │─────▶│  pulpod   │─────▶│  tmux + agent │
│  (CLI)   │ REST │  (daemon) │ spawn│  (backend)    │
└─────────┘      └──────────┘      └──────────────┘
                   │  │
          ┌────────┘  └────────┐
          ▼                    ▼
     ┌────────┐           ┌────────┐
     │ SQLite │           │  SSE   │
     │ Store  │           │ Events │
     └────────┘           └────────┘
```

- **`pulpod`** — the daemon. Owns session state, backends, watchdog, API, and persistence.
- **`pulpo`** — the CLI. A thin client over the daemon API.
- **`pulpo-common`** — shared types for sessions, nodes, and API payloads.
- **`web/`** — the embedded web UI. Useful, but conceptually a client of the daemon, not the core runtime itself.

## The Core Contract

The project becomes understandable once these terms are fixed:

### Session

A session is one managed command plus metadata:

- name
- workdir
- command
- runtime
- output snapshot
- lifecycle state
- timestamps and intervention history

### Runtime

A runtime is where that command executes. Sessions run on `tmux` — native, long-lived terminal sessions.

The lifecycle model is decoupled from the backend behind the `Backend` trait. That is the important abstraction.

### Lifecycle

Sessions move through explicit states:

`starting -> working <-> waiting -> done`

with failure paths to:

`lost`

This is the most important behavior in the system. See [Session Lifecycle](../operations/session-lifecycle.md) for exact transitions.

### Watchdog

The watchdog is the supervision loop. It is responsible for:

- detecting waiting-for-input and idle sessions (checked first on every tick, since it's
  what refreshes each session's cost metadata from the agent's own transcript)
- detecting exit markers
- enforcing per-session/per-schedule budget caps (80% alert, 100% stop), checked right
  after idle detection so it always judges this tick's own fresh cost numbers
- recording interventions

For a harness with its own lifecycle hooks (Claude Code, Codex, pi), a **harness
adapter** rewrites the spawn to wire those hooks to `pulpo hook <harness>` and the
resulting events drive the same transitions directly — the watchdog stops guessing from
scrollback for whatever signals that harness's events cover. A session blocked on the
harness (a permission prompt, a question) shows as `waiting` with a `status_reason =
needs_input:<reason>`, rendered as `waiting (needs input: <reason>)` rather than plain
`waiting (idle)`. See [Harness Adapters](harness-adapters.md).

## Control Surfaces

| Surface | Use Case |
|---------|----------|
| CLI (`pulpo`) | Terminal-first operations, scripting, cron jobs |
| Web UI | Dashboard, session inspection, settings |
| REST API | Integration with tools, automation, CI/CD |
| SSE (`/api/v1/events`) | Real-time event streaming |

These are all clients of the same session model. If one surface disappears, the core runtime is still intact.

The daemon owns the truth, and every surface reflects or operates on that same truth —
but "control plane" is deliberately not the framing (see
[Why Pulpo](../getting-started/why-pulpo.md)): there
is no cross-node orchestration, by design. Each node's `pulpod` is standalone
infrastructure that meters and governs its own sessions; reach it directly.

## Command-Based Sessions

Pulpo is command-agnostic — each session runs an arbitrary shell command. There is no built-in provider abstraction. You pass the exact command you want to run:

```bash
pulpo spawn my-task -- claude -p "fix auth tests"
pulpo spawn lint-check -- npm run lint
pulpo spawn review -- gemini "review this code"
```

## Operational Layers

These are important features, but they sit above the core runtime rather than defining it.

### Git Worktrees

`--worktree` creates an isolated git worktree for each session, so multiple agents can work on the same repo without conflicts:

```bash
pulpo spawn auth-fix --workdir ~/repo --worktree -- claude -p "fix auth"
pulpo spawn perf-fix --workdir ~/repo --worktree -- codex "optimize queries"
```

Each session gets `~/.pulpo/worktrees/<session-name>/` on a branch matching the session name. A plain `pulpo stop` leaves the worktree on disk; it's reclaimed on the next purge — `pulpo stop --purge`, `pulpo cleanup`, or a watchdog intervention (budget/idle) that stops the session. See [Worktrees](../guides/worktrees.md) for the full cleanup model.

### Built-in Scheduler

Cron-based schedules run inside `pulpod` (no crontab manipulation) and always fire on the
node that holds them. To schedule on another box, point the CLI at it with the global
`--url` connection flag — the schedule is created directly on that node's `pulpod`:

```bash
pulpo --url gpu-box schedule add nightly "0 3 * * *" -- claude -p "review"
```

Schedules are visible in the web UI dashboard at `/schedules`.

### Multi-Machine Access

> **Status (July 2026): there is no control plane.** A controller/node relay mode existed
> and was frozen (2026-06-14), then removed entirely (2026-07). Cross-node orchestration —
> remote spawn, a canonical fleet session index, controller-proxied commands — was a dead
> product lane: first parties (Claude Code Remote Control, Codex's desktop command center)
> already won that race. See the [Roadmap](https://github.com/darioblanco/pulpo/blob/main/ROADMAP.md)
> "Phase C" for the history. The peer registry and Tailscale peer discovery layered on top
> of that were removed later for the same reason: a read-only list of other nodes' sessions
> with no way to act on them wasn't worth the config surface and health-probing machinery.

Every `pulpod` is standalone. Multi-machine operation is direct, not brokered:

- **`bind = "tailscale"`** binds locally and serves HTTPS over the tailnet via
  `tailscale serve` — no port-forwarding, no public IP, Tailscale's own ACLs are the
  reachability boundary. This is transport only; it does not enumerate other nodes.
- **Direct access** is how you reach another node: `pulpo --url <host:port>` from the CLI, a
  saved connection in the web UI, or plain SSH + `pulpo attach`. Sessions and schedules are
  local to the node that runs them — nothing is proxied through a third machine.
- **Aggregated visibility**, when you want one view across machines, comes from the
  event-forwarding backbone: every node forwards events to your own collector via
  `[[webhooks]]` and exposes `/usage`, so you aggregate in Grafana/Datadog/a SIEM (or a
  single designated node) — see "Monitoring & event topology" below.

Important limits:

- there is no fleet-wide session index; each node's SQLite store is authoritative only for
  its own sessions
- the web UI shows the local node only — no cross-node table
- distributed terminal attach is intentionally out of scope; remote detail remains HTTP/log-oriented
- schedules always fire on the node that holds them; there is no remote schedule dispatch

### Monitoring & event topology (local-first invariant)

Event forwarding is **local-first, not orchestrator-routed**. Every node runs its own event
dispatcher, so the events and alerts it emits are delivered to *its own* configured
`[[webhooks]]` (and SSE) independent of any other machine. There is no central hop events
must pass through.

Consequences, by design:

- A node's own webhooks keep firing regardless of what any other node is doing — the
  dispatcher is node-local. Delivery is in-memory and best-effort (plain POST, a fixed
  retry schedule, then drop) — there is no durable outbox, so nothing survives a restart.
- To get a cross-machine view, point every node's `[[webhooks]]` at the same collector (your
  own aggregator, Grafana/Datadog/a SIEM, or a single designated node). That aggregation
  point is something you own; Pulpo does not run one for you.

**Agent callbacks point at the local node (locked invariant).** Harness adapters (see
[Harness Adapters](harness-adapters.md)) inject hooks into the agent process
that always target the **local `pulpod`** that spawned the session — never a remote
machine. The local daemon owns the session lifecycle and forwards events onward from
there. Routing agent processes at a central machine would couple every agent to that
machine's address and uptime, add a hop, and break standalone operation. Same principle
as events: **local-first, then aggregate.**

## Data Flow

```
Session spawn → resolve_command → build_command → tmux create
       ↓                                                                           ↓
    SQLite                                                                     Agent runs
       ↓                                                                           ↓
   Watchdog ←── check output ──────────────────────────────────────────── terminal output
       ↓
  State transitions (working ⇄ waiting → done/lost)
       ↓
  SSE events → web UI / webhooks
```

## Security Model

- **Network**: `pulpod` binds to `127.0.0.1` by default (`bind = "local"`). In
  `"public"` mode it binds to `0.0.0.0` and requires a bearer token on every
  `/api/v1/*` request (auto-generated on first run, retrievable locally via
  `GET /api/v1/auth/token`). In `"tailscale"` mode it stays on `127.0.0.1` and runs
  `tailscale serve` to proxy the dashboard over HTTPS on the tailnet — auth is
  delegated to Tailscale (WireGuard) instead of a token. See
  [Private Infrastructure With Tailscale](../guides/private-infra-with-tailscale.md)
  and [Config Reference](../reference/config.md) for the operational detail.
- **Auth**: in `local`/`tailscale` modes, network isolation *is* the auth layer; only
  `public` mode needs the bearer token.
- **Agents**: sessions run as the same user `pulpod` runs as — equivalent to running
  the agent directly in a terminal. The `command` field gives full control over what
  runs in the session, same as a shell would.
- **No secrets in the API**: the API never exposes API keys. Credentials live in each
  agent's own config/environment, on each node; `pulpod` does not read or store them
  (see ADR [0006](../adr/0006-exact-metering-and-flat-budget-cap-only.md) for why it
  stopped reading agent credential files at all).

## Stable vs Experimental

The most stable part of the project is:

- daemon-managed sessions
- the tmux runtime
- lifecycle states
- watchdog supervision
- CLI/API/web UI access to that state

Useful but more secondary:

- Tailscale bind (remote transport)
- schedules
- worktrees
- notifications

Experimental or convenience-oriented:

- themed presentation surfaces

## Backend Abstraction

All session operations go through a `Backend` trait. The session lifecycle, watchdog, scheduler, and web UI work identically regardless of backend:

| Backend | Use case | Backend ID format |
|---------|----------|-------------------|
| **tmux** (only backend today) | Local/remote servers, zero infrastructure | `$0`, `$1`, ... |

Adding a new backend means implementing ~10 methods (`create_session`, `kill_session`, `is_alive`, `capture_output`, etc.) — everything above the backend layer (lifecycle states, watchdog, scheduler, web UI) works unchanged. A Kubernetes (or other cluster) backend is a hypothetical illustration of that extension point, not a roadmap item — it's explicitly parked (see [ROADMAP.md](https://github.com/darioblanco/pulpo/blob/main/ROADMAP.md) "Parked"), and a containerized *agent* runtime was tried and removed (containerized agents hide their session files from the exact-usage-metering readers).

## Design Principles

- **Runtime first** — the session model matters more than any one surface
- **Infrastructure layer, not agent intelligence** — Pulpo manages execution, not prompting strategy
- **Command-agnostic** — the same lifecycle applies regardless of command
- **Explicit failure states** — every session is in a known, auditable state
- **Zero-config local start** — `pulpod` runs out of the box, with optional operational depth
- **No unsafe code** — `forbid(unsafe_code)` workspace-wide

This page, [Core Concepts](core-concepts.md), and [Harness Adapters](harness-adapters.md)
are the maintained architecture reference; for the strategic narrative (the bet, what
shipped, what was cut) see [ROADMAP.md](https://github.com/darioblanco/pulpo/blob/main/ROADMAP.md)
and the [Architecture Decision Records](https://github.com/darioblanco/pulpo/blob/main/docs/adr/README.md).
