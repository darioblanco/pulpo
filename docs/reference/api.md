# API Reference

Pulpo exposes REST + SSE from `pulpod` (default port `7433`).

All endpoints require auth when `bind = "public"` (pass `Authorization: Bearer <token>` header).

## Health

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/health` | Health check |

## Node & Config

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/node` | Node info (name, hostname, os, arch, cpus, memory, GPU) |
| GET | `/api/v1/config` | Current config |
| PUT | `/api/v1/config` | Update config (live reload) |
| GET | `/api/v1/watchdog` | Watchdog config |
| PUT | `/api/v1/watchdog` | Update watchdog config (live reload) |
| GET | `/api/v1/notifications` | Notification config |
| PUT | `/api/v1/notifications` | Update notification config |
| GET | `/api/v1/peers` | List known peers |
| POST | `/api/v1/peers` | Add a manual peer |
| DELETE | `/api/v1/peers/:name` | Remove a manual peer |

## Auth

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/auth/token` | Get current auth token |
| GET | `/api/v1/auth/pairing-url` | Get pairing URL for web UI connection |

## Sessions

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/sessions` | List sessions (supports `?status=active`) |
| POST | `/api/v1/sessions` | Create (spawn) a new session |
| GET | `/api/v1/sessions/:id` | Get session details |
| POST | `/api/v1/sessions/:id/stop` | Stop a running session (add ?purge=true to also remove record) |
| POST | `/api/v1/sessions/:id/resume` | Resume a lost or ready session |
| GET | `/api/v1/sessions/:id/output` | Get captured terminal output |
| GET | `/api/v1/sessions/:id/output/download` | Download full output as file |
| POST | `/api/v1/sessions/:id/input` | Send text input to a session |
| GET | `/api/v1/sessions/:id/interventions` | List watchdog interventions |
| GET | `/api/v1/sessions/:id/stream` | WebSocket terminal stream |
| POST | `/api/v1/sessions/cleanup` | Remove all stopped and lost sessions |
| GET | `/api/v1/fleet/sessions` | Aggregate sessions across peers |

### Create Session (POST /api/v1/sessions)

```json
{
  "name": "my-api",
  "command": "claude -p 'Fix the auth bug'",
  "workdir": "/path/to/repo",
  "ink": "reviewer",
  "description": "Fix auth bug in login endpoint",
  "metadata": {},
  "idle_threshold_secs": 120,
  "worktree": true,
  "worktree_base": "main",
  "runtime": "docker",
  "secrets": ["GITHUB_TOKEN"]
}
```

`name` is required. All other fields are optional. If `ink` is specified, its `command` is used as the default (explicit `command` overrides it). `idle_threshold_secs` overrides the global idle threshold for this session (`null` = use global, `0` = never idle). `worktree_base` specifies the branch to fork from (implies `worktree: true`). Session responses include `worktree_branch` with the branch name when a worktree is active.

## Inks

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/inks` | List all configured inks |
| GET | `/api/v1/inks/:name` | Get a specific ink |
| POST | `/api/v1/inks/:name` | Create a new ink |
| PUT | `/api/v1/inks/:name` | Update an existing ink |
| DELETE | `/api/v1/inks/:name` | Delete an ink |

Ink request/response body:

```json
{
  "description": "Code reviewer focused on correctness",
  "command": "claude -p 'review this code'",
  "secrets": ["GITHUB_TOKEN"],
  "runtime": "docker"
}
```

All fields are optional. `secrets` defaults to `[]`, `runtime` defaults to `null` (inherits tmux). Changes are persisted to `config.toml`.

## Schedules

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/schedules` | List schedules |
| POST | `/api/v1/schedules` | Create a schedule |
| GET | `/api/v1/schedules/:id` | Get a schedule |
| PUT | `/api/v1/schedules/:id` | Update a schedule |
| DELETE | `/api/v1/schedules/:id` | Delete a schedule |
| GET | `/api/v1/schedules/:id/runs` | List schedule run history |

## Secrets

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/secrets` | List secret names |
| PUT | `/api/v1/secrets/:name` | Set a secret |
| DELETE | `/api/v1/secrets/:name` | Delete a secret |

## Push Notifications

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/push/vapid-key` | Get the public VAPID key |
| POST | `/api/v1/push/subscribe` | Register a Web Push subscription |
| POST | `/api/v1/push/unsubscribe` | Remove a Web Push subscription |

## Events (SSE)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/events` | Server-Sent Events stream |

Event types:
- **Session events**: `created`, `active`, `idle`, `ready`, `stopped`, `lost`, `resumed`

```bash
curl -N http://localhost:7433/api/v1/events
```
