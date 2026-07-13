# Pulpo — the self-hosted meter and breaker box for coding agents

> _Eight arms, one brain — see and control what every agent costs, on infrastructure you own._
>
> Last verified against code: 2026-06-14

Pulpo is a lightweight daemon that runs coding-agent sessions as durable background
workers, **measures exactly what each one costs** (across agents, accounts, and
machines), **enforces budgets**, and **forwards alerts and events to your own
observability stack**. It abstracts `tmux` behind a clean API and ships a
mobile-friendly web UI. Sovereign by architecture: usage and account data are read
from local files and never relayed to a vendor.

It is **not** an agent framework, a prompt tool, or a terminal-orchestration UX —
modern agents handle interactive worktrees, sandboxing, and guardrails themselves.
Pulpo is the layer they lack: usage telemetry, cost control, and monitoring on
infrastructure you own. See [ROADMAP.md](ROADMAP.md) for the full positioning.

## Problem

Coding agents have become background workers — and a quota-and-cost multiplier. A
few in parallel can burn a weekly subscription allowance in an afternoon. The tools
that could warn you won't: a vendor's `/usage` is one account, one machine, one
vendor, shown after the fact; no vendor will aggregate spend across *your* accounts
(it would help you arbitrage their limits); and only the thing actually running the
session can stop a runaway before the wall. Meanwhile your usage and account data are
exactly what you'd least want flowing through a third-party relay.

## Goals

1. **Single binary** (`pulpod`) runs on each machine as a daemon (embedded web UI)
2. **Exact usage metering** — read tokens/cost from each agent's own session files,
   attributed per session and rolled up per account/pool and per repo
3. **Cost control** — per-session/schedule budget caps (alert 80%, stop 100%) and a
   burn-velocity governor; alert-first, opt-in auto-stop
4. **Monitoring backbone** — signed canonical events to multiple webhooks (durable
   outbox + backoff + HMAC) and a toggleable Prometheus `/metrics` endpoint
5. **Durable sessions** — explicit lifecycle that survives reboots; `tmux` backend;
   per-session git worktrees; watchdog supervision
6. **Sovereign** — self-hosted, local-only account data, Tailscale transport for
   private remote access; **open source** (MIT or Apache 2.0)

## Non-Goals

- **Cross-node agent orchestration** — a controller/node control plane existed, was frozen,
  then removed (July 2026; see Roadmap "Phase C"). Every `pulpod` is standalone, reached
  directly (`pulpo --node <name>`); cross-node visibility is the event backbone (forward to
  your own collector), not a bespoke controller.
- Optimizing the **inference path** (prompt caching, per-request routing, context
  trimming) — that's the agent's job; Pulpo optimizes the *operation* of agents
- Agent-to-agent communication; custom model hosting/serving
- Multi-user / team features (single-user, your tailnet)
- Defining the "best" preset catalog or prompting methodology — a preset registry
  (`inks`) existed and was removed (July 2026; see Roadmap "Removed"); command/secrets
  live directly on sessions and schedules
- Replacing specialized local agent UX tools, or becoming an all-in-one platform

---

## Architecture

```
  ┌───────────┐  ┌───────────┐  ┌─────────────┐
  │  Browser  │  │    CLI    │  │ Any REST/   │
  │  (phone/  │  │  (pulpo)  │  │ SSE client  │
  │  laptop)  │  │           │  │             │
  └─────┬─────┘  └─────┬─────┘  └──────┬──────┘
        │ REST/WS       │ REST          │ REST/SSE
        └───────────────┼───────────────┘
                        │
     ┌──────────────────┼──────────────────────────┐
     │                  │                           │
  ┌──▼─────────┐  ┌────▼───────┐  ┌────────────────▼──────────────┐
  │  mac-mini  │  │  macbook   │  │  Docker (container deploy)    │
  │  pulpod    │  │  pulpod    │  │  ┌───────────┐ ┌───────────┐  │
  │  ┌──────┐  │  │  ┌──────┐  │  │  │ tailscale │ │  pulpod   │  │
  │  │ tmux │  │  │  │ tmux │  │  │  │ sidecar   │ │  agents   │  │
  │  └──────┘  │  │  └──────┘  │  │  │  :443 ────┼─┤  :7433    │  │
  │  ┌──────┐  │  │  ┌──────┐  │  │  └───────────┘ │  ┌──────┐ │  │
  │  │SQLite│  │  │  │SQLite│  │  │   shared netns  │  │ tmux │ │  │
  │  └──────┘  │  │  └──────┘  │  │                 │  └──────┘ │  │
  └────────────┘  └────────────┘  │                 │  ┌──────┐ │  │
                                  │                 │  │SQLite│ │  │
                                  │                 │  └──────┘ │  │
                                  │                 └───────────┘  │
                                  └────────────────────────────────┘
  ◄─── bare-metal (bind=tailscale) ───►  ◄── container (bind=container) ──►
       runs TS discovery loop                 sidecar handles tailnet
```

