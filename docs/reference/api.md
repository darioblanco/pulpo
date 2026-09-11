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
| GET | `/api/v1/metrics` | Prometheus text exposition (opt-in — `404` unless `[metrics] enabled = true`) |

## Usage

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/usage/projection` | Live per-session burn rate ($/hr, tokens/hr) and time-to-cap for pulpo-managed sessions |
| GET | `/api/v1/usage/scan` | Scan-only: total spend across *all* local agent history (Claude Code, Codex, pi), no sessions routed through pulpo required (`?since_days=`, `?by_worktree=true`) |

## Auth

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/auth/token` | Get current auth token |

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
| POST | `/api/v1/sessions/:id/harness-events` | Ingest a harness lifecycle event (posted by `pulpo hook <harness>`) |
| POST | `/api/v1/sessions/:id/handoff` | Spawn a new session inheriting this one's working directory and git worktree |
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
  "budget_cost_usd": 5.0
}
```

`name` is required. All other fields are optional; without a `command`, the session falls back to the node's `default_command` or `$SHELL`. `idle_threshold_secs` overrides the global idle threshold for this session (`null` = use global, `0` = never idle). `worktree_base` specifies the branch to fork from (implies `worktree: true`). `budget_cost_usd` sets a cost budget for this session — the watchdog alerts at 80% and stops the session at 100%. Session responses include `worktree_branch` with the branch name when a worktree is active.

There is no cross-node targeting — a create request always runs on the `pulpod` that
receives it. To spawn on another machine, send the request to that machine directly (point
the CLI or an HTTP client at its address, e.g. `pulpo --url gpu-box spawn ...`).
`GET /api/v1/sessions/:id/stream` is node-local by the same principle (any client that can
reach this node can open it — there's no loopback-only check); remote terminal proxying
*across* nodes is intentionally out of scope.

### Handoff Session (POST /api/v1/sessions/:id/handoff)

```json
{
  "name": "my-api-2",
  "command": "codex 'implement PLAN.md'",
  "description": "Build from the plan",
  "budget_cost_usd": 5.0,
  "idle_threshold_secs": null
}
```

`:id` is the **source** session (resolved by ID or name, same as `GET .../sessions/:id`).
Every field is optional: `name` auto-generates as `<source>-2`, `-3`, ... when omitted;
without `command`, the new session opens a login shell. The new session inherits the
source's working directory and, if the source used one, its git worktree (`adopted`, not
copied — no new branch or checkout). Returns `201 Created` with the same shape as
`POST /api/v1/sessions`. See [Plan Then Build](/guides/plan-then-build).

### Harness Events (POST /api/v1/sessions/:id/harness-events)

```json
{
  "harness": "claude",
  "event": { "hook_event_name": "Stop", "session_id": "...", "...": "..." }
}
```

`harness` is the adapter id (`"claude"`, `"codex"`, or `"pi"`; `"generic"` never emits
events); `event` is the raw JSON payload exactly as the harness's hook runner produced
it (for `"pi"`, whatever `pulpo.ts` posted), passed through unchanged. The daemon
resolves the session's harness adapter, translates the payload into a normalized
lifecycle event, applies the resulting state transition (see
[Harness Adapters](/architecture/harness-adapters)), and emits the existing SSE `session`
event — no separate notification channel.

Status codes, checked in this order:
- `404` if the session doesn't exist.
- `204 No Content` (no-op) if the session is already `Stopped` or `Lost` — a hook can
  fire after the harness process (and pulpo's own bookkeeping for it) is already done,
  which is expected and racy, not an error. This check runs *before* the harness-id
  check below, so a terminal session's events are never rejected just because
  `harness` looks wrong.
- `400` if `harness` doesn't match the session's own stored harness (set once, at spawn
  time) — including an unrecognized `harness` id. The request body's `harness` field is
  untrusted client input; it's never used to resolve the adapter unless it agrees with
  `session.harness`, so a spoofed value can't run the wrong adapter's parser against this
  session's payload.
- `204 No Content` on a successful, recognized event.
- `500` for anything else (an internal/store failure applying the event).

This endpoint isn't meant to be called directly — it's what `pulpo hook <harness>`
(injected into the harness's own hook config at spawn time) posts to.

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

Each SSE frame's `event:` field is one of:
- **`session`**: a session's status changed — `status` is one of `creating`, `active`,
  `idle`, `ready`, `stopped`, `lost` (`needs_input` is set alongside `idle` when a harness
  adapter reports the session is blocked on the user; see
  [Harness Adapters](/architecture/harness-adapters))
- **`session_deleted`**: a session was purged (`stop --purge` or `pulpo cleanup`)
- **`usage_alert`**: a budget or burn-ceiling threshold fired
- **`intervention`**: the watchdog forcibly stopped a session

```bash
curl -N http://localhost:7433/api/v1/events
```
