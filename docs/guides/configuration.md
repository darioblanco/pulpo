# Configuration Guide

Default config path: `~/.pulpo/config.toml`

Pulpo runs with zero config — all sections are optional with sensible defaults.

## Minimal Example

```toml
[node]
name = "mac-mini"
```

## Node

```toml
[node]
name = "mac-mini"           # Node name (default: hostname)
port = 7433                  # HTTP port (default: 7433)
data_dir = "~/.pulpo/data"   # Data directory (default: ~/.pulpo/data)
bind = "local"               # "local", "public", "tailscale", "container"
```

Bind modes:
- `local` (default) — binds to `127.0.0.1`, no auth, no discovery
- `public` — binds to `0.0.0.0`, requires auth token, enables mDNS or seed discovery
- `tailscale` — binds to Tailscale IP, auto-serves HTTPS via `tailscale serve`, peer discovery via Tailscale API
- `container` — binds to `0.0.0.0`, no auth (trusts container network isolation)

## Inks

Inks are reusable command templates. Each ink has 2 fields:

```toml
[inks.reviewer]
description = "Code reviewer focused on correctness and security"
command = "claude -p 'review this code for correctness, security, and performance'"

[inks.quick-fix]
description = "Quick fix with Codex"
command = "codex --quiet 'Fix the issue quickly with minimal changes.'"
```

Use with: `pulpo spawn auth-review --ink reviewer`

## Watchdog

The watchdog monitors sessions for memory pressure, idle detection, and ready session cleanup:

```toml
[watchdog]
enabled = true                # Enable watchdog (default: true)
check_interval_secs = 10      # How often to check (default: 10)
idle_timeout_secs = 600        # Seconds idle before action (default: 600)
idle_action = "alert"          # "alert" (mark idle) or "kill" (default: "alert")
idle_threshold_secs = 60       # Seconds of unchanged output before Active→Idle (default: 60)
ready_ttl_secs = 0             # Seconds after Ready before kill (0 = disabled)
memory_threshold = 90          # Memory % to trigger intervention (default: 90)
breach_count = 3               # Consecutive breaches before kill (default: 3)
waiting_patterns = ["custom prompt>"]  # Extra waiting-for-input patterns (default: [])
```

Per-session idle threshold: `pulpo spawn my-task --idle-threshold 0` (never idle) or `--idle-threshold 120` (2 minutes).

See [Session Lifecycle](/operations/session-lifecycle) for how the watchdog drives state transitions.

## Notifications

```toml
[notifications.discord]
webhook_url = "https://discord.com/api/webhooks/..."
events = ["ready", "killed", "lost"]      # default: ["ready", "killed"]

[notifications.webhook]
url = "https://example.com/hooks/pulpo"
events = ["ready", "killed", "lost"]
```

## Peers

Manual peer entries coexist with automatic discovery:

```toml
[peers]
mac = "10.0.0.1:7433"

[peers.linux]
address = "10.0.0.2:7433"
token = "secret"
```

See [Discovery Guide](/guides/discovery) for automatic peer discovery options.

## Auth

Only used with `bind = "public"`. Auto-generated on first run:

```toml
[auth]
token = "auto-generated-uuid"
```

For `local`, `tailscale`, and `container` modes, auth is skipped.

## Full Reference

For field-level details, see [Config Reference](/reference/config).
