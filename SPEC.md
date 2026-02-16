# Norn — Agent Session Orchestrator

> _The Norns weave the fate of gods and men from beneath the world tree._

Norn is a lightweight, Tailscale-native daemon that manages coding agent sessions
(Claude Code, Codex, Aider) across multiple machines. It abstracts tmux, Docker,
and terminal management behind a clean API, and provides a mobile-friendly web UI
for orchestrating agents from your phone.

## Problem

You have multiple machines (Macs, Windows PCs) connected via Tailscale. You want
to spawn, monitor, and manage coding agents on any of them from your phone or
laptop. Today this requires: Termius -> SSH -> tmux attach -> navigate windows.
Too many layers. And if a machine reboots, you lose your session state.

## Goals

1. **Single binary** (`nornd`) runs on each machine as a daemon
2. **Abstracts tmux** (macOS/Linux) and **Docker+tmux** (Windows/WSL2) behind a
   unified session API
3. **Web UI** served by the daemon — mobile-first, works great on iPhone Safari
4. **Multi-node** — discover and manage sessions across all your Tailscale machines
   from one dashboard
5. **Session persistence** — survive reboots by storing conversation IDs, prompts,
   output snapshots, and git state in a local database
6. **Open source** — MIT or Apache 2.0

## Non-Goals (for now)

- iOS native app (future — start with mobile web)
- Agent-to-agent communication
- Custom AI model hosting/serving
- CI/CD integration (use GitHub Actions separately)
- Multi-user / team features (single-user, your Tailnet)

---

## Architecture

```
                        ┌──────────────────────┐
                        │     Your Phone       │
                        │  (Safari / iOS app)  │
                        └──────────┬───────────┘
                                   │ HTTPS (Tailscale)
                        ┌──────────▼───────────┐
                        │      Web UI          │
                        │  (served by any      │
                        │   nornd instance)    │
                        └──────────┬───────────┘
                                   │
              ┌────────────────────┼────────────────────┐
              │                    │                    │
     ┌────────▼───────┐  ┌────────▼───────┐  ┌────────▼───────┐
     │   mac-mini     │  │   macbook      │  │   win-pc       │
     │   nornd        │  │   nornd        │  │   nornd        │
     │                │  │                │  │   (WSL2)       │
     │   ┌─────────┐  │  │   ┌─────────┐  │  │   ┌─────────┐  │
     │   │  tmux   │  │  │   │  tmux   │  │  │   │ Docker  │  │
     │   │sessions │  │  │   │sessions │  │  │   │ + tmux  │  │
     │   └─────────┘  │  │   └─────────┘  │  │   └─────────┘  │
     │   ┌─────────┐  │  │   ┌─────────┐  │  │   ┌─────────┐  │
     │   │ SQLite  │  │  │   │ SQLite  │  │  │   │ SQLite  │  │
     │   └─────────┘  │  │   └─────────┘  │  │   └─────────┘  │
     └────────────────┘  └────────────────┘  └────────────────┘
```

### Components

#### 1. `nornd` — The Daemon (Rust)

Runs on every machine. Responsibilities:

- **Session lifecycle**: create, list, attach, kill, resume sessions
- **Terminal backend**: manages tmux (direct) or Docker+tmux (Windows)
- **API server**: REST + WebSocket on a configurable port (default: 7433)
- **Persistence**: SQLite for session state, output snapshots, conversation IDs
- **Node info**: reports machine capabilities (OS, CPU, RAM, GPU)
- **Peer discovery**: finds other `nornd` instances on the Tailnet

#### 2. `norn` — The CLI (Rust)

Thin CLI client that talks to the local (or remote) `nornd` API. For when you
want to manage sessions from a terminal instead of the web UI.

```bash
# Local usage (talks to local nornd)
norn spawn --repo ~/repos/my-api --provider claude "Fix the auth bug"
norn list
norn attach my-api
norn logs my-api
norn kill my-api
norn resume my-api         # after reboot, resume Claude conversation
norn nodes                 # list all nornd peers on the Tailnet
norn list --node win-pc    # list sessions on a remote node

# Remote usage (talks to remote nornd)
norn --node win-pc spawn --repo ~/repos/ml-model --provider claude "Train it"
```

#### 3. Web UI

Embedded in the `nornd` binary (static assets compiled in). Mobile-first design.

**Views:**

