# Config Reference

Default file: `~/.pulpo/config.toml`

All sections are optional. Pulpo runs with zero config.

Unknown config fields are rejected. Deprecated keys are not silently accepted.
Pre-`sqlx` legacy databases are unsupported; if startup reports an unsupported legacy schema,
delete `~/.pulpo/state.db` and restart.

## `[node]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | hostname | Node display name |
| `port` | u16 | `7433` | HTTP listen port |
| `data_dir` | string | `~/.pulpo` | Data directory for SQLite, logs |
| `bind` | string | `"local"` | `"local"`, `"public"`, `"tailscale"`, `"container"` |
| `tag` | string | — | Tailscale ACL tag for filtering (e.g. `"pulpo"`) |
| `discovery_interval_secs` | u64 | `30` | How often to run peer discovery |
| `default_command` | string | — | Default command when spawn has no explicit command |
| `log_retain_days` | u32 | `7` | Days of rotated daemon logs (`logs/pulpod.log.*`) to keep (hourly rotation) |
| `capture_session_output` | bool | `false` | Mirror each session's full terminal output to `logs/<id>.log` via `tmux pipe-pane`. Off by default — the capture is unbounded and fills the disk on long/chatty sessions. Enable only for debugging; the watchdog reads the live tail from tmux scrollback and persists the last snapshot in the database regardless. |

## `[auth]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `token` | string | auto-generated | Auth token for `bind = "public"` |

Not needed for `local`, `tailscale`, or `container` modes. Pulpo still auto-generates one on first run so a node can be switched to `public` later without manual bootstrap.

## `[controller]` (retired)

Controller/node relay mode was removed — every `pulpod` is standalone, reached directly
(`pulpo --node <name>`, a saved web UI connection, or SSH). A leftover `[controller]`
section from a config written before the removal is tolerated: it still parses, is ignored,
and is dropped the next time the config is saved. Same treatment as the retired `[docker]`
session-runtime section.

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
| `adopt_tmux` | bool | `true` | Auto-adopt external tmux sessions |
| `idle_threshold_secs` | u64 | `60` | Seconds of unchanged output before Active→Idle |
| `waiting_patterns` | string[] | `[]` | Extra patterns for waiting-for-input detection (appended to 29 built-in patterns) |

## `[inks.<name>]` (retired)

Inks — a named-preset registry (command, description, secrets, runtime, budget) — were
removed. Command and secrets are set directly per session/schedule (`pulpo spawn --secret`,
`pulpo schedule add --secret`), and the recurring cost budget moved onto the schedule itself
(`pulpo schedule add --budget-cost <USD>`). A leftover `[inks.*]` section from a config
written before the removal is tolerated: it still parses, is ignored, and is dropped the
next time the config is saved. Same treatment as the retired `[docker]` and `[controller]`
sections.

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

## `[peers]` / `[peers.<name>]`

Short form:

```toml
[peers]
mac = "10.0.0.1:7433"
```

Extended form:

```toml
[peers.linux]
address = "10.0.0.2:7433"
token = "secret"
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `address` | string | — | `host:port` of the peer |
| `token` | string | — | Auth token for this peer (optional) |

`[peers]` is discovery and routing metadata used to resolve `--node <name>` to an address. It does not grant any node authority over another — there is no control plane.

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

Event types are `lifecycle`, `intervention`, `usage_alert`, and `fleet`; see the
[session lifecycle reference](/operations/session-lifecycle) and the linked webhook example
for the full event catalogue.

### Legacy: `[notifications.webhooks]`

The nested `[[notifications.webhooks]]` form is **deprecated** but still read for
back-compat: any endpoints there are unioned with the top-level `[[webhooks]]` list at
startup. Prefer the top-level form for new configs. The fields are identical.