### Components

#### 1. `pulpod` — The Daemon (Rust)

Runs on every machine. Responsibilities:

- **Session lifecycle**: create, list, attach, stop, resume sessions
- **Terminal backend**: manages tmux sessions (macOS/Linux)
- **API server**: REST + WebSocket on a configurable port (default: 7433)
- **Persistence**: SQLite for session state, output snapshots, conversation IDs
- **Node info**: reports machine capabilities (OS, CPU, RAM, GPU)
- **Peer discovery**: finds other `pulpod` instances on the Tailnet

#### 2. `pulpo` — The CLI (Rust)

Thin CLI client that talks to the local (or remote) `pulpod` API. For when you
want to manage sessions from a terminal instead of the web UI.

```bash
# Local usage (talks to local pulpod)
pulpo spawn my-api --workdir ~/repos/my-api -- claude "Fix the auth bug"
pulpo list
pulpo logs my-api
pulpo stop my-api
pulpo resume my-api         # resume lost, ready, or stopped session (auto-attaches)
pulpo nodes                 # list all pulpod peers on the Tailnet
pulpo list --node server    # list sessions on a remote node

# Remote usage (talks to remote pulpod)
pulpo --node server spawn ml-train --workdir ~/repos/ml-model -- claude "Train it"
```

#### 3. Web UI

Embedded in the `pulpod` binary (static assets compiled in). Mobile-first design.

**Views:**

- **Dashboard**: sessions at a glance, with status filtering
- **Session detail**: live terminal output, input field, metadata (incl. per-session cost/tokens)
- **Usage**: cost/burn gauge — account cards + per-session table (the meter)
- **Schedules**: cron schedule management
- **Settings**: node config, peer management
- **Ocean**: gamified canvas view (frozen — no new investment)

---

## Session Lifecycle

```
  spawn           agent working        agent exits
    │                   │                   │
    ▼                   ▼                   ▼
┌────────┐       ┌──────────┐        ┌──────────┐
│CREATING│──────▶│  ACTIVE  │───────▶│  READY   │
└────────┘       └──────────┘        └──────────┘
                   ▲      │                │
            output │      │ waiting        │ TTL / user
           changed │      │ for input      ▼
                   │      ▼          ┌──────────┐
                   │ ┌──────────┐    │ STOPPED  │
                   └─│   IDLE   │    └──────────┘
                     └──────────┘          ▲
                                           │ watchdog / user
                     ┌──────────┐
                     │   LOST   │◀── tmux disappeared
                     └──────────┘
```

> Full lifecycle reference: [`docs/operations/session-lifecycle.md`](docs/operations/session-lifecycle.md)

### States

- **CREATING**: tmux session is being set up
- **ACTIVE**: agent is working — terminal output is changing
- **IDLE**: agent needs attention — waiting for user input or at its prompt
- **READY**: agent process exited — task is done. Detected by `[pulpo] Agent exited` marker
- **STOPPED**: session was terminated by user, watchdog (memory/idle), or ready TTL cleanup
- **LOST**: tmux process disappeared with no exit markers (crash, reboot, external kill mid-run). A session whose shell exited normally (exit markers present) resolves to STOPPED instead — exiting a session is a clean end, not a loss.

