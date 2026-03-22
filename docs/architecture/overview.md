# Architecture Overview

Pulpo is an agent session runtime. The shortest accurate description is:

- `pulpod` runs and tracks sessions
- each session is a command plus durable state
- sessions run on a backend: `tmux` or `docker`
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
- CLI, web UI, API, scheduler, and fleet features are **control surfaces**

This separation matters because Pulpo is not:

- an agent framework
- a prompt library
- a workflow orchestrator
- a special wrapper around one model vendor

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

A runtime is where that command executes:

- `tmux` for native long-lived terminal sessions
- `docker` for containerized execution

The lifecycle model is shared across runtimes. That is the important abstraction.

### Lifecycle

Sessions move through explicit states:

`creating -> active <-> idle -> ready`

with failure or intervention paths to:

`killed` or `lost`

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
| MCP (`pulpod mcp`) | Agent-to-agent integration via Model Context Protocol |
| Discord bot | Remote session control from Discord |

These are all clients of the same session model. If one surface disappears, the core runtime is still intact.

## Command-Based Sessions

Pulpo is command-agnostic — each session runs an arbitrary shell command. There is no built-in provider abstraction. You pass the exact command you want to run:

```bash
pulpo spawn my-task -- claude -p "fix auth tests"
pulpo spawn lint-check -- npm run lint
pulpo spawn review -- gemini "review this code"
```

Inks provide reusable command templates (see [Configuration Guide](/guides/configuration)).

## Operational Layers

These are important features, but they sit above the core runtime rather than defining it.

### Git Worktrees

`--worktree` creates an isolated git worktree for each session, so multiple agents can work on the same repo without conflicts:

```bash
pulpo spawn auth-fix --workdir ~/repo --worktree -- claude -p "fix auth"
pulpo spawn perf-fix --workdir ~/repo --worktree -- codex "optimize queries"
```

Each session gets `<repo>/.pulpo/worktrees/<session-name>/` on branch `pulpo/<session-name>`. Worktrees are cleaned up when sessions are killed or deleted.

### Built-in Scheduler

Cron-based schedules run inside `pulpod` (no crontab manipulation). Schedules support multi-node targeting:

```bash
pulpo schedule add nightly "0 3 * * *" --node gpu-box -- claude -p "review"
pulpo schedule add scan "0 0 * * 0" --node auto -- claude -p "security audit"
```

`--node auto` picks the least-loaded online peer at fire time. Schedules are visible in the web UI dashboard at `/schedules`.

### Multi-Node Architecture

Pulpo nodes discover each other and present a unified view:

- **Tailscale**: Discovers peers via local Tailscale API, serves HTTPS via `tailscale serve`
- **mDNS**: Zero-config LAN discovery via `_pulpo._tcp.local.`
- **Seed**: Bootstrap from a known peer, discover transitively
- **Manual**: Explicit peer entries in config

Each node runs independently with its own SQLite store. Session state stays local to each node — the unified view is assembled at query time by the UI/CLI.

## Data Flow

```
Session spawn → resolve_ink → build_command → tmux create
       ↓                                                                           ↓
    SQLite                                                                     Agent runs
       ↓                                                                           ↓
   Watchdog ←── check output ──────────────────────────────────────────── terminal output
       ↓
  State transitions (active ⇄ idle → ready/killed/lost)
       ↓
  SSE events → web UI / Discord / webhooks
```

## Runtime Details

### Docker Runtime

`--runtime docker` runs sessions in Docker containers instead of tmux. The workdir is mounted at `/workspace`, and any configured Docker volumes are mounted too.

```bash
# Safe for unrestricted agent execution
pulpo spawn risky-task --runtime docker -- claude --dangerously-skip-permissions -p "refactor"
```

The `DockerBackend` implements the same `Backend` trait as tmux, using `docker` CLI commands:
- `create_session` → `docker run -d --name pulpo-<name> -v <workdir>:/workspace ...`
- `capture_output` → `docker logs --tail N`
- `is_alive` → `docker inspect --format '{{.State.Running}}'`
- `kill_session` → `docker stop + docker rm`

Configure the Docker image in `config.toml`:
```toml
[docker]
image = "my-agents-image:latest"  # must have agent tools installed
```

Sessions are identified by `backend_session_id` prefix: `$N` for tmux, `docker:pulpo-<name>` for Docker. The session manager dispatches to the correct backend automatically.

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
- tmux/docker runtimes
- lifecycle states
- watchdog supervision
- CLI/API/web UI access to that state

Useful but more secondary:

- fleet discovery
- schedules
- worktrees
- secrets and notifications

Experimental or convenience-oriented:

- Discord bot
- MCP server
- themed presentation surfaces

## Backend Abstraction

All session operations go through a `Backend` trait. The session lifecycle, watchdog, scheduler, and fleet dashboard work identically regardless of backend:

| Backend | Use case | Backend ID format |
|---------|----------|-------------------|
| **tmux** (default) | Local/remote servers, zero infrastructure | `$0`, `$1`, ... |
| **Docker** (`--runtime docker`) | Isolated containers, safe for unrestricted agents | `docker:pulpo-<name>` |
| **Kubernetes** (future) | Cluster scale, team infrastructure | — |

Adding a new backend means implementing ~10 methods (`create_session`, `kill_session`, `is_alive`, `capture_output`, etc.). Everything above the backend layer — lifecycle states, watchdog, scheduler, fleet, web UI — works unchanged.

## Design Principles

- **Runtime first** — the session model matters more than any one surface
- **Infrastructure layer, not agent intelligence** — Pulpo manages execution, not prompting strategy
- **Command-agnostic** — the same lifecycle applies regardless of command
- **Explicit failure states** — every session is in a known, auditable state
- **Adopts existing work** — Pulpo can manage sessions it did not originally spawn
- **Zero-config local start** — `pulpod` runs out of the box, with optional operational depth
- **No unsafe code** — `forbid(unsafe_code)` workspace-wide

For the full architecture spec, see [SPEC.md](https://github.com/darioblanco/pulpo/blob/main/SPEC.md).
