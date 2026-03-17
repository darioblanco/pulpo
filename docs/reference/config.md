# Config Reference

Default file: `~/.pulpo/config.toml`

All sections are optional. Pulpo runs with zero config.

## `[node]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | hostname | Node display name |
| `port` | u16 | `7433` | HTTP listen port |
| `data_dir` | string | `~/.pulpo/data` | Data directory for SQLite, logs |
| `bind` | string | `"local"` | `"local"`, `"public"`, `"tailscale"`, `"container"` |
| `tag` | string | — | Tailscale ACL tag for filtering (e.g. `"pulpo"`) |
| `seed` | string | — | Seed peer address for gossip discovery (e.g. `"10.0.0.5:7433"`) |
| `discovery_interval_secs` | u64 | `30` | How often to run peer discovery |

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
| `ready_ttl_secs` | u64 | `0` | Seconds after Ready before kill (0 = disabled) |
| `memory_threshold` | u8 | `90` | Memory usage % to trigger intervention |
| `breach_count` | u32 | `3` | Consecutive breaches before kill |

## `[inks.<name>]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `description` | string | — | Human-readable ink description |
| `command` | string | — | Shell command to run (e.g. `"claude -p 'review code'"`) |

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
| `events` | string[] | `["ready", "killed"]` | Events that trigger notifications |

## `[notifications.webhook]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `url` | string | — | Webhook URL to POST events to |
| `events` | string[] | `["ready", "killed"]` | Events that trigger notifications |