### State Quick Reference

| Status     | Meaning                         | How it happens                    | What to do next                 |
| ---------- | ------------------------------- | --------------------------------- | ------------------------------- |
| `creating` | tmux session being set up       | `pulpo spawn <name>` just ran     | Wait (auto-attached)            |
| `active`   | Agent is working                | Session started / output changed  | `logs`, `attach`, `stop`        |
| `idle`     | Agent waiting for input         | Watchdog detected waiting pattern | `attach` to interact, or `stop` |
| `ready`    | Agent exited                    | `[pulpo] Agent exited` detected   | `resume`                        |
| `stopped`  | Session terminated              | User, watchdog, or TTL cleanup    | `spawn` new                     |
| `lost`     | tmux process disappeared        | Daemon restart / reboot / crash   | `resume` (auto-attaches)        |

Key distinctions:
- **Idle** is a live state — the agent process is running but waiting. **Ready** means the agent exited.
- **Ready** is resumable (restarts the agent). **Stopped** is not resumable (requires fresh `spawn`).
- **Lost** means the tmux process is gone but may be recoverable via `resume`.

### Persistence (what survives a reboot)

Stored in `~/.pulpo/state.db` (SQLite):

| Field             | Description                                             |
| ----------------- | ------------------------------------------------------- |
| `id`              | UUID                                                    |
| `name`            | Human-readable session name (default: workdir basename) |
| `workdir`         | Absolute path to the working directory                  |
| `command`         | Shell command executed in the session                   |
| `description`     | Optional human-readable description                     |
| `status`          | `creating`, `active`, `idle`, `ready`, `stopped`, `lost` |
| `exit_code`       | Process exit code (null if still running)               |
| `worktree_branch` | Git branch name for worktree sessions (null if no worktree) |
| `backend_session_id`    | Backend-specific session identifier                     |
| `output_snapshot` | Last N lines of terminal output                         |
| `created_at`      | Timestamp                                               |
| `updated_at`      | Timestamp                                               |

### Output Capture

The daemon periodically runs `tmux capture-pane` on watchdog ticks to grab the
current terminal content and stores it in the DB. This means:

- The web UI can show recent output even without a live WebSocket connection
- After a reboot, you can see what the agent was doing before it died
- Log files are also written to `~/.pulpo/logs/<session-id>.log` via
  `tmux pipe-pane`

### Interventions

An **intervention** is any time pulpo forcibly acts on a session — stopping it due to resource pressure, idle timeout, or another watchdog-detected condition. Every intervention is recorded in the `intervention_events` table with:

- **session_id** — which session was affected
- **reason** — human-readable cause (e.g. "Memory 95% exceeded threshold 90%", "Idle for 600s")
- **created_at** — when the intervention happened

The session itself also stores the most recent intervention in `intervention_reason` and `intervention_at` fields, so you can see at a glance whether a session was intervened on.

**What triggers an intervention:**

- **Memory pressure** — the watchdog checks system memory usage every `check_interval_secs`. If usage exceeds `memory_threshold` for `breach_count` consecutive checks, the highest-memory session is stopped.
- **Idle timeout** — if a session produces no output for `idle_timeout_secs`, the watchdog acts based on `idle_action`: `"alert"` logs a warning, `"stop"` terminates the session.

**How to inspect interventions:**

- CLI: `pulpo interventions <name>` (alias: `iv`)
- API: `GET /api/v1/sessions/:id/interventions`
### Failure & Recovery

Two recovery flows cover the common failure modes:

#### 1. Reboot / crash → lost → resume

When `pulpod` starts, it attempts to auto-resume sessions that were `active` or `idle` before the restart. If the backend session is gone and Pulpo cannot recreate it immediately, later liveness checks mark the session **lost**.

```
Machine reboots → pulpod starts → tries to auto-resume prior active/idle sessions
                                                      │
                                                      └─ if backend is still gone, session becomes LOST
                                                                                          │
User runs: pulpo resume <name> ────────────────────────────────────────────────────────────┘
    → recreates the backend session if needed
    → re-executes the saved command
    → session goes to ACTIVE
```