- **Dashboard**: all nodes, all sessions, at a glance
- **Session detail**: live terminal output (xterm.js), input field, metadata
- **New session**: pick node, repo, provider, write prompt
- **Node detail**: machine info, resource usage

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

- **CREATING**: tmux session (or Docker container) is being set up
- **RUNNING**: agent is active, terminal output is streaming
- **COMPLETED**: agent exited cleanly (exit code 0)
- **DEAD**: agent exited with error or was killed
- **STALE**: the daemon restarted and found a session record in SQLite but no
  matching tmux session — the machine rebooted or tmux crashed. The user can
  "resume" which creates a new tmux session and runs the agent with
  `--resume <conversation-id>` (Claude Code) or points it at the existing
  chat history file (Aider)

### Persistence (what survives a reboot)

Stored in `~/.norn/state.db` (SQLite):

| Field              | Description                                          |
| ------------------ | ---------------------------------------------------- |
| `id`               | UUID                                                 |
| `name`             | Human-readable session name (default: repo basename) |
| `repo_path`        | Absolute path to the repository                      |
| `provider`         | `claude`, `codex`, `aider`                           |
| `prompt`           | The original prompt/task description                 |
| `conversation_id`  | Claude Code conversation ID (from ~/.claude/)        |
| `status`           | `creating`, `running`, `completed`, `dead`, `stale`  |
| `exit_code`        | Process exit code (null if still running)            |
| `tmux_session`     | tmux session name                                    |
| `docker_container` | Docker container ID (Windows only, null on macOS)    |
| `output_snapshot`  | Last N lines of terminal output                      |
| `git_branch`       | Branch name at session start                         |
| `git_sha`          | Commit SHA at session start                          |
| `created_at`       | Timestamp                                            |
| `updated_at`       | Timestamp                                            |

### Output Capture

The daemon periodically (every 5s) runs `tmux capture-pane` to grab the
current terminal content and stores it in the DB. This means:

- The web UI can show recent output even without a live WebSocket connection
- After a reboot, you can see what the agent was doing before it died
- Log files are also written to `~/.norn/logs/<session-id>.log` via
  `tmux pipe-pane`

---

## Terminal Backend

### macOS / Linux

Direct tmux management:

```
nornd
  └─▶ tmux new-session -d -s norn-<id> -c <repo_path>
       └─▶ claude --dangerously-skip-permissions  (or codex/aider)
```

- Output streaming: `tmux pipe-pane` to a log file + periodic `capture-pane`
- Input: `tmux send-keys -t norn-<id> "text" Enter`
- Attach (web): WebSocket ↔ `tmux attach` via a PTY bridge (or xterm.js
  connecting to a PTY that runs `tmux attach`)

### Windows (WSL2 + Docker)

```
nornd (running inside WSL2)
  └─▶ docker run -d --name norn-<id>
       -v <repo_path>:/home/agent/project
       -v ~/.norn/sessions/<id>/claude:/home/agent/.claude
       claude-agent
       └─▶ tmux new-session ...
            └─▶ claude --dangerously-skip-permissions
```

Key difference: conversation files are stored in a per-session volume mount
(`~/.norn/sessions/<id>/claude/`) so they persist across container restarts.

The daemon detects the platform at startup and uses the appropriate backend.

---

## Peer Discovery

### Phase 1: Manual Configuration

`~/.norn/config.toml`:

```toml
[node]
name = "mac-mini"         # This node's display name
port = 7433

[peers]
# Other nornd instances on your Tailnet
macbook = "macbook:7433"
win-pc  = "win-pc:7433"
```

### Phase 2: Tailscale Auto-Discovery

Query the Tailscale local API to find peers:

```
GET http://127.0.0.1:41112/localapi/v0/status
```

This returns all devices on the Tailnet. The daemon probes each peer on the
known port (7433) to check if `nornd` is running. No manual config needed.

### API Between Nodes

