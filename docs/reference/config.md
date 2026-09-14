# Config Reference

Default file: `~/.pulpo/config.toml`

All sections are optional. Pulpo runs with zero config.

Any config key that isn't recognized — a leftover from a removed feature, a typo, anything
not listed below, at any nesting level — is logged once at startup (`config: unknown key
'<path>' ignored`) and otherwise ignored; it never fails startup. Recognized keys with an
invalid value (e.g. `bind = "container"`) still fail loudly, since that's a mistake in a
real setting, not an unrecognized one.
Pre-`sqlx` legacy databases are unsupported, but `pulpod` never crash-loops on one: an
unusable `state.db` (corrupt, an unsupported legacy schema, or a downgrade) is quarantined
as `state.db.unusable-<UTC timestamp>` and a fresh database is created in its place
automatically — no manual deletion needed. Every startup against an existing database also
backs it up to `state.db.pre-<version>` before migrating. See
[Release and Distribution](../operations/release-and-distribution.md) "Upgrading `pulpod`"
for the full recovery/backup behavior.

The config file is the source of truth: `pulpod` and the web UI only read it (see
[`GET /api/v1/config`](api.md#node--config)). To change anything, edit the file
by hand and restart `pulpod` — there is no API or UI to write it back, so a retired key
you remove yourself simply stays gone; one you leave in place stays in the file (ignored)
until you edit it out.

## `[node]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | hostname | Node display name |
| `port` | u16 | `7433` | HTTP listen port |
| `data_dir` | string | `~/.pulpo` | Data directory for SQLite, logs |
| `bind` | string | `"local"` | `"local"`, `"public"`, `"tailscale"` |
| `default_command` | string | — | Default command when spawn has no explicit command |
| `log_retain_days` | u32 | `7` | Days of rotated daemon logs (`logs/pulpod.log.*`) to keep (hourly rotation) |
| `capture_session_output` | bool | `false` | Mirror each session's full terminal output to `logs/<id>.log` via `tmux pipe-pane`. Off by default — the capture is unbounded and fills the disk on long/chatty sessions. Enable only for debugging; the watchdog reads the live tail from tmux scrollback and persists the last snapshot in the database regardless. |

Every pulpo-managed session gets `PULPO_URL=http://127.0.0.1:<port>` exported alongside
`PULPO_SESSION_ID`/`PULPO_SESSION_NAME` — `pulpo hook` reads it to reach the daemon, so
hooks work correctly whatever `port` is configured here, not just the default `7433`.

## `[auth]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `token` | string | auto-generated | Auth token for `bind = "public"` |

Not needed for `local` or `tailscale` modes. Pulpo still auto-generates one on first run so a node can be switched to `public` later without manual bootstrap.

## `[watchdog]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Enable/disable watchdog |
| `check_interval_secs` | u64 | `10` | Check interval in seconds |
| `idle_timeout_secs` | u64 | `600` | Seconds idle before action triggers |
| `idle_action` | string | `"alert"` | `"alert"` (mark idle) or `"kill"` |
| `idle_threshold_secs` | u64 | `60` | Seconds of unchanged output before Active→Idle |
| `waiting_patterns` | string[] | `[]` | Extra patterns for waiting-for-input detection (appended to the built-in patterns) |

For harnesses with their own lifecycle hooks (Claude Code, Codex, pi), once a session's
events start flowing, the watchdog stops applying its own scrollback-based *detection*
heuristics for that session: waiting-for-input pattern matching, the time-based
Active→Idle transition (`idle_threshold_secs`), and — for a harness whose adapter doesn't
own it (Codex has no error/rate-limit hook) — error/rate-limit scraping. The harness's own
events drive those transitions instead. Everything else still applies unconditionally,
including to harness-managed sessions: `idle_timeout_secs`/`idle_action` (alert/kill after
a session has sat idle too long) and the budget-cost fields (set per session/schedule, not
here — see `pulpo spawn --budget-cost` and `pulpo schedule add --budget-cost`). See
[Harness Adapters](../architecture/harness-adapters.md).

## `[scheduler]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `tick_secs` | u64 | `60` | How often the built-in cron scheduler checks for due schedules, in seconds. Clamped to a minimum of 1. |

The production default (60) matches cron's own minute granularity. There's normally no
reason to change it — see `pulpo schedule add` for defining schedules themselves.

## `[rates.<model>]`

Per-model cost rates in **USD per million tokens**, used to turn exact token counts into
cost. Pulpo ships a built-in table (Opus / Sonnet / Haiku), but it stays model-agnostic: a
model with no built-in rate still reports exact tokens with **cost withheld** rather than
guessed. Add a `[rates.<model>]` section to price a new model — or to reprice an existing
one — without waiting for a code change.

The `<model>` key is matched **case-insensitively as a substring** of the model ID, so
`[rates."claude-opus-4-9"]` matches that exact ID while `[rates.opus]` reprices the whole
family. The **most specific (longest) matching key wins**, and any override beats the
built-in table. Restart `pulpod` after editing rates.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `input` | float | — | USD per 1M uncached input tokens (required) |
| `output` | float | — | USD per 1M output tokens (required) |
| `cache_read` | float | `0.0` | USD per 1M cache-read tokens |
| `cache_write_5m` | float | `0.0` | USD per 1M 5-minute cache-write tokens |
| `cache_write_1h` | float | `0.0` | USD per 1M 1-hour cache-write tokens |

```toml
# Price a brand-new model the built-in table doesn't know yet.
[rates."claude-opus-4-9"]
input = 5.0
output = 25.0
cache_read = 0.5
cache_write_5m = 6.25
cache_write_1h = 10.0
```

## `[[webhooks]]`

`[[webhooks]]` is pulpo's only notification channel — each table is a delivery endpoint
that subscribes to the universal event stream. Pulpo POSTs the canonical event envelope
(see [the webhook example](https://github.com/darioblanco/pulpo/tree/main/contrib/examples/webhook-discord))
to every endpoint whose filter admits the event. Delivery is a **plain POST** from an
in-memory queue: the initial attempt plus up to 3 retries (~1s, 3s, 9s), and an event
that exhausts every attempt is logged and dropped — there is no persistence, so nothing
survives a restart or is retried after the daemon gives up. Delivery is also bounded — a
5s connect / 10s total per-attempt timeout, and at most 16 deliveries in flight across
every endpoint at once — and a failed delivery's log line never includes the URL itself
(it's the shared secret); see [API Reference § Webhooks](api.md#webhooks) for
the full envelope shape and these bounds.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | — | Endpoint name (must be unique — used to identify deliveries in logs) |
| `url` | string | — | Webhook URL to POST events to |
| `events` | string[] | `[]` | `<type>.<subtype>` glob filter; empty means all events |
| `min_severity` | string | — | Drop events below this floor (`info` < `warn` < `critical`); absent means no floor |

`events` patterns are matched against the event's `"<type>.<subtype>"` key:

- exact — `lifecycle.idle`
- prefix glob — `lifecycle.*` (any subtype of `lifecycle`)
- bare type — `lifecycle` (also any subtype of `lifecycle`)
- `*` — everything

```toml
[[webhooks]]
name = "ops"
url = "https://example.com/hooks/pulpo"
events = ["lifecycle.*", "usage_alert.*", "intervention.*"]
min_severity = "warn"
```

Each request carries `Content-Type: application/json`, `User-Agent: pulpo/<version>`,
`X-Pulpo-Event: <type>.<subtype>`, and `X-Pulpo-Event-Id: <uuid>` (a fresh id per event —
there's no durable outbox to dedupe retries against, but a receiver that wants
idempotency can still key on it). There is no request signing; treat the URL itself as
the shared secret, or put the endpoint behind your own auth.

Event types are `lifecycle`, `intervention`, and `usage_alert`; see the
[session lifecycle reference](../operations/session-lifecycle.md) and the linked webhook example
for the full event catalogue. (The envelope also reserves a `fleet` type from the earlier
multi-node design; nothing emits it today.)

### Legacy: `[notifications.webhooks]`

The nested `[[notifications.webhooks]]` form is **deprecated** but still read for
back-compat: any endpoints there are unioned with the top-level `[[webhooks]]` list at
startup. Prefer the top-level form for new configs. The fields are identical.

## Unknown keys

Any key not documented above — including one left over from a removed feature
(an old `[docker]`, `[controller]`, `[inks.<name>]`, `[peers]`, `[metrics]`,
`[plans.<name>]`, `[notifications.vapid]`/`.discord`, a per-webhook `secret`, a
retired `watchdog.*` or `node.*` setting, or a plain typo) — is logged once at
startup and ignored, at any nesting level; it never fails startup. Nothing
rewrites the file on its own to clean these up (`pulpod` only ever saves once,
the first time it runs with an empty `auth.token`), so an unknown key stays in
the file, still ignored, until you edit it out by hand.
