# Pulpo — Agent Session Orchestrator

> _Eight arms, one brain — orchestrating agents across your network._
>
> Last verified against code: 2026-03-03

Pulpo is a lightweight daemon that manages coding agent sessions (Claude Code,
Codex) across multiple machines on a trusted network (LAN, VPN, or Tailscale).
It abstracts tmux management behind a clean API, and provides a mobile-friendly
web UI for orchestrating agents from your phone or laptop.

## Problem

You have multiple machines (Macs, Linux servers) connected via Tailscale. You want
to spawn, monitor, and manage coding agents on any of them from your phone or
laptop. Today this requires: Termius -> SSH -> tmux attach -> navigate windows.
Too many layers. And if a machine reboots, you lose your session state.

## Goals

1. **Single binary** (`pulpod`) runs on each machine as a daemon
2. **Abstracts tmux** (macOS/Linux) behind a unified session API
3. **Web UI** served by the daemon — mobile-first, works great on iPhone Safari
4. **Multi-node** — discover and manage sessions across all your Tailscale machines
   from one dashboard
5. **Session persistence** — survive reboots by storing conversation IDs, prompts,
   output snapshots, and git state in a local database
6. **Open source** — MIT or Apache 2.0

## Non-Goals (for now)

- Agent-to-agent communication
- Custom AI model hosting/serving
- CI/CD integration (use GitHub Actions separately)
- Multi-user / team features (single-user, your Tailnet)
- Defining the "best" persona catalog or prompting methodology
- Replacing specialized local agent UX tools
- Becoming a monolithic all-in-one agent platform

---

## Architecture

```
  ┌───────────┐  ┌───────────┐  ┌─────────────┐
  │  Browser  │  │    CLI    │  │ Discord Bot │
  │  (phone/  │  │  (pulpo)  │  │ (contrib/)  │
  │  laptop)  │  │           │  │             │
  └─────┬─────┘  └─────┬─────┘  └──────┬──────┘
        │ REST/WS       │ REST          │ REST/SSE
        └───────────────┼───────────────┘
                        │
     ┌──────────────────┼──────────────────┐
     │                  │                  │
  ┌──▼─────────┐  ┌────▼───────┐  ┌───────▼────┐
  │  mac-mini  │  │  macbook   │  │   server   │
  │  pulpod    │  │  pulpod    │  │   pulpod   │
  │  ┌──────┐  │  │  ┌──────┐  │  │  ┌──────┐  │
  │  │ tmux │  │  │  │ tmux │  │  │  │ tmux │  │
  │  └──────┘  │  │  └──────┘  │  │  └──────┘  │
  │  ┌──────┐  │  │  ┌──────┐  │  │  ┌──────┐  │
  │  │SQLite│  │  │  │SQLite│  │  │  │SQLite│  │
  │  └──────┘  │  │  └──────┘  │  │  └──────┘  │
  └────────────┘  └────────────┘  └────────────┘
```

### Components

#### 1. `pulpod` — The Daemon (Rust)

Runs on every machine. Responsibilities:

- **Session lifecycle**: create, list, attach, kill, resume sessions
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
pulpo spawn --workdir ~/repos/my-api --provider claude "Fix the auth bug"
pulpo list
pulpo logs my-api
pulpo kill my-api
pulpo resume my-api         # after reboot, resume Claude conversation
pulpo nodes                 # list all pulpod peers on the Tailnet
pulpo list --node server    # list sessions on a remote node

# Remote usage (talks to remote pulpod)
pulpo --node server spawn --workdir ~/repos/ml-model --provider claude "Train it"
```

#### 3. Web UI

Embedded in the `pulpod` binary (static assets compiled in). Mobile-first design.

**Views:**

- **Dashboard**: all nodes, all sessions, at a glance
- **Session detail**: live terminal output (xterm.js), input field, metadata
- **History**: session history with search/filter
- **Settings**: node config, guard presets, peer management

---

## Session Lifecycle

```
  spawn          running           done/interrupted
    │               │                    │
    ▼               ▼                    ▼
