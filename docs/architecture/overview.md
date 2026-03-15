# Architecture Overview

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
- **`pulpo-common`** — shared types (Session, Provider, API types) used by both crates.
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

**Creating** → **Active** ⇄ **Idle** → **Finished** → (TTL) → **Killed**

The watchdog drives transitions by monitoring terminal output, detecting agent exit markers, and enforcing memory/idle policies. See [Session Lifecycle](/operations/session-lifecycle) for the full state machine.

## Provider Abstraction

Pulpo manages agents through a provider-agnostic adapter layer. Each provider translates Pulpo's session model into the specific CLI flags and behaviors:

| Provider | Binary | Autonomous | Unrestricted | System Prompt | Model |
|----------|--------|------------|--------------|---------------|-------|
| Claude Code | `claude` | `--print` | `--dangerously-skip-permissions` | `--system-prompt` | `--model` |
| Codex | `codex` | `--quiet` | `--full-auto` | prepend to prompt | `--model` |
| Gemini CLI | `gemini` | `--sandbox=false` | — | prepend to prompt | `--model` |
| OpenCode | `opencode` | — | — | prepend to prompt | — |
| Shell | `bash` | N/A | N/A | N/A | N/A |

Provider availability is checked at spawn time via PATH detection.

## Multi-Node Architecture

Pulpo nodes discover each other and present a unified view:

- **Tailscale**: Discovers peers via local Tailscale API, serves HTTPS via `tailscale serve`
- **mDNS**: Zero-config LAN discovery via `_pulpo._tcp.local.`
- **Seed**: Bootstrap from a known peer, discover transitively
- **Manual**: Explicit peer entries in config

Each node runs independently with its own SQLite store. Session state stays local to each node — the unified view is assembled at query time by the UI/CLI.

## Data Flow

```
Session spawn → apply_defaults → resolve_ink → build_command → tmux create
       ↓                                                                           ↓
    SQLite                                                                     Agent runs
       ↓                                                                           ↓
   Watchdog ←── check output ──────────────────────────────────────────── terminal output
       ↓
  State transitions (active ⇄ idle → finished/killed/lost)
       ↓
  SSE events → web UI / Discord / webhooks
```

## Design Principles

- **Infrastructure layer, not agent intelligence** — Pulpo manages the runtime, not the prompts
- **Provider-agnostic** — same lifecycle and controls regardless of which agent you run
- **Explicit failure states** — every session is in a known, auditable state
- **Zero-config local start** — `pulpod` runs out of the box, progressive operational depth
- **No unsafe code** — `forbid(unsafe_code)` workspace-wide

For the full architecture spec, see [SPEC.md](https://github.com/darioblanco/pulpo/blob/main/SPEC.md).
