# Architecture Overview

Pulpo is an agent session runtime — it runs coding agents in tmux sessions or Docker containers, with lifecycle management, crash recovery, watchdog supervision, and multi-node operations. Designed for coding agents but flexible enough for any long-running terminal work.

**What makes it unique**: No other tool combines multi-node session orchestration with agent-aware lifecycle management and dual backends (tmux + Docker sandbox). tmuxinator manages layouts, overmind runs Procfiles, cmux wraps Claude — Pulpo is the infrastructure layer that makes any session durable, observable, and manageable across machines.

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

- **`pulpod`** — daemon runtime. Axum HTTP server, session manager, watchdog, peer discovery. Embeds the web UI via `rust-embed`.
- **`pulpo`** — CLI client. Thin HTTP client that talks to `pulpod`'s REST API.
- **`pulpo-common`** — shared types (Session, API types) used by both crates.
- **`web/`** — React 19 + Vite + Tailwind v4 + shadcn/ui SPA. Includes an ocean-themed dashboard with pixel art octopus sprites.

## Control Surfaces

| Surface | Use Case |
|---------|----------|
| CLI (`pulpo`) | Terminal-first operations, scripting, cron jobs |
| Web UI | Dashboard, session inspection, settings |
| REST API | Integration with tools, automation, CI/CD |
| SSE (`/api/v1/events`) | Real-time event streaming |
| MCP (`pulpod mcp`) | Agent-to-agent integration via Model Context Protocol |
| Discord bot | Remote session control from Discord |

## Session Lifecycle

Sessions move through explicit states with clear transitions:

**Creating** → **Active** ⇄ **Idle** → **Ready** → (TTL) → **Killed**

The watchdog drives transitions by monitoring terminal output, detecting agent exit markers, and enforcing memory/idle policies. See [Session Lifecycle](/operations/session-lifecycle) for the full state machine.

## Command-Based Sessions

Pulpo is command-agnostic — each session runs an arbitrary shell command. There is no built-in provider abstraction. You pass the exact command you want to run:

```bash
pulpo spawn my-task -- claude -p "fix auth tests"
pulpo spawn lint-check -- npm run lint
pulpo spawn review -- gemini "review this code"
```

Inks provide reusable command templates (see [Configuration Guide](/guides/configuration)).

## Git Worktrees

`--worktree` creates an isolated git worktree for each session, so multiple agents can work on the same repo without conflicts:

```bash
pulpo spawn auth-fix --workdir ~/repo --worktree -- claude -p "fix auth"
pulpo spawn perf-fix --workdir ~/repo --worktree -- codex "optimize queries"
```

Each session gets `<repo>/.pulpo/worktrees/<session-name>/` on branch `pulpo/<session-name>`. Worktrees are cleaned up when sessions are killed or deleted.

## Built-in Scheduler

Cron-based schedules run inside `pulpod` (no crontab manipulation). Schedules support multi-node targeting:

```bash
pulpo schedule add nightly "0 3 * * *" --node gpu-box -- claude -p "review"
pulpo schedule add scan "0 0 * * 0" --node auto -- claude -p "security audit"
```

`--node auto` picks the least-loaded online peer at fire time. Schedules are visible in the web UI dashboard at `/schedules`.

## Multi-Node Architecture

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

## Docker Sandbox

`--sandbox` runs sessions in Docker containers instead of tmux. The workdir is mounted at `/workspace` — the agent can read and write code but can't touch the host system.

```bash
# Safe for unrestricted agent execution
pulpo spawn risky-task --sandbox -- claude --dangerously-skip-permissions -p "refactor"
```

The `DockerBackend` implements the same `Backend` trait as tmux, using `docker` CLI commands:
- `create_session` → `docker run -d --name pulpo-<name> -v <workdir>:/workspace ...`
- `capture_output` → `docker logs --tail N`
- `is_alive` → `docker inspect --format '{{.State.Running}}'`
- `kill_session` → `docker stop + docker rm`

Configure the sandbox image in `config.toml`:
```toml
[sandbox]
image = "my-agents-image:latest"  # must have agent tools installed
```

Sessions are identified by `backend_session_id` prefix: `$N` for tmux, `docker:pulpo-<name>` for Docker. The session manager dispatches to the correct backend automatically.

## tmux Session Adoption

Pulpo doesn't require you to use `pulpo spawn`. Start tmux however you want — `tmux new-session`, scripts, other tools — and the watchdog discovers and adopts those sessions automatically:

- Classifies running agents (claude, codex, gemini) as **Active**, shells as **Ready**
- Captures the full command line (not just process name) for accurate resume
- Uses tmux's internal `$N` session IDs, so killing and re-creating sessions with the same name works correctly
- Tags adopted sessions with `PULPO_SESSION_ID` and `PULPO_SESSION_NAME` env vars

This is enabled by default (`adopt_tmux = true` in watchdog config).

## Backend Abstraction

All session operations go through a `Backend` trait. The session lifecycle, watchdog, scheduler, and fleet dashboard work identically regardless of backend:

| Backend | Use case | Backend ID format |
|---------|----------|-------------------|
| **tmux** (default) | Local/remote servers, zero infrastructure | `$0`, `$1`, ... |
| **Docker** (`--sandbox`) | Isolated containers, safe for unrestricted agents | `docker:pulpo-<name>` |
| **Kubernetes** (future) | Cluster scale, team infrastructure | — |

Adding a new backend means implementing ~10 methods (`create_session`, `kill_session`, `is_alive`, `capture_output`, etc.). Everything above the backend layer — lifecycle states, watchdog, scheduler, fleet, web UI — works unchanged.

## Design Principles

- **Universal agent runtime** — run agents anywhere: tmux, Docker, or future backends. Same lifecycle, same controls.
- **Infrastructure layer, not agent intelligence** — Pulpo manages the runtime, not the prompts
- **Command-agnostic** — same lifecycle and controls regardless of which command you run
- **Explicit failure states** — every session is in a known, auditable state
- **Adopts existing work** — start tmux however you want, pulpo manages it
- **Zero-config local start** — `pulpod` runs out of the box, progressive operational depth
- **No unsafe code** — `forbid(unsafe_code)` workspace-wide

For the full architecture spec, see [SPEC.md](https://github.com/darioblanco/pulpo/blob/main/SPEC.md).