┌────────┐    ┌──────────┐    ┌───────────────────┐
│CREATING│───▶│ RUNNING  │───▶│ COMPLETED / DEAD  │
└────────┘    └──────────┘    └───────────────────┘
                   │                    │
                   │    reboot/crash    │
                   ▼                    │
              ┌──────────┐             │
              │  STALE   │─── resume ──┘
              └──────────┘
```

### States

- **CREATING**: tmux session is being set up
- **RUNNING**: agent is active, terminal output is streaming
- **COMPLETED**: agent exited cleanly (exit code 0)
- **DEAD**: agent exited with error or was killed
- **STALE**: the daemon restarted and found a session record in SQLite but no
  matching tmux session — the machine rebooted or tmux crashed. The user can
  "resume" which creates a new tmux session and runs the agent with
  `--resume <conversation-id>` (Claude Code)

### State Quick Reference

| Status      | Meaning                       | How it happens               | What to do next               |
| ----------- | ----------------------------- | ---------------------------- | ----------------------------- |
| `creating`  | tmux session being set up     | `pulpo spawn` just ran       | Wait                          |
| `running`   | Agent is active               | Session started successfully | `logs`, `attach`, `kill`      |
| `completed` | Agent exited cleanly (exit 0) | Task finished                | `delete` or keep for history  |
| `dead`      | Agent crashed or was killed   | Error, `kill`, or watchdog   | `spawn` new or `delete`       |
| `stale`     | DB record but no tmux session | Daemon restart / reboot      | `resume`                      |

Key distinction: **stale** means the session record exists but the tmux process is gone (recoverable via `resume`). **Dead** means the process exited with an error or was killed (requires a fresh `spawn`).

### Persistence (what survives a reboot)

Stored in `~/.pulpo/state.db` (SQLite):

| Field             | Description                                             |
| ----------------- | ------------------------------------------------------- |
| `id`              | UUID                                                    |
| `name`            | Human-readable session name (default: workdir basename) |
| `workdir`         | Absolute path to the working directory                  |
| `provider`        | `claude`, `codex`                                       |
| `prompt`          | The original prompt/task description                    |
| `conversation_id` | Claude Code conversation ID (from ~/.claude/)           |
| `status`          | `creating`, `running`, `completed`, `dead`, `stale`     |
| `exit_code`       | Process exit code (null if still running)               |
| `tmux_session`    | tmux session name                                       |
| `output_snapshot` | Last N lines of terminal output                         |
| `git_branch`      | Branch name at session start                            |
| `git_sha`         | Commit SHA at session start                             |
| `created_at`      | Timestamp                                               |
| `updated_at`      | Timestamp                                               |

### Output Capture

The daemon periodically (every 5s) runs `tmux capture-pane` to grab the
current terminal content and stores it in the DB. This means:

- The web UI can show recent output even without a live WebSocket connection
- After a reboot, you can see what the agent was doing before it died
- Log files are also written to `~/.pulpo/logs/<session-id>.log` via
  `tmux pipe-pane`

### Interventions

An **intervention** is any time pulpo forcibly acts on a session — killing it due to resource pressure, idle timeout, or another watchdog-detected condition. Every intervention is recorded in the `intervention_events` table with:

- **session_id** — which session was affected
- **reason** — human-readable cause (e.g. "Memory 95% exceeded threshold 90%", "Idle for 600s")
- **created_at** — when the intervention happened

The session itself also stores the most recent intervention in `intervention_reason` and `intervention_at` fields, so you can see at a glance whether a session was intervened on.

**What triggers an intervention:**

- **Memory pressure** — the watchdog checks system memory usage every `check_interval_secs`. If usage exceeds `memory_threshold` for `breach_count` consecutive checks, the highest-memory session is killed.
- **Idle timeout** — if a session produces no output for `idle_timeout_secs`, the watchdog acts based on `idle_action`: `"alert"` logs a warning, `"kill"` terminates the session.

**How to inspect interventions:**

- CLI: `pulpo interventions <name>` (alias: `iv`)
- API: `GET /api/v1/sessions/:id/interventions`
### Failure & Recovery

Two recovery flows cover the common failure modes:

#### 1. Reboot / crash → stale → resume

When `pulpod` starts, it checks SQLite for sessions that were `running` or `creating`. For each one, it looks for a matching tmux session. If the tmux session is gone (machine rebooted, tmux crashed), it marks the session **stale**.

```
Machine reboots → pulpod starts → finds session in DB → no tmux → marks STALE
                                                                        │
