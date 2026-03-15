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
| GET | `/api/v1/node` | Node info (name, version, platform, memory, GPU) |
| GET | `/api/v1/config` | Current config |
| PUT | `/api/v1/config` | Update config (live reload) |
| GET | `/api/v1/watchdog` | Watchdog config |
| PUT | `/api/v1/watchdog` | Update watchdog config (live reload) |
| GET | `/api/v1/notifications` | Notification config |
| PUT | `/api/v1/notifications` | Update notification config |

## Auth

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/auth/token` | Get current auth token |
| GET | `/api/v1/auth/pairing-url` | Get pairing URL for web UI connection |

## Sessions

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/sessions` | List sessions (supports `?status=active&provider=claude`) |
| POST | `/api/v1/sessions` | Create (spawn) a new session |
| GET | `/api/v1/sessions/:id` | Get session details |
| DELETE | `/api/v1/sessions/:id` | Delete session record |
| POST | `/api/v1/sessions/:id/kill` | Kill a running session |
| POST | `/api/v1/sessions/:id/resume` | Resume a lost or finished session |
| GET | `/api/v1/sessions/:id/output` | Get captured terminal output |
| GET | `/api/v1/sessions/:id/output/download` | Download full output as file |
| POST | `/api/v1/sessions/:id/input` | Send text input to a session |
| GET | `/api/v1/sessions/:id/interventions` | List watchdog interventions |
| GET | `/api/v1/sessions/:id/stream` | WebSocket terminal stream |

### Create Session (POST /api/v1/sessions)

```json
{
  "name": "my-api",
  "workdir": "/path/to/repo",
  "prompt": "Fix the auth bug",
  "provider": "claude",
  "mode": "autonomous",
  "unrestricted": false,
  "model": "claude-sonnet-4-20250514",
  "ink": "reviewer",
  "worktree": true,
  "system_prompt": "Be thorough",
  "allowed_tools": ["Bash", "Read"],
  "max_turns": 50,
  "max_budget_usd": 10.0,
  "output_format": "stream-json"
}
```

`name` is required. All other fields are optional. Defaults are applied from session_defaults → inks → node config.

## Providers

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/providers` | List available providers with compatibility matrix |

Returns which providers are installed and which flags each supports.

## Inks

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/inks` | List configured inks |

## Peers

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/peers` | List known peers (manual + discovered) |
| POST | `/api/v1/peers` | Add a manual peer |
| DELETE | `/api/v1/peers/:name` | Remove a manual peer |

## Events (SSE)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/events` | Server-Sent Events stream |

Event types:
- **Session events**: `created`, `active`, `idle`, `finished`, `killed`, `lost`, `resumed`

```bash
curl -N http://localhost:7433/api/v1/events
```