`resume` works for `lost` and `ready` sessions. `stopped` sessions require a fresh `spawn`.

#### 2. Watchdog stop → stopped → manual spawn

The watchdog stops a session and records an intervention. The session stays `stopped` — the user decides whether to `spawn` a new session.

```
Watchdog detects issue → stops session → records intervention → session is STOPPED
    └─ user runs: pulpo spawn ... (fresh session)
```

**Relevant config knobs** (`[watchdog]` in `~/.pulpo/config.toml`):

| Key                   | Default   | Description                                     |
| --------------------- | --------- | ----------------------------------------------- |
| `memory_threshold`    | `90`      | Stop when system memory usage exceeds this %    |
| `check_interval_secs` | `10`      | How often to check (seconds)                    |
| `breach_count`        | `3`       | Consecutive breaches before acting              |
| `idle_timeout_secs`   | `600`     | Seconds of no output before idle action         |
| `idle_action`         | `"alert"` | `"alert"` (log warning) or `"stop"` (terminate) |

### Troubleshooting

| Symptom                                   | Likely cause                    | Fix                                                       |
| ----------------------------------------- | ------------------------------- | --------------------------------------------------------- |
| Session stuck in `creating`               | tmux failed to start            | Check `tmux -V` (need 3.2+), check logs                   |
| Session is `lost` after reboot            | Backend session is gone         | `pulpo resume <name>`                                     |
| Session is `stopped`, wasn't manual       | Watchdog or prior failure path  | Check `pulpo interventions <name>`, then `spawn` new      |
| `resume` fails with "cannot be resumed"   | Session is still active/idle or was stopped | Use `pulpo spawn` or wait for the running session |
| Watchdog keeps stopping sessions          | Memory threshold too low        | Raise `memory_threshold` or reduce concurrent sessions    |
| No output in `pulpo logs`                 | Session just started            | Wait, or use `--follow` to stream: `pulpo logs -f <name>` |

---

## Terminal Backend

Direct tmux management on macOS and Linux:

```
pulpod
  └─▶ tmux new-session -d -s <session-name> -c <workdir>
       └─▶ <command>  (e.g. claude, codex, gemini, or any shell command)
```

- Output streaming: `tmux pipe-pane` to a log file + periodic `capture-pane`
- Input: `tmux send-keys -t <session-name> "text" Enter`
- Attach (web): WebSocket ↔ PTY bridge that connects to the tmux session

---

## Peer Discovery

### Phase 1: Manual Configuration

`~/.pulpo/config.toml`:

```toml
[node]
name = "mac-mini"         # This node's display name
port = 7433

[peers]
# Other pulpod instances on your Tailnet
macbook = "macbook:7433"
server  = "server:7433"
```

### Phase 2: Tailscale Auto-Discovery

Query the Tailscale local API to find peers:

```
GET http://127.0.0.1:41112/localapi/v0/status
```

This returns all devices on the Tailnet. The daemon probes each peer on the
known port (7433) to check if `pulpod` is running. No manual config needed.

### API Between Nodes

Each `pulpod` exposes the same REST API. The web UI (served by one node) fans
out requests to all known peers:

```
GET /api/v1/sessions          → local sessions
GET /api/v1/node              → local node info
```

The web UI aggregates these by calling each peer's API.

---

## API Design

Base URL: `http://<tailscale-hostname>:7433/api/v1`

### Sessions

```
POST   /sessions              Create a new session
GET    /sessions              List all sessions
GET    /sessions/:id          Get session details
POST   /sessions/:id/stop     Stop a session (status → stopped)
POST   /sessions/:id/resume   Resume a lost or ready session
POST   /sessions/:id/input    Send input to the session terminal
GET    /sessions/:id/output   Get recent output (polling)
WS     /sessions/:id/stream   Stream terminal output (WebSocket)
```

#### POST /sessions