Each `nornd` exposes the same REST API. The web UI (served by one node) fans
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
DELETE /sessions/:id          Kill a session
POST   /sessions/:id/resume   Resume a stale session
POST   /sessions/:id/input    Send input to the session terminal
GET    /sessions/:id/output   Get recent output (polling)
WS     /sessions/:id/stream   Stream terminal output (WebSocket)
```

#### POST /sessions

```json
{
  "name": "my-api",
  "repo_path": "/home/user/repos/my-api",
  "provider": "claude",
  "prompt": "Fix the auth bug in login.py",
  "resources": {
    "memory": "6g",
    "cpus": 4
  }
}
```

#### GET /sessions

```json
[
  {
    "id": "a1b2c3d4",
    "name": "my-api",
    "provider": "claude",
    "status": "running",
    "prompt": "Fix the auth bug in login.py",
    "created_at": "2026-02-16T10:30:00Z",
    "output_preview": "Analyzing login.py...\nFound issue in validate_token()..."
  }
]
```

### Node

```
GET    /node                  Node info (hostname, OS, resources)
GET    /node/stats            Live resource usage
```

### Peers

```
GET    /peers                 List known peers and their status
```

---

## Web UI Design

### Tech Stack

**Option A: Leptos (Rust WASM)** — single language, compiles into the binary.
Heavier to develop but zero JS toolchain.

**Option B: Static SPA (Svelte/Solid)** — faster to iterate, better ecosystem
for terminal UIs (xterm.js). Built separately, output bundled into the Rust
binary via `include_dir!` or `rust-embed`.

**Recommendation: Option B (Svelte + xterm.js)** for the MVP. The terminal
rendering story is much better with xterm.js (mature, battle-tested) than any
Rust WASM terminal emulator. The build output is just static files embedded
in the binary — still a single binary to distribute.

### Layout (Mobile-First)

```
┌─────────────────────────────┐
│  norn            ⚙ Settings │
├─────────────────────────────┤
│                             │
│  ● mac-mini (2 running)    │
│  ┌─────────────────────┐   │
│  │ ● my-api    claude  │──▶│
│  │   Fix auth   2h ago │   │
│  ├─────────────────────┤   │
│  │ ○ docs      aider   │──▶│
│  │   Update API  done  │   │
│  └─────────────────────┘   │
│                             │
│  ● win-pc (1 running)      │
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

```
norn/
├── Cargo.toml                  # Workspace root
├── LICENSE                     # MIT or Apache 2.0
├── README.md
├── SPEC.md                     # This file
│
├── crates/
│   ├── nornd/                  # Daemon binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs         # Entry point, CLI args, signal handling
│   │       ├── config.rs       # Config file parsing
│   │       ├── api/
│   │       │   ├── mod.rs
│   │       │   ├── routes.rs   # Axum route definitions
│   │       │   ├── sessions.rs # Session CRUD handlers
│   │       │   ├── node.rs     # Node info handlers
│   │       │   ├── peers.rs    # Peer discovery handlers
│   │       │   └── ws.rs       # WebSocket terminal streaming
│   │       ├── session/
│   │       │   ├── mod.rs
│   │       │   ├── manager.rs  # Session lifecycle orchestration
│   │       │   ├── state.rs    # Session state machine
│   │       │   └── output.rs   # Output capture (tmux capture-pane)
│   │       ├── backend/
│   │       │   ├── mod.rs      # Backend trait definition
│   │       │   ├── tmux.rs     # Direct tmux backend (macOS/Linux)
│   │       │   └── docker.rs   # Docker+tmux backend (Windows/WSL2)
│   │       ├── store/
│   │       │   ├── mod.rs
│   │       │   └── sqlite.rs   # SQLite persistence
│   │       ├── peers/
│   │       │   ├── mod.rs
│   │       │   ├── manual.rs   # Config-file-based peer list
│   │       │   └── tailscale.rs # Tailscale API auto-discovery
│   │       └── platform.rs     # OS detection, platform-specific paths
│   │
│   ├── norn-cli/               # CLI client binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs         # CLI commands via clap
│   │
│   └── norn-common/            # Shared types
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── session.rs      # Session types, states
│           ├── node.rs         # Node info types
│           └── api.rs          # API request/response types
│
└── web/                        # Web UI (Svelte)
    ├── package.json
    ├── svelte.config.js
    ├── src/
    │   ├── app.html
    │   ├── routes/
    │   │   ├── +layout.svelte  # Shell: nav, node selector
    │   │   ├── +page.svelte    # Dashboard
    │   │   └── sessions/
    │   │       └── [id]/
    │   │           └── +page.svelte  # Session detail + terminal
    │   ├── lib/
    │   │   ├── api.ts          # API client
    │   │   ├── terminal.ts     # xterm.js wrapper
    │   │   └── stores.ts       # Svelte stores for sessions/nodes
    │   └── components/
    │       ├── SessionCard.svelte
    │       ├── NodeStatus.svelte
    │       ├── Terminal.svelte  # xterm.js component
    │       └── NewSession.svelte
    └── static/
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

- [x] `nornd` daemon with REST API (no WebSocket yet)
- [x] tmux backend (macOS/Linux only)
- [x] SQLite persistence for session state
- [x] Output capture via `tmux capture-pane` (polling)
- [x] `norn` CLI: spawn, list, kill, logs
- [x] Web UI: dashboard + session list + output viewer (polling, no live terminal)
- [x] Single-node only (no peer discovery)
- [x] Claude Code provider only

### Out of Scope (Phase 2+)

- [ ] WebSocket streaming + live terminal (xterm.js attach)
- [ ] Multi-node peer discovery
- [ ] Docker backend (Windows/WSL2)
- [ ] Codex and Aider providers
- [ ] Session resume after reboot
- [ ] `tailscale serve` integration for HTTPS
- [ ] Push notifications (web push API)
- [ ] iOS native app

---

## Phase Roadmap

### Phase 1: Single-Node MVP

- `nornd` + `norn` CLI + basic web UI
- macOS/Linux, tmux, Claude Code only
- Polling-based output, no live terminal
- **Goal**: replace `ssh + tmux` with `norn spawn` + phone web UI

### Phase 2: Live Terminal + Persistence

- WebSocket streaming with xterm.js
- Full interactive terminal in the web UI
- Session resume after reboot (STALE -> RUNNING)
- Output log files via `tmux pipe-pane`

### Phase 3: Multi-Node

- Peer discovery (manual config, then Tailscale auto)
- Aggregated dashboard across all nodes
- Remote session spawning from any node's UI

### Phase 4: Windows + Multi-Provider

- Docker+tmux backend for Windows/WSL2
- Codex and Aider support
- Provider-specific resume logic

### Phase 5: iOS + Polish

- iOS native app (Swift, uses the same REST API)
- Push notifications when sessions complete
- Siri Shortcuts integration ("Hey Siri, check my agents")
- `tailscale serve` integration for automatic HTTPS

---

## Configuration

`~/.norn/config.toml`:

```toml
[node]
# Display name for this node (default: system hostname)
name = "mac-mini"