User runs: pulpo resume <name> ─────────────────────────────────────────┘
    → creates new tmux session
    → passes --resume <conversation-id> to the agent
    → session goes CREATING → RUNNING (conversation context preserved)
```

`resume` **only works for stale sessions**. Dead sessions require a fresh `spawn`.

#### 2. Watchdog kill → dead → manual spawn

The watchdog kills a session and records an intervention. The session stays dead — the user decides whether to `spawn` a new session.

```
Watchdog detects issue → kills session → records intervention → session is DEAD
    └─ user runs: pulpo spawn ... (fresh session)
```

**Relevant config knobs** (`[watchdog]` in `~/.pulpo/config.toml`):

| Key                   | Default   | Description                                     |
| --------------------- | --------- | ----------------------------------------------- |
| `memory_threshold`    | `90`      | Kill when system memory usage exceeds this %    |
| `check_interval_secs` | `10`      | How often to check (seconds)                    |
| `breach_count`        | `3`       | Consecutive breaches before acting              |
| `idle_timeout_secs`   | `600`     | Seconds of no output before idle action         |
| `idle_action`         | `"alert"` | `"alert"` (log warning) or `"kill"` (terminate) |

### Troubleshooting

| Symptom                                   | Likely cause                    | Fix                                                       |
| ----------------------------------------- | ------------------------------- | --------------------------------------------------------- |
| Session stuck in `creating`               | tmux failed to start            | Check `tmux -V` (need 3.2+), check logs                   |
| Session is `stale` after reboot           | Expected — tmux session is gone | `pulpo resume <name>`                                     |
| Session is `dead`, wasn't killed          | Agent crashed or OOM            | Check `pulpo interventions <name>`, then `spawn` new      |
| `resume` fails with "not stale"           | Session is dead, not stale      | Use `pulpo spawn` to start fresh                          |
| Watchdog keeps killing sessions           | Memory threshold too low        | Raise `memory_threshold` or reduce concurrent sessions    |
| No output in `pulpo logs`                 | Session just started            | Wait, or use `--follow` to stream: `pulpo logs -f <name>` |

---

## Terminal Backend

Direct tmux management on macOS and Linux:

```
pulpod
  └─▶ tmux new-session -d -s pulpo-<session-name> -c <workdir>
       └─▶ claude --dangerously-skip-permissions  (or codex --full-auto)
```

- Output streaming: `tmux pipe-pane` to a log file + periodic `capture-pane`
- Input: `tmux send-keys -t pulpo-<session-name> "text" Enter`
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
POST   /sessions/:id/kill     Kill a session (status → dead)
DELETE /sessions/:id          Permanently remove a session from history
POST   /sessions/:id/resume   Resume a stale session
POST   /sessions/:id/input    Send input to the session terminal
GET    /sessions/:id/output   Get recent output (polling)
WS     /sessions/:id/stream   Stream terminal output (WebSocket)
```

#### POST /sessions