```json
{
  "name": "my-api",
  "workdir": "/home/user/repos/my-api",
  "command": "claude 'Fix the auth bug in login.py'",
  "description": "Fix auth bug",
  "metadata": { "ticket": "AUTH-123" },
  "worktree": true,
  "worktree_base": "main",
  "budget_cost_usd": 5.0
}
```

`name` is required. All other fields are optional. `workdir` defaults to the
user's home directory, `command` defaults to `node.default_command` or an
interactive shell. `worktree_base` specifies the branch to fork from (implies
`worktree: true`). `budget_cost_usd` sets a cost budget — the watchdog alerts
at 80% and stops the session at 100%.

#### GET /sessions

```json
[
  {
    "id": "a1b2c3d4-...",
    "name": "my-api",
    "workdir": "/home/user/repos/my-api",
    "command": "claude 'Fix the auth bug in login.py'",
    "description": "Fix auth bug",
    "status": "active",
    "output_snapshot": "Analyzing login.py...\nFound issue in validate_token()...",
    "created_at": "2026-02-16T10:30:00Z",
    "updated_at": "2026-02-16T10:35:00Z"
  }
]
```

`ink` also appears on historical `Session` rows created before the ink registry was
removed (July 2026); it is never set for new sessions.

The full `Session` object includes additional nullable fields: `exit_code`,
`backend_session_id`, `worktree_branch`, `metadata`, `intervention_code`,
`intervention_reason`, `intervention_at`, `last_output_at`, `idle_since`.

### Node

```
GET    /node                  Node info (hostname, OS, memory, platform)
```

### Peers

```
GET    /peers                 List known peers and their status
POST   /peers                 Add a peer
DELETE /peers/:name           Remove a peer
```

### Events

```
GET    /events                SSE event stream
```

`/events` emits tagged SSE events:
- `event: session` — session lifecycle updates (`creating`, `active`, `idle`, `ready`, `stopped`, `lost`)

### Quick Reference

| Method   | Path                            | Description                    |
| -------- | ------------------------------- | ------------------------------ |
| `GET`    | `/health`                       | Health check (no auth)         |
| `GET`    | `/sessions`                     | List all sessions              |
| `POST`   | `/sessions`                     | Create a new session           |
| `GET`    | `/sessions/:id`                 | Get session details            |
| `POST`   | `/sessions/:id/stop`            | Stop a session (status → stopped) |
| `POST`   | `/sessions/:id/resume`          | Resume a lost or ready session |
| `POST`   | `/sessions/:id/input`           | Send input to the terminal     |
| `GET`    | `/sessions/:id/output`          | Get recent output              |
| `GET`    | `/sessions/:id/output/download` | Download full output           |
| `GET`    | `/sessions/:id/interventions`   | List intervention events       |
| `WS`     | `/sessions/:id/stream`          | Stream terminal output         |
| `GET`    | `/node`                         | Node info                      |
| `GET`    | `/peers`                        | List known peers               |
| `POST`   | `/peers`                        | Add a peer                     |
| `DELETE` | `/peers/:name`                  | Remove a peer                  |
| `GET`    | `/config`                       | Get daemon config              |
| `PUT`    | `/config`                       | Update daemon config           |
| `GET`    | `/auth/token`                   | Get auth token (local only)    |
| `GET`    | `/auth/pairing-url`             | Get QR pairing URL (local)     |
| `GET`    | `/events`                       | SSE event stream               |

---

## Web UI Design

**Stack:** React 19 + Vite + Tailwind CSS v4 + shadcn/ui. Built as a
static SPA, embedded into the `pulpod` binary via `rust-embed`.
Single binary to distribute — no separate web server needed.

### Layout (Mobile-First)

```
┌─────────────────────────────┐
│  pulpo            ⚙ Settings │
├─────────────────────────────┤
│                             │
│  ● mac-mini (2 running)    │
│  ┌─────────────────────┐   │
│  │ ● my-api            │──▶│
│  │   Fix auth   2h ago │   │
│  ├─────────────────────┤   │
│  │ ○ docs              │──▶│
│  │   Update API  done  │   │
│  └─────────────────────┘   │
│                             │
│  ● server (1 running)      │
│  ┌─────────────────────┐   │
│  │ ● ml-model          │──▶│
│  │   Train      3h ago │   │
│  └─────────────────────┘   │
│                             │
│  ○ macbook (offline)        │
│                             │
│        [ + New Session ]    │
│                             │
└─────────────────────────────┘
```

