# API Reference

Pulpo exposes REST + SSE from `pulpod` (default port `7433`).

All `/api/v1/*` endpoints require auth when `bind = "public"` (pass
`Authorization: Bearer <token>` header, or `?token=<token>` as a query parameter).
Three things are exempt from that check even in `public` mode: `GET /api/v1/health`,
`/api/v1/auth/*` when the request comes from loopback, and everything outside
`/api/v1/*` (the embedded web UI's static files). In `local`/`tailscale` bind modes no
token is required anywhere — network isolation is the guard instead.

Every JSON error response has the same shape:

```json
{ "error": "session not found: abc123" }
```

## Health

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/health` | none (always exempt) | `{ "status": "ok", "version": "<cargo pkg version>" }` |

## Node & Config

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/node` | required in `public` | Node info |
| GET | `/api/v1/config` | required in `public` | Current effective config |
| GET | `/api/v1/watchdog` | required in `public` | Watchdog config |
| GET | `/api/v1/notifications` | required in `public` | Notification (webhook) config |

These are all **read-only** — `PUT /api/v1/config`, `PUT /api/v1/watchdog`, and
`PUT /api/v1/notifications` do not exist (any `PUT`/`POST` to these paths falls through
to `405 Method Not Allowed`). The config file (`~/.pulpo/config.toml`) is the sole
source of truth — there is no API or web UI path to edit it. To change anything, edit
the file and restart `pulpod`; see [Configuration](../guides/configuration.md).

**`GET /api/v1/node`** → `NodeInfo`:

| Field | Type | Description |
|-------|------|--------------|
| `name` | string | Node display name (`[node].name`) |
| `hostname` | string | OS hostname |
| `os` | string | `macos`, `linux`, or `wsl2` |
| `arch` | string | `std::env::consts::ARCH` (e.g. `aarch64`, `x86_64`) |
| `cpus` | number | Logical CPU count |
| `memory_mb` | number | Total system memory in MB |
| `gpu` | string \| null | Always `null` today (no GPU detection implemented) |

**`GET /api/v1/config`** → `ConfigResponse`:

| Field | Type | Description |
|-------|------|--------------|
| `node.name` / `node.port` / `node.data_dir` / `node.bind` | — | Mirrors `[node]` (see [Config Reference](config.md)) |
| `auth` | object | Always `{}` — the token itself is never included here (see `GET /api/v1/auth/token`) |
| `watchdog.*` | — | Same shape as `GET /api/v1/watchdog` below |
| `notifications.webhooks` | array | Same shape as `GET /api/v1/notifications` below |

**`GET /api/v1/watchdog`** → `WatchdogConfigResponse`:

| Field | Type | Description |
|-------|------|--------------|
| `enabled` | bool | `[watchdog].enabled` |
| `check_interval_secs` | number | `[watchdog].check_interval_secs` |
| `idle_timeout_secs` | number | `[watchdog].idle_timeout_secs` |
| `idle_action` | string | `"alert"` or `"kill"` |
| `idle_threshold_secs` | number | `[watchdog].idle_threshold_secs` |
| `extra_waiting_patterns` | string[] | `[watchdog].waiting_patterns` |

**`GET /api/v1/notifications`** → `NotificationsConfigResponse`:

```json
{ "webhooks": [ { "name": "ops", "url": "https://example.com/***", "events": ["lifecycle.*"], "min_severity": "warn" } ] }
```

`webhooks` is the union of the top-level `[[webhooks]]` list and the deprecated
`[[notifications.webhooks]]` list (top-level entries first) — see
[Config Reference § webhooks](config.md#webhooks). Each entry's `min_severity`
key is omitted entirely (not `null`) when the endpoint sets no floor. `url` is
masked (scheme and host kept, path/query redacted) since a Slack/Discord-style
webhook URL embeds its secret in the path — the same thing a failed delivery's
log line already redacts (see [Webhooks](#webhooks) below). The per-endpoint
`secret` field is never echoed back either — request signing was removed (see
[Webhooks](#webhooks) below).

## Auth

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/auth/token` | exempt from loopback; otherwise required | `{ "token": "<current bearer token>" }` |

Effectively localhost-only in practice: the auth middleware already exempts
`/api/v1/auth/*` for loopback clients, and a non-loopback caller still needs the very
token this endpoint would hand back.

## Usage

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/usage/sessions` | required in `public` | Exact per-session usage + per-repo rollups, pulpo-managed sessions only |
| GET | `/api/v1/usage/scan` | required in `public` | Read-only sweep of *all* local Claude/Codex/pi history, no pulpo session required |

Both are computed from the structured usage readers (exact token/cost data read
directly from each harness's own session files) — there is no output-scraping
fallback and no credential reading. This data is normally refreshed by the
watchdog while a session is `Working`/`Waiting`, and again the moment it reaches
`Done`; a `Done` session that still has no cost recorded (e.g. one from before this
on-demand path existed) is computed and persisted on demand the first time
`GET /api/v1/usage/sessions` (or `pulpo usage`) is queried, rather than reporting no
cost forever.

**`GET /api/v1/usage/sessions`** → `UsageSessionsResponse`:

| Field | Type | Description |
|-------|------|--------------|
| `node_name` | string | This node's name |
| `generated_at` | RFC 3339 string | When the response was computed |
| `sessions` | `SessionUsage[]` | One entry per pulpo-managed session |
| `repos` | `DimensionRollup[]` | Cost/token rollup per working directory |

`SessionUsage`: `session_id`, `session_name`, `workdir`, `usage_source` (`"claude-jsonl"` /
`"codex-jsonl"` / `"pi-jsonl"` / `null` if nothing was read yet), `total_tokens` (u64),
`cost_usd` (number \| null — null when the model has no priced rate, see
`[rates.<model>]` in the [Config Reference](config.md)).

`DimensionRollup`: `label` (the repo path), `session_count`, `total_tokens`,
`total_cost_usd` (number \| null).

**`GET /api/v1/usage/scan`** — query params `?since_days=<u32>` (default: all-time) and
`?by_worktree=true` (default: `false`, collapses worktrees/subdirectories onto their
origin repo) → `UsageScanResponse`:

| Field | Type | Description |
|-------|------|--------------|
| `node_name` | string | This node's name |
| `generated_at` | RFC 3339 string | When the scan ran |
| `window_days` | number \| null | Echoes `since_days`, `null` for all-time |
| `total_tokens` | u64 | Grand total across every agent found on disk |
| `total_cost_usd` | number \| null | Grand total cost (null if nothing was priceable) |
| `by_agent` | `ScanRollup[]` | Totals per agent (`claude`, `codex`) |
| `by_model` | `ScanRollup[]` | Totals per model, most expensive first |
| `by_repo` | `ScanRollup[]` | Totals per repo, most expensive first |

`ScanRollup`: `label` (agent name, model id, or repo path), `total_tokens`,
`total_cost_usd` (number \| null — always null for Codex, which has no priced rate
table, or an unrecognized model).

## Sessions

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/sessions` | List sessions (`?status=`, `?search=`, `?sort=`, `?order=`) |
| POST | `/api/v1/sessions` | Create (spawn) a new session |
| GET | `/api/v1/sessions/:id` | Get session details (`:id` resolves by UUID or name) |
| DELETE | `/api/v1/sessions/:id` | Remove a single session outright (refuses `Starting`/`Working`/`Waiting`; a `Starting` session is only removable once its backend is confirmed dead) |
| POST | `/api/v1/sessions/:id/stop` | Stop a running session (`?purge=true` to also remove the record) |
| POST | `/api/v1/sessions/:id/resume` | Resume a done or lost session |
| GET | `/api/v1/sessions/:id/output?lines=<n>` | Get captured terminal output (default 100 lines) |
| GET | `/api/v1/sessions/:id/output/download` | Download full output as a `.log` file |
| POST | `/api/v1/sessions/:id/input` | Send text input to a session |
| GET | `/api/v1/sessions/:id/interventions` | List watchdog interventions for this session |
| GET | `/api/v1/sessions/:id/stream` | WebSocket terminal stream |
| POST | `/api/v1/sessions/:id/harness-events` | Ingest a harness lifecycle event (posted by `pulpo hook <harness>`) |
| POST | `/api/v1/sessions/:id/handoff` | Spawn a new session inheriting this one's working directory and git worktree |
| POST | `/api/v1/sessions/cleanup` | Remove all done and lost sessions |

### The Session object

Every endpoint that returns a session (`list`, `get`, `create`, `resume`, `handoff`,
`list_runs`) returns this full shape:

| Field | Type | Description |
|-------|------|--------------|
| `id` | UUID | Session id |
| `name` | string | Unique, kebab-case |
| `workdir` | string | Working directory |
| `command` | string | The original command as given at spawn/schedule time — **not** the harness-rewritten form actually executed; the rewrite (adding `--session-id`, hook flags, etc.) is re-derived from this stored original at spawn/resume time |
| `description` | string \| null | Free-text note set at spawn time |
| `status` | string | `starting` \| `working` \| `waiting` \| `done` \| `lost` (see [Session Lifecycle](../operations/session-lifecycle.md)). Old six-state text (`creating`, `active`, `idle`, `ready`, `stopped`, `killed`) still deserializes on input, aliased onto the new states, for back-compat with older clients/stored data |
| `status_reason` | string \| null | Why the session is in `status` — only ever set for `waiting` (`idle` \| `needs_input:<reason>`) and `done` (`exited` \| `stopped` \| `idle_timeout` \| `budget_exceeded` \| `memory_pressure` — historical only, the intervention producing it was removed in September 2026, see [ROADMAP.md](https://github.com/darioblanco/pulpo/blob/main/ROADMAP.md) "Removed"); `null` for `starting`/`working`/`lost` |
| `exit_code` | number \| null | The agent process's exit code, once known (recorded from the `.code` exit marker, or from a hook-reported `SessionEnded` for harness-managed sessions) |
| `backend_session_id` | string \| null | tmux `$N` id |
| `output_snapshot` | string \| null | Last captured output (persisted snapshot; live tail comes from `output`/`stream`) |
| `metadata` | object \| null | Free-form key/value string map (also carries internal bookkeeping such as usage fields) |
| `ink` | string \| null | Historical only — the ink preset registry was removed; never set on new sessions |
| `intervention_code` | string \| null | `memory_pressure` \| `idle_timeout` \| `user_stop` \| `budget_exceeded`, or `null` if never intervened. An unrecognized/retired stored code (e.g. a historical `burn_rate` row from the removed burn-rate governor) also reads back as `null` — see [Config Reference](config.md) |
| `intervention_reason` | string \| null | Human-readable reason for the last intervention |
| `intervention_at` | RFC 3339 string \| null | When the last intervention happened |
| `last_output_at` | RFC 3339 string \| null | Last time output changed |
| `idle_since` | RFC 3339 string \| null | When the session went idle |
| `idle_threshold_secs` | number \| null | Per-session idle override (`null` = use global, `0` = never idle) |
| `worktree_path` | string \| null | Path to the git worktree, if one was created |
| `worktree_branch` | string \| null | Branch name for the worktree |
| `git_branch` / `git_commit` | string \| null | Current branch / short commit hash (watchdog-detected) |
| `git_files_changed` / `git_insertions` / `git_deletions` | number \| null | Working-tree diff stats (watchdog-detected) |
| `git_ahead` | number \| null | Commits ahead of the remote tracking branch |
| `runtime` | string | Always `"tmux"` for new sessions; `"docker"` only appears on historical rows (the docker runtime was removed) |
| `harness` | string \| null | Harness adapter id (`claude`, `codex`, `pi`) matched at spawn time, or `null` for a generic command |
| `harness_session_id` | string \| null | The harness's own conversation/session id, used to resume |
| `harness_last_event_at` | RFC 3339 string \| null | Last time a harness lifecycle event landed for this session; once set, the watchdog stops applying scrollback-heuristic detection for the signals that harness's adapter owns |
| `created_at` / `updated_at` | RFC 3339 string | Timestamps |

### Create Session (`POST /api/v1/sessions`)

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
  "term_program": "ghostty",
  "budget_cost_usd": 5.0
}
```

`name` is required. All other fields are optional; without a `command`, the session
falls back to the node's `default_command` or `$SHELL`. `idle_threshold_secs` overrides
the global idle threshold for this session (`null` = use global, `0` = never idle).
`worktree_base` specifies the branch to fork from (implies `worktree: true`). `runtime`
is kept for wire compatibility only — `"docker"` still deserializes but is rejected with
`400 Bad Request` (the docker runtime was removed; sessions run in tmux). `term_program`
is forwarded into the session shell as `TERM_PROGRAM` so agents can detect the outer
terminal's capabilities. `budget_cost_usd` sets a cost budget for this session — the
watchdog alerts at 80% and stops the session at 100%. Response: `201 Created`,
`{ "session": <Session> }`.

There is no cross-node targeting — a create request always runs on the `pulpod` that
receives it. To spawn on another machine, send the request to that machine directly (point
the CLI or an HTTP client at its address, e.g. `pulpo --url gpu-box spawn ...`).
`GET /api/v1/sessions/:id/stream` is node-local by the same principle (any client that can
reach this node can open it — there's no loopback-only check); remote terminal proxying
*across* nodes is intentionally out of scope.

### Handoff Session (`POST /api/v1/sessions/:id/handoff`)

```json
{
  "name": "my-api-2",
  "command": "codex 'implement PLAN.md'",
  "description": "Build from the plan",
  "budget_cost_usd": 5.0,
  "idle_threshold_secs": null,
  "term_program": "ghostty"
}
```

`:id` is the **source** session (resolved by ID or name, same as `GET .../sessions/:id`).
Every field is optional: `name` auto-generates as `<source>-2`, `-3`, ... when omitted;
without `command`, the new session opens a login shell. The new session inherits the
source's working directory and, if the source used one, its git worktree (`adopted`, not
copied — no new branch or checkout). Returns `201 Created` with the same shape as
`POST /api/v1/sessions`. `400 Bad Request` if the source's worktree no longer exists on
disk. See [Plan Then Build](../guides/plan-then-build.md).

### Stop / Resume / Cleanup

`POST /api/v1/sessions/:id/stop?purge=true` → `204 No Content` when this call actually
stopped a live session; `200 OK` when the session was already `done`/`lost` — a no-op on
status (it does **not** overwrite an existing `status_reason` like `exited` or
`budget_exceeded` with the generic `stopped`), though `?purge=true` still removes the
record either way. `404` if the session doesn't exist; `409 Conflict` covers no stop case
today (stop is idempotent-safe on an already-terminal session).

`POST /api/v1/sessions/:id/resume` → `200 OK` with the updated `Session`; `400 Bad
Request` if the session is `Working`/`Waiting`/`Starting` (cannot be resumed — still
running), `404` if not found. See [Resume Semantics](../operations/session-lifecycle.md#resume-semantics).

`POST /api/v1/sessions/cleanup` — removes every `Done`/`Lost` session → `200 OK`,
`CleanupResponse`:

| Field | Type | Description |
|-------|------|--------------|
| `sessions_deleted` | number | Session records removed |
| `worktrees_cleaned` | number | Git worktrees removed |
| `logs_cleaned` | number | Per-session output log files (`{id}.log`) removed |

### Remove (`DELETE /api/v1/sessions/:id`)

Removes a single session outright: purges the row, its intervention events, exit
markers, session log, and git worktree/harness dir — the same purge helper
`POST /api/v1/sessions/:id/stop?purge=true` and `POST /api/v1/sessions/cleanup` use.
→ `204 No Content`; `404` if the session doesn't exist; `409 Conflict` if it's
currently `Working`/`Waiting`, or `Starting` with a backend that's still alive —
stop it first (`pulpo stop`). A `Starting` session whose backend is already
confirmed dead (crashed between insert and finalize) is removable directly, since
there's no live backend left to stop. `pulpo rm <name-or-id>` (alias `remove`) is
the CLI equivalent.

### Output

`GET /api/v1/sessions/:id/output?lines=<n>` → `{ "output": "<captured tail>" }` (default
100 lines). `GET /api/v1/sessions/:id/output/download` streams the same content (up to
10,000 lines for a live session, or the full persisted snapshot for a finished one) as
`Content-Type: text/plain; charset=utf-8` with `Content-Disposition: attachment;
filename="<session-name>.log"`.

### Input

`POST /api/v1/sessions/:id/input` — body `{ "text": "hello" }` → `204 No Content`.
Writes the text into the session's terminal (e.g. to answer a permission prompt).

### Interventions

`GET /api/v1/sessions/:id/interventions` → `InterventionEventResponse[]`:

| Field | Type | Description |
|-------|------|--------------|
| `id` | number | Row id |
| `session_id` | string | — |
| `code` | string \| null | Same code values as `Session.intervention_code`; omitted from the JSON entirely (not `null`) when absent |
| `reason` | string | Human-readable reason |
| `created_at` | RFC 3339 string | — |

### Harness Events (`POST /api/v1/sessions/:id/harness-events`)

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
lifecycle event, applies the resulting state transition — including recording
`exit_code` for a hook-reported session end — and emits the existing SSE `session`
event (see [Harness Adapters](../architecture/harness-adapters.md)); no separate
notification channel.

Status codes, checked in this order:
- `404` if the session doesn't exist.
- `204 No Content` (no-op) if the session is already `Done` or `Lost` — a hook can
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

### Terminal Stream (`GET /api/v1/sessions/:id/stream`)

WebSocket upgrade; `400 Bad Request` if the session isn't `Working`/`Waiting`, `404` if it
doesn't exist. Binary frames carry raw PTY I/O in both directions. A JSON text frame
from the client can send a control message:

```json
{ "type": "resize", "cols": 120, "rows": 40 }
```

`resize` is the only control message today.

## Schedules

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/schedules` | List schedules |
| POST | `/api/v1/schedules` | Create a schedule |
| GET | `/api/v1/schedules/:id` | Get a schedule |
| PUT | `/api/v1/schedules/:id` | Update a schedule |
| DELETE | `/api/v1/schedules/:id` | Delete a schedule |
| GET | `/api/v1/schedules/:id/runs` | List up to the last 20 sessions this schedule fired |

`Schedule`:

| Field | Type | Description |
|-------|------|--------------|
| `id` | UUID | — |
| `name` | string | Kebab-case; becomes the prefix of every session name it fires (e.g. `nightly-20260331-0300`) |
| `cron` | string | 5-field cron expression |
| `command` | string | — |
| `workdir` | string | — |
| `ink` | string \| null | Historical only — never set on new schedules |
| `description` | string \| null | — |
| `runtime` | string \| null | Historical only; `"docker"` is rejected on create/update |
| `worktree` | bool \| null | — |
| `worktree_base` | string \| null | — |
| `budget_cost_usd` | number \| null | Applied to every session this schedule fires; watchdog alerts at 80%, stops at 100% |
| `enabled` | bool | — |
| `last_run_at` | RFC 3339 string \| null | — |
| `last_session_id` | string \| null | — |
| `last_attempted_at` | RFC 3339 string \| null | — |
| `last_error` | string \| null | — |
| `created_at` | RFC 3339 string | — |

`POST /api/v1/schedules` request (`CreateScheduleRequest`): `name` (required,
kebab-case, ≤128 chars — validated the same as session names), `cron` (required, valid
5-field expression), `command`, `workdir` (required), `description`, `runtime`,
`worktree`, `worktree_base`, `budget_cost_usd`. `400 Bad Request` for an invalid name,
an invalid cron expression, or `runtime: "docker"`; `409 Conflict` if the name is
already taken. Returns `201 Created` with the new `Schedule`.

`PUT /api/v1/schedules/:id` request (`UpdateScheduleRequest`) — every field optional and
only overwrites what's present: `cron`, `command`, `workdir`, `description`, `enabled`,
`runtime`, `worktree`, `worktree_base`, `budget_cost_usd`. `404` if not found, `400` for
an invalid cron or `runtime: "docker"`. Returns `200 OK` with the updated `Schedule`.

`DELETE /api/v1/schedules/:id` → `204 No Content`; `404` if not found.

## Events (SSE)

`GET /api/v1/events` — a `text/event-stream` connection, no request body, kept alive
with a 15-second ping. Each frame's `event:` field names one of five variants, and
`data:` is that variant's own struct serialized as JSON (**not** the canonical webhook
envelope described under [Webhooks](#webhooks) — SSE carries the raw internal event):

**`session`** (`SessionEvent`) — a session's status changed:

| Field | Type | Description |
|-------|------|--------------|
| `session_id` / `session_name` / `node_name` | string | — |
| `status` | string | `starting` \| `working` \| `waiting` \| `done` \| `lost` |
| `previous_status` | string \| null | — |
| `status_reason` | string \| null | Why the session is in `status` — see the [Session object](#the-session-object) above; omitted (not just empty) when not set. Only set for `waiting`/`done` |
| `output_snippet` | string \| null | — |
| `timestamp` | RFC 3339 string | — |
| `git_branch` / `git_commit` | string \| null | Omitted when absent |
| `git_insertions` / `git_deletions` / `git_files_changed` | number \| null | Omitted when absent |
| `pr_url` | string \| null | Omitted when absent |
| `error_status` | string \| null | Omitted when absent |
| `needs_input` | string \| null | **Deprecated, kept for one release.** The needs-input sub-reason (e.g. `permission`, `question`) when `status_reason` is `needs_input:<reason>`, populated from `status_reason` rather than written independently; omitted (not just empty) for a plain `waiting` (reason `idle`) session or any other status. New consumers should read `status_reason` instead |
| `total_input_tokens` / `total_output_tokens` | number \| null | Omitted when absent |
| `session_cost_usd` | number \| null | Omitted when absent |

**`session_deleted`** (`SessionDeletedEvent`) — a session was purged (`stop --purge` or
`pulpo cleanup`): `session_id`, `session_name`, `node_name`, `timestamp`.

**`usage_alert`** (`UsageAlertEvent`) — a budget/quota/rate-limit threshold fired:

| Field | Type | Description |
|-------|------|--------------|
| `session_id` / `session_name` / `node_name` | string | — |
| `alert_kind` | string | `budget_threshold` \| `quota_threshold` \| `rate_limit` |
| `message` | string | Human-readable, e.g. `"Cost $0.85 reached 80% of $1.00 budget"` |
| `cost_usd` / `budget_usd` | number \| null | Omitted when absent |
| `timestamp` | RFC 3339 string | — |

**`intervention`** (`SessionInterventionEvent`) — the watchdog forcibly stopped a
session:

| Field | Type | Description |
|-------|------|--------------|
| `session_id` / `session_name` / `node_name` | string | — |
| `code` | string | `memory_pressure` \| `idle_timeout` \| `budget_exceeded` \| `user_stop` |
| `reason` | string | — |
| `timestamp` | RFC 3339 string | — |

**`daemon`** (`DaemonEvent`) — a daemon-level event not tied to any one session:

| Field | Type | Description |
|-------|------|--------------|
| `node_name` | string | — |
| `subtype` | string | `db_unusable` (today's only subtype) |
| `message` | string | Human-readable description of what happened |
| `timestamp` | RFC 3339 string | — |

Fired once at startup, only when an unusable `state.db` is quarantined and replaced
with a fresh database — see [Release and Distribution](../operations/release-and-distribution.md)
"The daemon never crash-loops on a bad database".

```bash
curl -N http://localhost:7433/api/v1/events
```

## Webhooks

`[[webhooks]]` is pulpo's only outbound notification channel (see
[Config Reference § webhooks](config.md#webhooks) for the config fields). There
is no webhook management API — endpoints are configured in `config.toml` only.

Every session/usage-alert/intervention/daemon event that isn't purely internal
housekeeping (`session_deleted` is never forwarded) is converted to one canonical
envelope and POSTed to every endpoint whose filter admits it:

```json
{
  "schema_version": 1,
  "event_id": "b6b6c7b0-...",
  "type": "lifecycle",
  "subtype": "done",
  "severity": "info",
  "occurred_at": "2026-06-13T12:00:00Z",
  "node": "mac-mini",
  "session": {
    "id": "sess-1",
    "name": "fix-auth",
    "status": "done",
    "exit_code": 0,
    "git_branch": "feat/x",
    "pr_url": "https://github.com/org/repo/pull/9",
    "cost_usd": 2.5,
    "total_tokens": 1234000
  },
  "payload": {}
}
```

A `daemon` event carries no `session` key at all (it isn't session-scoped) and its
`payload` is just the human-readable message:

```json
{
  "schema_version": 1,
  "event_id": "d4b1e0aa-...",
  "type": "daemon",
  "subtype": "db_unusable",
  "severity": "critical",
  "occurred_at": "2026-09-16T03:00:00Z",
  "node": "mac-mini",
  "payload": { "message": "state.db failed its integrity check; quarantined as state.db.unusable-20260916T030000.123Z and replaced with a fresh database" }
}
```

| Field | Type | Description |
|-------|------|--------------|
| `schema_version` | number | Always `1` |
| `event_id` | UUID | Fresh per event; the idempotency key for at-least-once delivery (dedupe retries of the *same* event on it — there is no durable outbox, so an event that exhausts retries is simply dropped, not redelivered later) |
| `type` | string | `lifecycle` \| `usage_alert` \| `intervention` \| `daemon` (`fleet` is reserved from an earlier multi-node design; nothing emits it today) |
| `subtype` | string | For `lifecycle`: the new session status (`starting`/`working`/`waiting`/`done`/`lost`). For `usage_alert`: `alert_kind`. For `intervention`: the `InterventionCode` string. For `daemon`: `db_unusable` (today's only value) |
| `severity` | string | `info` \| `warn` \| `critical` — the value `min_severity` filters on. Lifecycle: `lost`→critical, `waiting`→warn, `done`→info if `status_reason` is `exited` (a clean end, same as the old `ready`'s severity) else warn (an explicit stop or an intervention code, same as the old `stopped`'s severity), else (`starting`/`working`) info. Usage alert: always `warn`. Intervention: `budget_exceeded`/`memory_pressure`→critical, else warn. Daemon: `db_unusable`→critical (today's only subtype) |
| `occurred_at` | RFC 3339 string | — |
| `node` | string | Emitting node's name |
| `session` | object \| omitted | `{ id, name, status, exit_code?, ink?, git_branch?, pr_url?, cost_usd?, total_tokens? }` — present for session-scoped events; omitted entirely for `daemon` (not session-scoped) |
| `payload` | object | Type-specific extras: `usage_alert` carries `cost_usd`/`budget_usd` (each omitted if absent); `intervention` carries `intervention_reason`; `daemon` carries `{ message }`; `lifecycle` is always `{}` |

**Request**: `POST` to the endpoint's `url`, `Content-Type: application/json`,
`User-Agent: pulpo/<version>`, `X-Pulpo-Event: <type>.<subtype>`,
`X-Pulpo-Event-Id: <event_id>`. There is **no request signing** — treat the URL itself
as the shared secret (a per-endpoint `secret`/HMAC option existed before and was
removed along with the durable outbox; a config still setting it is ignored with a
startup warning), or put the endpoint behind your own auth.

**Delivery model** — in-memory, best-effort, no persistence:
- **Retries**: the initial attempt plus 3 retries with ~1s/3s/9s backoff (4 attempts
  total). An event that fails every attempt is logged and dropped — never retried again,
  even across a restart.
- **Timeouts**: 5s connect timeout, 10s total per-attempt timeout (connect + send +
  response). An endpoint that accepts the connection and never responds is aborted and
  moves to the next retry rather than hanging forever.
- **Concurrency cap**: at most 16 webhook deliveries in flight at once, across every
  endpoint combined. Beyond that, a new delivery that would exceed the cap is dropped
  (and logged) immediately rather than queued — no worse than one that later exhausts
  its retries.
- **Redaction**: a failed delivery's log line never includes the request URL (stripped
  via `reqwest`'s `without_url()`) — a Slack/Discord-style webhook URL embeds its secret
  in the path, so it must not end up in `pulpod`'s own logs.
