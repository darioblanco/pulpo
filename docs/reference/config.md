# Config Reference

Default file: `~/.pulpo/config.toml`

All sections are optional. Pulpo runs with zero config.

## `[node]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | hostname | Node display name |
| `port` | u16 | `7433` | HTTP listen port |
| `data_dir` | string | `~/.pulpo` | Data directory for SQLite, logs |
| `bind` | string | `"local"` | `"local"`, `"public"`, `"tailscale"`, `"container"` |
| `tag` | string | — | Tailscale ACL tag for filtering (e.g. `"pulpo"`) |
| `discovery_interval_secs` | u64 | `30` | How often to run peer discovery |
| `default_command` | string | — | Default command when spawn has no explicit command or ink command |

## `[auth]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `token` | string | auto-generated | Auth token for `bind = "public"` |

Not needed for `local`, `tailscale`, or `container` modes. Pulpo still auto-generates one on first run so a node can be switched to `public` later without manual bootstrap.

## `[master]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Promote this node to master mode |
| `address` | string | — | Master URL for worker mode; when set, this node becomes a worker |
| `token` | string | — | Optional bearer token used by workers when talking to a public master |
| `stale_timeout_secs` | u64 | `300` | Seconds before the master marks a silent worker as lost |

Role rules:

- `enabled = true` and `address` unset => master node
- `address` set and `enabled = false` => worker node
- neither set => standalone node
- `enabled` and `address` are mutually exclusive

Auth rules:

- In `public` mode, a master should expose `auth.token`, and workers should set `master.token`.
- In `tailscale`, `local`, and `container` modes, bearer auth is optional because network isolation is the trust boundary.

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

## `[inks.<name>]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `description` | string | — | Human-readable ink description |
| `command` | string | — | Shell command to run (e.g. `"claude -p 'review code'"`) |
| `secrets` | string[] | `[]` | Stored secret names to inject when the ink is used |
| `runtime` | string | — | Default runtime for this ink: `tmux` or `docker` |

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

## `[docker]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `image` | string | `"ubuntu:latest"` | Docker image for Docker runtime containers |
| `volumes` | string[] | `["~/.claude:/root/.claude:ro", "~/.codex:/root/.codex:ro", "~/.gemini:/root/.gemini:ro"]` | Extra host mounts passed to Docker |

```toml
[docker]
image = "my-agents-image:latest"  # Image with your agent tools installed
```

Use with `pulpo spawn --runtime docker` to run sessions in isolated Docker containers.

## `[notifications.discord]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `webhook_url` | string | — | Discord webhook URL |
| `events` | string[] | `[]` | Event filter; empty means all events |

## `[notifications.webhooks]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | — | Human-readable endpoint name |
| `url` | string | — | Webhook URL to POST events to |
| `events` | string[] | `[]` | Event filter; empty means all events |
| `secret` | string | — | Optional HMAC signing secret |