**Session Detail View:**

```
┌─────────────────────────────┐
│  ← my-api         ● running│
├─────────────────────────────┤
│  mac-mini · 2h              │
│  "Fix the auth bug"        │
├─────────────────────────────┤
│                             │
│  ┌───────────────────────┐  │
│  │ $ claude              │  │
│  │                       │  │
│  │ I'll analyze the auth │  │
│  │ module...             │  │
│  │                       │  │
│  │ Reading login.py...   │  │
│  │                       │  │
│  │ Found the issue in    │  │
│  │ validate_token():     │  │
│  │ the JWT expiry check  │  │
│  │ uses < instead of <=  │  │
│  │                       │  │
│  └───────────────────────┘  │
│                             │
│  ┌───────────────────────┐  │
│  │ Type here...      Send│  │
│  └───────────────────────┘  │
│                             │
│  [Stop] [Detach] [Resume]  │
└─────────────────────────────┘
```

---

## Project Structure

See [CLAUDE.md](CLAUDE.md) for the full, maintained project layout. Key directories:

```
pulpo/
├── crates/
│   ├── pulpod/src/             # Daemon: Axum API, tmux backend, SQLite, watchdog,
│   │   ├── api/                #   REST API, SSE
│   │   ├── backend/            #   tmux.rs — terminal backend
│   │   ├── session/            #   manager, state machine, output capture, PTY bridge
│   │   ├── store/              #   SQLite persistence + migrations
│   │   ├── notifications/      #   webhook + web-push notifiers
│   │   ├── peers/              #   PeerRegistry + health probing
│   │   └── discovery/          #   Tailscale peer discovery
│   ├── pulpo-cli/src/          # CLI: thin client, clap commands
│   └── pulpo-common/src/       # Shared types: Session, NodeInfo, PeerInfo,
│                               #   SessionEvent, API request/response
├── web/                        # React 19 + Vite + Tailwind v4 + shadcn/ui
```

---

## Rust Crate Dependencies (key ones)

| Crate                  | Purpose                                             |
| ---------------------- | --------------------------------------------------- |
| `axum`                 | HTTP/WebSocket server                               |
| `tokio`                | Async runtime                                       |
| `sqlx`                 | SQLite (async, compile-time checked queries)        |
| `serde` / `serde_json` | Serialization                                       |
| `clap`                 | CLI argument parsing                                |
| `rust-embed`           | Embed web UI static files in binary                 |
| `tokio-tungstenite`    | WebSocket support                                   |
| `reqwest`              | HTTP client (for peer communication, Tailscale API) |
| `tracing`              | Structured logging                                  |
| `uuid`                 | Session IDs                                         |
| `toml`                 | Config file parsing                                 |

---

## MVP Scope (Phase 1)

Ship the smallest useful thing first.

### In Scope

- [x] `pulpod` daemon with REST API (no WebSocket yet)
- [x] tmux backend (macOS/Linux only)
- [x] SQLite persistence for session state
- [x] Output capture via `tmux capture-pane` (polling)
- [x] `pulpo` CLI: spawn, list, stop, logs
- [x] Web UI: dashboard + session list + output viewer (polling, no live terminal)
- [x] Single-node only (no peer discovery)
- [x] Command-agnostic sessions (any shell command)

### Out of Scope (Phase 2+)

- [x] WebSocket streaming + live terminal
- [x] Multi-node peer discovery
- [x] Session resume after reboot
- [x] In-app + desktop notifications (Notification API)
- [x] Installable mobile app (PWA + Web Push; native Tauri builds retired June 2026)

---

## Phase Roadmap

### Phase 1: Single-Node MVP ✅