```json
{
  "name": "my-api",
  "workdir": "/home/user/repos/my-api",
  "provider": "claude",
  "prompt": "Fix the auth bug in login.py",
  "mode": "interactive",
  "guard_preset": "standard",
  "model": "sonnet",
  "system_prompt": "Focus on security",
  "allowed_tools": ["Read", "Glob", "Grep"],
  "metadata": { "discord_channel_id": "123456" },
  "persona": "reviewer"
}
```

All fields except `workdir` and `prompt` are optional. `mode` is
`"interactive"` (default) or `"autonomous"`. If `persona` is set, its config
is used as defaults; explicit fields override persona values.

#### GET /sessions

```json
[
  {
    "id": "a1b2c3d4-...",
    "name": "my-api",
    "workdir": "/home/user/repos/my-api",
    "provider": "claude",
    "prompt": "Fix the auth bug in login.py",
    "status": "running",
    "mode": "interactive",
    "model": "sonnet",
    "persona": "reviewer",
    "guard_config": { "...": "..." },
    "output_snapshot": "Analyzing login.py...\nFound issue in validate_token()...",
    "git_branch": "main",
    "git_sha": "abc1234",
    "recovery_count": 0,
    "created_at": "2026-02-16T10:30:00Z",
    "updated_at": "2026-02-16T10:35:00Z"
  }
]
```

The full `Session` object includes additional nullable fields: `conversation_id`,
`exit_code`, `tmux_session`, `allowed_tools`, `system_prompt`, `metadata`,
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

### Personas & Events

```
GET    /personas              List configured personas
GET    /events                SSE event stream (session lifecycle events)
```

`/events` emits tagged events:
- `kind: "session"` for session lifecycle updates (`creating`, `running`, `completed`, `dead`, `stale`)

### Quick Reference

| Method   | Path                            | Description                    |
| -------- | ------------------------------- | ------------------------------ |
| `GET`    | `/health`                       | Health check (no auth)         |
| `GET`    | `/sessions`                     | List all sessions              |
| `POST`   | `/sessions`                     | Create a new session           |
| `GET`    | `/sessions/:id`                 | Get session details            |
| `POST`   | `/sessions/:id/kill`            | Kill a session (status → dead) |
| `DELETE` | `/sessions/:id`                 | Permanently remove a session   |
| `POST`   | `/sessions/:id/resume`          | Resume a stale session         |
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
| `GET`    | `/personas`                     | List configured personas       |
| `GET`    | `/events`                       | SSE event stream               |

---

## Web UI Design

**Stack:** Svelte 5 + SvelteKit + Tailwind CSS v4 + Konsta UI v5. Built as a
static SPA (`adapter-static`), embedded into the `pulpod` binary via `rust-embed`.
Single binary to distribute — no separate web server needed.

### Layout (Mobile-First)

```
┌─────────────────────────────┐
│  pulpo            ⚙ Settings │
├─────────────────────────────┤
│                             │
│  ● mac-mini (2 running)    │
│  ┌─────────────────────┐   │
│  │ ● my-api    claude  │──▶│
│  │   Fix auth   2h ago │   │
│  ├─────────────────────┤   │
│  │ ○ docs      codex   │──▶│
│  │   Update API  done  │   │
│  └─────────────────────┘   │
│                             │
│  ● server (1 running)      │
│  ┌─────────────────────┐   │
│  │ ● ml-model  claude  │──▶│
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
│  claude · mac-mini · 2h    │
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
│  [Kill] [Detach] [Resume]  │
└─────────────────────────────┘
```

---

## Project Structure

See [CLAUDE.md](CLAUDE.md) for the full, maintained project layout. Key directories:

