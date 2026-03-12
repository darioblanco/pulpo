# Architecture Overview

## Components

```
┌─────────┐      ┌──────────┐      ┌──────────────┐
│  pulpo   │─────▶│  pulpod   │─────▶│  tmux + agent │
│  (CLI)   │ REST │  (daemon) │ spawn│  (backend)    │
└─────────┘      └──────────┘      └──────────────┘
                   │  │  │
          ┌────────┘  │  └────────┐
          ▼           ▼           ▼
     ┌────────┐  ┌────────┐  ┌────────┐
     │ SQLite │  │ Culture│  │  SSE   │
     │ Store  │  │  Repo  │  │ Events │
     └────────┘  └────────┘  └────────┘
```

- **`pulpod`** — daemon runtime. Axum HTTP server, session manager, watchdog, culture repo, peer discovery. Embeds the web UI via `rust-embed`.
- **`pulpo`** — CLI client. Thin HTTP client that talks to `pulpod`'s REST API.
- **`pulpo-common`** — shared types (Session, Provider, Culture, API types) used by both crates.
- **`web/`** — React 19 + Vite + Tailwind v4 + shadcn/ui SPA. Includes an ocean-themed dashboard with pixel art octopus sprites.

## Control Surfaces

| Surface | Use Case |
|---------|----------|
| CLI (`pulpo`) | Terminal-first operations, scripting, cron jobs |
| Web UI | Dashboard, session inspection, culture management |
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

## Culture System

The culture system enables collective learning across sessions and nodes:

1. **Format**: AGENTS.md-formatted markdown files in scoped directories (`culture/`, `repos/<slug>/`, `inks/<ink>/`)
2. **Write-back**: Agents write `pending/<session>.md` files during their session
3. **Harvest**: On session completion, pending files are validated (length, duplication, code-only rejection) and committed
4. **Lifecycle**: Entries have relevance scores with age decay and reference boost. Stale entries are excluded from compilation. Entries can be superseded, approved, or rejected.
5. **Sync**: Background pull loop with rebase-first conflict resolution. Fire-and-forget push after commits.
6. **Injection**: Compiled AGENTS.md content is injected into new sessions as context.

## Multi-Node Architecture

Pulpo nodes discover each other and present a unified view:

- **Tailscale**: Discovers peers via local Tailscale API, serves HTTPS via `tailscale serve`
- **mDNS**: Zero-config LAN discovery via `_pulpo._tcp.local.`
- **Seed**: Bootstrap from a known peer, discover transitively
- **Manual**: Explicit peer entries in config

Each node runs independently with its own SQLite store and culture repo. Culture syncs via git remote. Session state stays local to each node — the unified view is assembled at query time by the UI/CLI.

## Data Flow

```
Session spawn → apply_defaults → resolve_ink → inject_culture → build_command → tmux create
       ↓                                                                           ↓
    SQLite                                                                     Agent runs
       ↓                                                                           ↓
   Watchdog ←── check output ──────────────────────────────────────────── terminal output
       ↓
  State transitions (active ⇄ idle → finished/killed/lost)
       ↓
  Culture harvest → validate → dedup → commit → push
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