- `pulpod` + `pulpo` CLI + basic web UI
- macOS/Linux, tmux, Claude Code only
- Polling-based output, no live terminal
- **Goal**: replace `ssh + tmux` with `pulpo spawn` + phone web UI

### Phase 2: Live Terminal + Persistence ✅

- WebSocket streaming with the embedded terminal view
- Full interactive terminal in the web UI
- Session resume after reboot
- Output log files via `tmux pipe-pane`

### Phase 3: Multi-Node ✅

- Manual peer configuration via `[peers]` in config
- Aggregated dashboard across all nodes
- Remote session spawning from any node's UI

### Phase 4: Command-Agnostic Sessions ✅

- ✅ Command-agnostic session model (any shell command instead of provider enum)
- ~~Inks simplified to description + command~~ — inks themselves removed July 2026 (see Phase 8, Roadmap "Removed")

### Phase 5: Web UI ✅

- ✅ React 19 + Vite + Tailwind CSS v4 + shadcn/ui
- ✅ Responsive dashboard, history, settings pages
- ✅ Static SPA embedded in `pulpod` binary via `rust-embed`

### Phase 5b: Desktop App UX Features ✅

**Deliverables:**

- ✅ Config API (`GET/PUT /api/v1/config`) with hot-reload and restart detection
- ✅ Settings view with tabbar navigation (Node, Peers)
- ✅ Session list filtering (`status`, `search`, `sort`, `order` query params)
- ✅ Session output download endpoint (`GET /api/v1/sessions/{id}/output/download`)
- ✅ Session history view with search/filter bar
- ✅ Chat view (Messages/Messagebar) with Terminal toggle
- ✅ In-app toast + desktop Notification API for session status changes
- ✅ Peer add/remove API (`POST /api/v1/peers`, `DELETE /api/v1/peers/{name}`)
- ✅ Peer management in settings view (list, add, remove with status indicators)

### Phase 6: Mobile + Notifications

**Stack:** PWA (installable web app + service worker) + Web Push

The mobile surface is the embedded web UI, installable as a PWA on iOS and
Android. Native Tauri builds and the voice-command experiments (formerly
Phase 7) were retired in June 2026: the PWA plus Web Push covers remote
monitoring without app-store distribution overhead, and the phone remains
the primary management surface.

**Deliverables:**

- ✅ Token authentication + bind modes (local/public/container)
- ✅ QR code pairing for mobile clients
- ✅ Tailscale auto-discovery
- ✅ PWA install + Web Push notifications
- ~~Tauri iOS/Android native builds~~ — retired June 2026 in favor of the PWA
- ~~Voice commands (Siri Shortcuts / Google Assistant)~~ — retired June 2026

### Phase 8: Control Plane + Notifications ✅

- ✅ Flexible session model (command, description, metadata)
- ✅ SSE event stream (`GET /api/v1/events`, broadcast channel, SessionEvent)
- ✅ Generic webhook notifications (`[[notifications.webhooks]]` config) + Web Push
- ~~Ink config (`[inks.name]` preset registry, `GET/POST/PUT/DELETE /api/v1/inks`)~~ — removed July 2026: command/secrets set directly per session/schedule; budget moved onto schedules (`--budget-cost`)
- ~~Discord webhook notifier (`[notifications.discord]` config)~~ — removed June 2026: use `[[notifications.webhooks]]`
- ~~Discord bot (`contrib/discord-bot/`)~~ — removed June 2026
- ~~MCP server (session management as MCP tools)~~ — removed June 2026: REST API is the primary integration surface

---

## Configuration

`~/.pulpo/config.toml`:

```toml
[node]
name = "mac-mini"       # Display name (default: hostname)
port = 7433             # API port (default: 7433)
bind = "local"          # "local", "tailscale", "public", or "container"

[auth]
# token is auto-generated on first run (only used with bind = "public")

[watchdog]
enabled = true
memory_threshold = 90
check_interval_secs = 10
breach_count = 3
idle_timeout_secs = 600
idle_action = "alert"       # "alert" or "stop"

[peers]
macbook = "macbook:7433"
server = "hetzner:7433"

[[notifications.webhooks]]
name = "primary"
url = "https://example.com/hooks/pulpo"
events = ["active", "ready", "stopped"]   # optional filter; omit for all events
```