```
pulpo/
├── crates/
│   ├── pulpod/src/             # Daemon: Axum API, tmux backend, SQLite, watchdog,
│   │   ├── api/                #   MCP server, mDNS, guard enforcement, SSE, personas
│   │   ├── backend/            #   tmux.rs — terminal backend
│   │   ├── session/            #   manager, state machine, output capture, PTY bridge
│   │   ├── store/              #   SQLite persistence + migrations
│   │   ├── notifications/      #   Discord webhook notifier
│   │   ├── peers/              #   PeerRegistry + health probing
│   │   ├── discovery/          #   mDNS service discovery
│   │   └── mcp/                #   MCP server (session tools as MCP resources)
│   ├── pulpo-cli/src/          # CLI: thin client, clap commands
│   └── pulpo-common/src/       # Shared types: Session, Provider, NodeInfo, PeerInfo,
│                               #   GuardConfig, SessionEvent, API request/response
├── web/                        # Svelte 5 + SvelteKit + Tailwind v4 + Konsta UI v5
└── contrib/discord-bot/        # Discord bot: slash commands + SSE listener
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
- [x] `pulpo` CLI: spawn, list, kill, logs
- [x] Web UI: dashboard + session list + output viewer (polling, no live terminal)
- [x] Single-node only (no peer discovery)
- [x] Claude Code provider only

### Out of Scope (Phase 2+)

- [x] WebSocket streaming + live terminal (xterm.js attach)
- [x] Multi-node peer discovery
- [x] Codex provider support
- [x] Environment sanitization + sandbox profiles
- [x] Session resume after reboot
- [x] In-app + desktop notifications (Notification API)
- [x] iOS native app (Tauri 2 + TestFlight)

---

## Phase Roadmap

### Phase 1: Single-Node MVP ✅

- `pulpod` + `pulpo` CLI + basic web UI
- macOS/Linux, tmux, Claude Code only
- Polling-based output, no live terminal
- **Goal**: replace `ssh + tmux` with `pulpo spawn` + phone web UI

### Phase 2: Live Terminal + Persistence ✅

- WebSocket streaming with xterm.js
- Full interactive terminal in the web UI
- Session resume after reboot (STALE -> RUNNING)
- Output log files via `tmux pipe-pane`

### Phase 3: Multi-Node ✅

- Manual peer configuration via `[peers]` in config
- Aggregated dashboard across all nodes
- Remote session spawning from any node's UI

### Phase 4: Sandboxing ✅

- ✅ Codex provider support
- ✅ Guard presets (standard/strict/unrestricted) with per-provider flags

### Phase 5: Web UI + Konsta UI ✅

- ✅ Konsta UI migration (iOS-native look, responsive phone/tablet/desktop)
- ✅ Svelte 5 + SvelteKit + Tailwind CSS v4 + Konsta UI v5
- ✅ Static SPA embedded in `pulpod` binary via `rust-embed`

### Phase 5b: Desktop App UX Features ✅

**Deliverables:**

- ✅ Config API (`GET/PUT /api/v1/config`) with hot-reload and restart detection
- ✅ Settings view with tabbar navigation (Node, Guards, Peers)
- ✅ Session list filtering (`status`, `provider`, `search`, `sort`, `order` query params)
- ✅ Session output download endpoint (`GET /api/v1/sessions/{id}/output/download`)
- ✅ Session history view with search/filter bar
- ✅ Chat view (Messages/Messagebar) with Terminal toggle
- ✅ In-app toast + desktop Notification API for session status changes
- ✅ Peer add/remove API (`POST /api/v1/peers`, `DELETE /api/v1/peers/{name}`)
- ✅ Peer management in settings view (list, add, remove with status indicators)

### Phase 6: Mobile + Notifications

**Stack:** Tauri 2 mobile (iOS/Android) + `tauri-plugin-remote-push` (APNs + FCM)

Tauri 2 builds native iOS `.ipa` and Android `.apk` from the same Svelte + Rust codebase. The Konsta UI components from Phase 5 already provide the mobile-native look.

**Deliverables:**

- ✅ Tauri iOS build + TestFlight distribution
- ✅ Tauri Android build + Play Store distribution
- ✅ Token authentication + bind modes (local/lan)
- ✅ mDNS peer discovery (`_pulpo._tcp.local.`) — activates in `lan` bind mode
- ✅ QR code pairing for mobile clients
- ⬜ Tailscale auto-discovery — planned
- ⬜ Push notifications via APNs (iOS) and FCM (Android) — not planned (polling + Notification API sufficient)

### Phase 7: Voice Commands (experimental)

- ✅ Connection settings bridge (Tauri `save_connection` / `load_connection` commands)
- 🧪 iOS Siri Shortcuts: "Check my agents in Pulpo", "Tell my Pulpo agent [message]", "Stop my Pulpo agent"
- 🧪 Android App Actions: check agents, send to agent, stop agent via Google Assistant

### Phase 8: Control Plane + Notifications ✅

- ✅ Flexible session model (model, allowed_tools, system_prompt, metadata, persona)
- ✅ Persona config (`[personas.name]` in config.toml, `GET /api/v1/personas`)
- ✅ SSE event stream (`GET /api/v1/events`, broadcast channel, SessionEvent)
- ✅ Discord webhook notifications (`[notifications.discord]` config)
- ✅ Discord bot (`contrib/discord-bot/`) — slash commands + SSE listener
- ✅ MCP server (session management as MCP tools)

---

## Configuration

`~/.pulpo/config.toml`:

```toml
[node]
name = "mac-mini"       # Display name (default: hostname)
port = 7433             # API port (default: 7433)

