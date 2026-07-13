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
| POST | `/api/v1/sessions/:id/resume` | Resume a lost, ready, or stopped session |
| GET | `/api/v1/sessions/:id/output` | Get captured terminal output |
| GET | `/api/v1/sessions/:id/output/download` | Download full output as file |
| POST | `/api/v1/sessions/:id/input` | Send text input to a session |
| GET | `/api/v1/sessions/:id/interventions` | List watchdog interventions |
| GET | `/api/v1/sessions/:id/stream` | WebSocket terminal stream |
| POST | `/api/v1/sessions/cleanup` | Remove all stopped and lost sessions |

### Create Session (POST /api/v1/sessions)

```json
{
  "name": "my-api",
  "command": "claude -p 'Fix the auth bug'",
  "workdir": "/path/to/repo",
  "description": "Fix auth bug in login endpoint",
  "metadata": {},
  "idle_threshold_secs": 120,
  "worktree": true,
  "worktree_base": "main",
  "runtime": "tmux",
  "secrets": ["GITHUB_TOKEN"],
  "budget_cost_usd": 5.0
}
```

`name` is required. All other fields are optional; without a `command`, the session falls back to the node's `default_command` or `$SHELL`. `idle_threshold_secs` overrides the global idle threshold for this session (`null` = use global, `0` = never idle). `worktree_base` specifies the branch to fork from (implies `worktree: true`). `budget_cost_usd` sets a cost budget for this session — the watchdog alerts at 80% and stops the session at 100%. Session responses include `worktree_branch` with the branch name when a worktree is active.

There is no cross-node targeting — a create request always runs on the `pulpod` that
receives it. To spawn on another machine, send the request to that machine directly (point
the CLI or an HTTP client at its address, e.g. `pulpo --node gpu-box spawn ...`).
`GET /api/v1/sessions/:id/stream` is local-only by the same principle; remote terminal
proxying is intentionally out of scope.

## Schedules

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/schedules` | List schedules |
| POST | `/api/v1/schedules` | Create a schedule |
| GET | `/api/v1/schedules/:id` | Get a schedule |
| PUT | `/api/v1/schedules/:id` | Update a schedule |
| DELETE | `/api/v1/schedules/:id` | Delete a schedule |
| GET | `/api/v1/schedules/:id/runs` | List schedule run history |

Like sessions, schedules accept a `budget_cost_usd` field — applied to every session the
schedule fires (watchdog alerts at 80%, stops at 100%).

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
| POST | `/api/v1/push/action` | Act on a push notification's action token (currently just `stop`) — **unauthenticated**, see below |

Every subscription receives `lifecycle`, `usage_alert`, and `intervention` push
notifications; `usage_alert` payloads additionally carry a short-lived, HMAC-signed
action token that lets the "Stop session" button on the notification stop the session
without the app's bearer token (`POST /api/v1/push/action` is exempt from
`bind = "public"` auth for this reason — the token itself is the capability). Full
payload schema, token format, and status codes: [Push Notifications reference](/reference/push).

## Events (SSE)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/events` | Server-Sent Events stream |

Event types:
- **Session events**: `created`, `active`, `idle`, `ready`, `stopped`, `lost`, `resumed`

```bash
curl -N http://localhost:7433/api/v1/events
```
