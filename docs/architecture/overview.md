# Architecture Overview

Pulpo is a self-hosted control plane for background coding agents.

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
- **`pulpo-common`** — shared types for sessions, nodes, peers, and API payloads.
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

`creating -> active <-> idle -> ready`

with failure or intervention paths to:

`stopped` or `lost`

This is the most important behavior in the system. See [Session Lifecycle](/operations/session-lifecycle) for exact transitions.

### Watchdog

The watchdog is the supervision loop. It is responsible for:

- detecting waiting-for-input and idle sessions
- detecting exit markers
- enforcing ready TTL cleanup
- recording interventions
- adopting external tmux sessions when enabled

## Control Surfaces

| Surface | Use Case |
|---------|----------|
| CLI (`pulpo`) | Terminal-first operations, scripting, cron jobs |
| Web UI | Dashboard, session inspection, settings |
| REST API | Integration with tools, automation, CI/CD |
| SSE (`/api/v1/events`) | Real-time event streaming |

These are all clients of the same session model. If one surface disappears, the core runtime is still intact.

This is why "control plane" is the right framing: the daemon owns the truth,
and every surface reflects or operates on that same truth.

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

Each session gets `~/.pulpo/worktrees/<session-name>/` on a branch matching the session name. Worktrees and their branches are cleaned up when sessions are stopped.

### Built-in Scheduler

Cron-based schedules run inside `pulpod` (no crontab manipulation) and always fire on the
node that holds them. To schedule on another box, point the CLI at it with the global
`--node` connection flag — the schedule is created directly on that node's `pulpod`:

```bash
pulpo --node gpu-box schedule add nightly "0 3 * * *" -- claude -p "review"
```

Schedules are visible in the web UI dashboard at `/schedules`.

### Multi-Node Architecture

> **Status (July 2026): there is no control plane.** A controller/node relay mode existed
> and was frozen (2026-06-14), then removed entirely (2026-07). Cross-node orchestration —
> remote spawn, a canonical fleet session index, controller-proxied commands — was a dead
> product lane: first parties (Claude Code Remote Control, Codex's desktop command center)
> already won that race. See the [Roadmap](https://github.com/darioblanco/pulpo/blob/main/ROADMAP.md)
> "Phase C" for the history.

Every `pulpod` is standalone. Multi-machine operation is direct, not brokered:

- **Discovery** tells nodes about each other — **Tailscale** (peers discovered via the local
  Tailscale API, HTTPS served via `tailscale serve`) or **manual** `[peers]` entries — but a
  discovered peer is just a name-to-address mapping, not a subordinate. See the
  [Discovery Guide](/guides/discovery).
- **Direct access** is how you reach another node: `pulpo --node <name|host:port>` from the
  CLI (resolves through the local peer registry), a saved connection in the web UI, or plain
  SSH + `pulpo attach`. Sessions, schedules, and secrets are local to the node that runs them
  — nothing is proxied through a third machine.
- **Aggregated visibility**, when you want one view across machines, comes from the
  event-forwarding backbone: every node forwards signed events to your own collector via
  `[[webhooks]]` and exposes `/metrics` + `/usage`, so you aggregate in Grafana/Datadog/a SIEM
  (or a single designated node) — see "Monitoring & event topology" below.

Important limits:

- there is no fleet-wide session index; each node's SQLite store is authoritative only for
  its own sessions
- the web UI shows the local node only — no fleet tabs or cross-node table
- distributed terminal attach is intentionally out of scope; remote detail remains HTTP/log-oriented
- schedules always fire on the node that holds them; there is no remote schedule dispatch

### Monitoring & event topology (local-first invariant)

Event forwarding is **local-first, not orchestrator-routed**. Every node runs its own event
dispatcher and durable webhook outbox, so the events and alerts it emits are delivered to
*its own* configured `[[webhooks]]` (and web-push / SSE) independent of any other machine.
There is no central hop events must pass through.

Consequences, by design:

- A node's own webhooks keep firing regardless of what any other node is doing — the
  dispatcher and the durable outbox are node-local.
- To get a cross-machine view, point every node's `[[webhooks]]` at the same collector (your
  own aggregator, Grafana/Datadog/a SIEM, or a single designated node). That aggregation
  point is something you own; Pulpo does not run one for you.

**Agent callbacks point at the local node (locked invariant).** When agent-side hooks /
completion callbacks land (e.g. an injected callback URL), they target the **local
`pulpod`** that spawned the session — never a remote machine. The local daemon owns the
session lifecycle and forwards events onward from there. Routing agent processes at a
central machine would couple every agent to that machine's address and uptime, add a hop,
and break standalone operation. Same principle as events: **local-first, then aggregate.**

## Data Flow

```
Session spawn → resolve_command → build_command → tmux create
       ↓                                                                           ↓
    SQLite                                                                     Agent runs
       ↓                                                                           ↓
   Watchdog ←── check output ──────────────────────────────────────────── terminal output
       ↓
  State transitions (active ⇄ idle → ready/stopped/lost)
       ↓
  SSE events → web UI / webhooks
```

## Runtime Details

### tmux Session Adoption

Pulpo doesn't require you to use `pulpo spawn`. Start tmux however you want — `tmux new-session`, scripts, other tools — and the watchdog discovers and adopts those sessions automatically:

- Classifies adopted sessions into Pulpo lifecycle states
- Captures the full command line (not just process name) for accurate resume
- Uses tmux's internal `$N` session IDs, so killing and re-creating sessions with the same name works correctly
- Tags adopted sessions with `PULPO_SESSION_ID` and `PULPO_SESSION_NAME` env vars

This is enabled by default (`adopt_tmux = true` in watchdog config).

## Stable vs Experimental

The most stable part of the project is:

- daemon-managed sessions
- the tmux runtime
- lifecycle states
- watchdog supervision
- CLI/API/web UI access to that state

Useful but more secondary:

- peer discovery
- schedules
- worktrees
- secrets and notifications

Experimental or convenience-oriented:

- themed presentation surfaces

## Backend Abstraction

All session operations go through a `Backend` trait. The session lifecycle, watchdog, scheduler, and web UI work identically regardless of backend:

| Backend | Use case | Backend ID format |
|---------|----------|-------------------|
| **tmux** (default) | Local/remote servers, zero infrastructure | `$0`, `$1`, ... |
| **Kubernetes** (future) | Cluster scale, team infrastructure | — |

Adding a new backend means implementing ~10 methods (`create_session`, `kill_session`, `is_alive`, `capture_output`, etc.). Everything above the backend layer — lifecycle states, watchdog, scheduler, web UI — works unchanged.

## Design Principles

- **Runtime first** — the session model matters more than any one surface
- **Infrastructure layer, not agent intelligence** — Pulpo manages execution, not prompting strategy
- **Command-agnostic** — the same lifecycle applies regardless of command
- **Explicit failure states** — every session is in a known, auditable state
- **Adopts existing work** — Pulpo can manage sessions it did not originally spawn
- **Zero-config local start** — `pulpod` runs out of the box, with optional operational depth
- **No unsafe code** — `forbid(unsafe_code)` workspace-wide

For the full architecture spec, see [SPEC.md](https://github.com/darioblanco/pulpo/blob/main/SPEC.md).