[auth]
bind = "local"          # "local" (127.0.0.1) or "lan" (0.0.0.0)
# token is auto-generated on first run

[guards]
preset = "standard"     # standard (default), strict, or unrestricted

[watchdog]
enabled = true
memory_threshold = 90
check_interval_secs = 10
breach_count = 3
idle_timeout_secs = 600
idle_action = "alert"       # "alert" or "kill"

[peers]
macbook = "macbook:7433"
server = "hetzner:7433"

[personas.reviewer]
provider = "claude"
model = "sonnet"
guard_preset = "strict"
allowed_tools = ["Read", "Glob", "Grep"]
system_prompt = "You are a code reviewer. Focus on correctness and security."

[personas.coder]
provider = "claude"
mode = "autonomous"
guard_preset = "standard"

[notifications.discord]
webhook_url = "https://discord.com/api/webhooks/..."
events = ["running", "completed", "dead"]   # optional filter; omit for all events
```

---

## Security Model

- **Network**: `pulpod` binds to `127.0.0.1` by default (`local` mode). In `lan`
  mode, it binds to `0.0.0.0` and requires bearer token authentication on all
  `/api/v1/*` requests. Tailscale encryption is recommended for multi-node setups.
- **Auth**: In `local` mode, network isolation is the auth layer. In `lan` mode,
  a base64url token is auto-generated on first run and required in every request.
  Retrieve it locally via `GET /api/v1/auth/token`.
- **Agents**: agents run as your user (same as running Claude Code directly).
  Guard presets control environment variable sanitization and agent permissions.
- **No secrets in the API**: the API never exposes API keys. Keys are in the
  environment or config files on each node. The daemon passes them through to
  the agent process.

---

## Open Questions (Resolved)

1. **License**: Dual MIT / Apache-2.0 (both license files in repo root).
2. **Binary distribution**: GitHub Actions CI builds and tests on every push. `draft-release.yml` creates draft releases; `release.yml` publishes tagged releases with pre-built binaries for macOS (aarch64) and Linux (x86_64).
3. **Tailscale dependency**: Optional enhancement, not required. Core works on localhost/LAN. Tailscale makes multi-node seamless but isn't a hard dependency.
4. **Web UI bundling**: Embedded in binary via `rust-embed` — single binary distribution. Dev mode uses Vite dev server with API proxy.
5. **tmux version requirements**: Minimum tmux 3.2+. Checked at daemon startup with a clear error message if too old or not installed.
