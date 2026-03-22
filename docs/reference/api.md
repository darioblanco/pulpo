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
| DELETE | `/api/v1/sessions/:id` | Delete session record |
| POST | `/api/v1/sessions/:id/kill` | Kill a running session |
| POST | `/api/v1/sessions/:id/resume` | Resume a lost or ready session |
| GET | `/api/v1/sessions/:id/output` | Get captured terminal output |
| GET | `/api/v1/sessions/:id/output/download` | Download full output as file |
| POST | `/api/v1/sessions/:id/input` | Send text input to a session |
| GET | `/api/v1/sessions/:id/interventions` | List watchdog interventions |
| GET | `/api/v1/sessions/:id/stream` | WebSocket terminal stream |
| POST | `/api/v1/sessions/cleanup` | Delete all killed and lost sessions |
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
  "runtime": "docker",
  "secrets": ["GITHUB_TOKEN"]
}
```

`name` is required. All other fields are optional. If `ink` is specified, its `command` is used as the default (explicit `command` overrides it). `idle_threshold_secs` overrides the global idle threshold for this session (`null` = use global, `0` = never idle).

## Inks

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/inks` | List configured inks |

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
- **Session events**: `created`, `active`, `idle`, `ready`, `killed`, `lost`, `resumed`

```bash
curl -N http://localhost:7433/api/v1/events
```
