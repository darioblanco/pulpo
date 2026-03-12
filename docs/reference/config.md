# Config Reference

Default file: `~/.pulpo/config.toml`

All sections are optional. Pulpo runs with zero config.

## `[node]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | hostname | Node display name |
| `port` | u16 | `7433` | HTTP listen port |
| `data_dir` | string | `~/.pulpo/data` | Data directory for SQLite, culture, logs |
| `bind` | string | `"local"` | `"local"`, `"public"`, `"tailscale"`, `"container"` |
| `default_provider` | string | `"claude"` | Default provider when none specified |
| `tag` | string | — | Tailscale ACL tag for filtering (e.g. `"pulpo"`) |
| `seed` | string | — | Seed peer address for gossip discovery (e.g. `"10.0.0.5:7433"`) |
| `discovery_interval_secs` | u64 | `30` | How often to run peer discovery |

## `[session_defaults]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `provider` | string | — | Default provider for new sessions |
| `model` | string | — | Default model |
| `mode` | string | — | `"interactive"` or `"autonomous"` |
| `max_turns` | u32 | — | Maximum agent turns |
| `max_budget_usd` | f64 | — | Maximum budget in USD |
| `output_format` | string | — | Output format (e.g. `"stream-json"`) |

Session defaults fill in values when the user doesn't specify them. Priority: explicit request > ink > session_defaults > node default_provider > Claude.

## `[auth]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `token` | string | auto-generated | Auth token for `bind = "public"` |

Not needed for `local`, `tailscale`, or `container` modes.

## `[watchdog]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Enable/disable watchdog |
| `check_interval_secs` | u64 | `10` | Check interval in seconds |
| `idle_timeout_secs` | u64 | `600` | Seconds idle before action triggers |
| `idle_action` | string | `"alert"` | `"alert"` (mark idle) or `"kill"` |
| `finished_ttl_secs` | u64 | `0` | Seconds after Finished before kill (0 = disabled) |
| `memory_threshold` | u8 | `90` | Memory usage % to trigger intervention |
| `breach_count` | u32 | `3` | Consecutive breaches before kill |

## `[inks.<name>]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `description` | string | — | Human-readable ink description |
| `provider` | string | — | Agent provider (`claude`, `codex`, `gemini`, `opencode`) |
| `model` | string | — | Model override |
| `mode` | string | — | `"interactive"` or `"autonomous"` |
| `unrestricted` | bool | — | Disable safety guardrails |
| `instructions` | string | — | Instructions (system prompt for Claude, prompt prepend for others) |

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

## `[notifications.discord]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `webhook_url` | string | — | Discord webhook URL |
| `events` | string[] | `["finished", "killed"]` | Events that trigger notifications |

## `[notifications.webhook]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `url` | string | — | Webhook URL to POST events to |
| `events` | string[] | `["finished", "killed"]` | Events that trigger notifications |

## `[culture]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `remote` | string | — | Git remote URL for cross-node sync |
| `inject` | bool | `true` | Inject culture context into new sessions |
| `ttl_days` | u64 | `90` | Days before entries become stale |
| `curator` | string | — | Provider for standalone curation sessions |
| `sync_interval_secs` | u64 | `300` | Background sync interval |
| `sync_scopes` | string[] | — | Only sync these scopes (empty = all) |

Culture is stored as markdown files in `<data_dir>/culture/` in a local git repo with AGENTS.md format.