---

## Security Model

- **Network**: `pulpod` binds to `127.0.0.1` by default (`local` mode). In `public`
  mode, it binds to `0.0.0.0` and requires bearer token authentication on all
  `/api/v1/*` requests. In `tailscale` mode, it binds to `127.0.0.1` and automatically
  runs `tailscale serve` to proxy the dashboard over HTTPS on the tailnet — accessible
  at `https://<machine-name>.<tailnet>.ts.net`. Auth is delegated to Tailscale
  (WireGuard). The serve rule is cleaned up on shutdown and stale rules from crashes
  are cleared on startup. In `container` mode, it binds to `0.0.0.0` but skips auth
  (trusts container network isolation).
- **Auth**: In `local` and `tailscale` modes, network isolation is the auth layer.
  In `public` mode, a base64url token is auto-generated on first run and required
  in every request. Retrieve it locally via `GET /api/v1/auth/token`. In `container`
  mode, auth is disabled — the container runtime provides isolation.
- **Agents**: agents run as your user (same as running Claude Code directly).
  The `command` field gives full control over what runs in the session.
- **No secrets in the API**: the API never exposes API keys. Keys are in the
  environment or config files on each node. The daemon passes them through to
  the agent process.

### Remote Access via Tailscale

The recommended way to run multi-node pulpo is `bind = "tailscale"`. This
automatically runs `tailscale serve` to proxy pulpod over HTTPS on your tailnet,
enables automatic peer discovery via the Tailscale API, and skips token auth
(WireGuard provides encryption and identity):

```toml
[node]
name = "mac-mini"
bind = "tailscale"
```

On startup, pulpod runs `tailscale serve --bg --https=443 http://127.0.0.1:{port}`
and logs the HTTPS URL (e.g., `https://mac-mini.tailnet-name.ts.net`). On shutdown
(or Ctrl+C), it runs `tailscale serve off` to clean up. Stale serve rules from a
previous crash are also cleared on startup.

Use `public` bind mode only when you need direct LAN access without Tailscale
(e.g., devices not on the tailnet). Use `container` bind mode for Docker/Podman
deployments where the container runtime provides network isolation.

### Container Deployment with Tailscale Sidecar

For containerized pulpo nodes on the tailnet, use the Tailscale sidecar pattern
(see `docker/compose/tailscale.yml`). The agents container uses `bind = "container"`
(binds `0.0.0.0`, no auth) and shares a network namespace with a
`tailscale/tailscale` sidecar that handles tailnet identity and `tailscale serve`.

**Why not `bind = "tailscale"` in containers?** The `tailscale` bind mode spawns
`tailscale status --json` for peer discovery and runs `tailscale serve` for HTTPS
exposure. In the sidecar pattern, the `tailscale` CLI lives in the sidecar container,
not the agents container. The sidecar handles networking; the agents container trusts
its network boundary. Bare-metal pulpod nodes running `bind = "tailscale"` discover
container peers via their own Tailscale discovery loop — the container doesn't need
to discover anyone.

See `docker/README.md` for full setup instructions, architecture diagram, and
troubleshooting guide.

---

## Open Questions (Resolved)

1. **License**: Dual MIT / Apache-2.0 (both license files in repo root).
2. **Binary distribution**: GitHub Actions CI builds and tests on every push. `draft-release.yml` creates draft releases; `release.yml` publishes tagged releases with pre-built binaries for macOS (aarch64) and Linux (x86_64).
3. **Tailscale dependency**: Optional enhancement, not required. Core works on localhost/LAN. Tailscale makes multi-node seamless but isn't a hard dependency.
4. **Web UI bundling**: Embedded in binary via `rust-embed` — single binary distribution. Dev mode uses Vite dev server with API proxy.
5. **tmux version requirements**: Minimum tmux 3.2+. Checked at daemon startup with a clear error message if too old or not installed.
