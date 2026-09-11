# Config Reference

Default file: `~/.pulpo/config.toml`

All sections are optional. Pulpo runs with zero config.

Unknown config fields are rejected — except a short list of **retired keys** (see the
section at the bottom), which still parse from a config written before their removal, are
ignored, and are dropped the next time the config is saved. Any other unrecognized field
is rejected outright.
Pre-`sqlx` legacy databases are unsupported; if startup reports an unsupported legacy schema,
delete `~/.pulpo/state.db` and restart.

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
| `ready_ttl_secs` | u64 | `0` | Seconds after Ready before stop (0 = disabled) |
| `memory_threshold` | u8 | `90` | Memory usage % to trigger intervention |
| `breach_count` | u32 | `3` | Consecutive breaches before stop |
| `idle_threshold_secs` | u64 | `60` | Seconds of unchanged output before Active→Idle |
| `waiting_patterns` | string[] | `[]` | Extra patterns for waiting-for-input detection (appended to the built-in patterns) |
| `burn_ceiling_usd_per_hour` | float | — | Alert when a session's lifetime-average cost rate (USD/hour) exceeds this. Unset disables the check. |
| `burn_ceiling_tokens_per_hour` | integer | — | Alert when a session's lifetime-average token rate exceeds this — covers agents with no cost signal (e.g. Codex). Unset disables the check. |
| `burn_action` | string | `"alert"` | `"alert"` (emit a `usage_alert.burn_ceiling` event) or `"stop"` (also stop the session via the intervention path) when a burn ceiling is crossed |

For harnesses with their own lifecycle hooks (Claude Code, Codex, pi), once a session's
events start flowing, the watchdog stops applying its own scrollback-based *detection*
heuristics for that session: waiting-for-input pattern matching, the time-based
Active→Idle transition (`idle_threshold_secs`), and — for a harness whose adapter doesn't
own it (Codex has no error/rate-limit hook) — error/rate-limit scraping. The harness's own
events drive those transitions instead. Everything else still applies unconditionally,
including to harness-managed sessions: `idle_timeout_secs`/`idle_action` (alert/kill after
a session has sat idle too long), `memory_threshold`, `ready_ttl_secs`, and the budget/burn
fields. See [Harness Adapters](/architecture/harness-adapters).

## `[plans.<name>]`

Per-plan quota estimates, keyed by the plan name in a session's `auth_plan` (e.g. `max`,
`pro`). Anthropic does not publish subscription token allowances, so Claude "% of weekly
cap" and time-to-cap in `pulpo usage` are shown **only** when you supply an estimate here.
Codex quota is read exactly from the agent and needs no configuration.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `weekly_token_allowance` | integer | — | Estimated weekly token allowance for the plan |

```toml
[plans.max]
weekly_token_allowance = 500_000_000
```

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

## `[metrics]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Serve the Prometheus `/api/v1/metrics` endpoint |

Off by default. When enabled, every gauge (active sessions by status, cost today, quota %,
budget-breach/intervention counters) is computed from the current store state on each
scrape — pull-based and stateless, nothing persisted. Complements `[[webhooks]]` (push of
discrete events) with continuous dashboard state.

## `[[webhooks]]`

Each `[[webhooks]]` table is a delivery endpoint that subscribes to the universal event
stream. Pulpo POSTs the canonical event envelope (see
[the webhook example](https://github.com/darioblanco/pulpo/tree/main/contrib/examples/webhook-discord))
to every endpoint whose filter admits the event. Delivery is at-least-once from a durable
outbox with exponential backoff.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | — | Endpoint name (must be unique — used to track deliveries) |
| `url` | string | — | Webhook URL to POST events to |
| `events` | string[] | `[]` | `<type>.<subtype>` glob filter; empty means all events |
| `min_severity` | string | — | Drop events below this floor (`info` < `warn` < `critical`); absent means no floor |
| `secret` | string | — | Optional HMAC-SHA256 signing secret (`X-Pulpo-Signature`) |

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
secret = "optional-hmac-signing-secret"
```

Event types are `lifecycle`, `intervention`, and `usage_alert`; see the
[session lifecycle reference](/operations/session-lifecycle) and the linked webhook example
for the full event catalogue. (The envelope also reserves a `fleet` type from the earlier
multi-node design; nothing emits it today.)

### Legacy: `[notifications.webhooks]`

The nested `[[notifications.webhooks]]` form is **deprecated** but still read for
back-compat: any endpoints there are unioned with the top-level `[[webhooks]]` list at
startup. Prefer the top-level form for new configs. The fields are identical.

## `[notifications.vapid]`

The Web Push keys and action-token signing secret used by `pulpo`'s built-in push
notifications (see the [Push Notifications reference](/reference/push) for the wire
contract). Every field is auto-generated on first run and persisted here — there is
nothing to configure by hand.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `private_key` | string | auto-generated | Base64url P-256 VAPID private key (32 bytes) |
| `public_key` | string | auto-generated | Base64url P-256 VAPID public key (65 bytes), served by `GET /api/v1/push/vapid-key` |
| `action_secret` | string | auto-generated | Base64url 256-bit HMAC secret signing the "Stop session" push action tokens |

`action_secret` is generated independently of the VAPID key pair, so a config that
already has VAPID keys from before this field existed still gets one backfilled on the
next startup rather than left empty. None of these three fields is ever returned by
`GET /api/v1/config` or `GET /api/v1/notifications` — only the public key is exposed,
and only via the dedicated `/api/v1/push/vapid-key` endpoint.

## Retired keys (ignored with a warning)

These keys existed in earlier releases and are gone. A config file written before a given
removal still **loads**: the key is parsed, ignored, and dropped the next time the config
is saved (`watchdog.adopt_tmux` and `node.tag` additionally log a startup warning; the
rest are dropped silently). Do not set any of these in a new config — they have no effect.

| Key | Removed | Replacement |
|-----|---------|-------------|
| `[docker]` | Docker session runtime removed | Agents run in `tmux`; use `--worktree` for isolation |
| `[controller]` | Controller/node control plane removed (July 2026) | `pulpo --url <host:port>` for direct multi-machine access |
| `[inks.<name>]` | Ink preset registry removed (July 2026) | Command set directly per session/schedule; `pulpo schedule add --budget-cost <USD>` for recurring budgets |
| `[peers]` | Peer registry + Tailscale peer discovery removed (September 2026) | `pulpo --url <host:port>`, a saved web UI connection, or SSH — see [Control Your Agents From Anywhere](/guides/remote-control) |
| `notifications.discord` | Discord webhook notifier removed | `[[webhooks]]` to any HTTP endpoint (see `contrib/examples/webhook-discord` for a Discord relay) |
| `node.discovery_interval_secs` | Tailscale peer-discovery scan frequency; peer discovery removed (September 2026) | No replacement needed — `bind = "tailscale"` requires no discovery |
| `node.tag` | Reserved for a Tailscale-ACL-based peer scoping that was never built; its only reader was the removed peer discovery | No replacement needed |
| `watchdog.adopt_tmux` | Auto-adoption of external tmux sessions removed (September 2026) | Start sessions that matter with `pulpo spawn`, which gets harness hooks, a preset session id, and real resume |