# Port for the HTTP/WebSocket API
port = 7433

# Where to store session data
data_dir = "~/.norn"

[session.defaults]
# Default provider when not specified
provider = "claude"

# Default resource limits (used for Docker backend)
memory = "6g"
cpus = 4

[providers.claude]
# Path to claude binary (auto-detected if on PATH)
binary = "claude"

# Default flags
flags = ["--dangerously-skip-permissions"]

[providers.codex]
binary = "codex"
flags = ["--full-auto"]

[providers.aider]
binary = "aider"
flags = ["--yes-always"]
model = "gpt-4o"

# Phase 3: peer nodes
# [peers]
# macbook = "macbook:7433"
# win-pc = "win-pc:7433"
```

---

## Security Model

- **Network**: all traffic is Tailscale-encrypted. `nornd` only listens on the
  Tailscale interface (or localhost). No public internet exposure.
- **Auth**: Tailscale identity is the auth layer. If you're on the Tailnet, you
  can access `nornd`. Single-user system — your Tailnet is your trust boundary.
- **Agents**: on macOS, agents run as your user (same as running Claude Code
  directly). On Windows, agents run in Docker containers (sandboxed).
- **No secrets in the API**: the API never exposes API keys. Keys are in the
  environment or config files on each node. The daemon passes them through to
  the agent process.

---

## Open Questions

1. **License**: MIT or Apache 2.0? (MIT is simpler, Apache has patent protection)
2. **Binary distribution**: should we cross-compile for all platforms from CI,
   or rely on `cargo install norn`?
3. **Tailscale dependency**: should `nornd` hard-require Tailscale, or work on
   any network? (I'd say: optimize for Tailscale but don't hard-require it —
   it's just an HTTP server on a port)
4. **Web UI bundling**: embed in binary (single binary distribution) or serve
   from a separate directory (easier to develop)?
5. **tmux version requirements**: what's the minimum tmux version for the
   features we need? (capture-pane, pipe-pane, send-keys all work on tmux 3.0+)
