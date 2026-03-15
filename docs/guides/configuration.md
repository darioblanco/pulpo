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
default_provider = "claude"  # Default provider when none specified
```

Bind modes:
- `local` (default) — binds to `127.0.0.1`, no auth, no discovery
- `public` — binds to `0.0.0.0`, requires auth token, enables mDNS or seed discovery
- `tailscale` — binds to Tailscale IP, auto-serves HTTPS via `tailscale serve`, peer discovery via Tailscale API
- `container` — binds to `0.0.0.0`, no auth (trusts container network isolation)

## Session Defaults

Override default values for new sessions:

```toml
[session_defaults]
provider = "claude"
model = "claude-sonnet-4-20250514"
mode = "autonomous"
max_turns = 50
max_budget_usd = 10.0
output_format = "stream-json"
```

These apply when the user doesn't specify values explicitly. Explicit request values always win.

## Inks

Inks are reusable agent role definitions. Each ink can set up to 6 fields:

```toml
[inks.reviewer]
description = "Code reviewer focused on correctness and security"
provider = "claude"
model = "claude-sonnet-4-20250514"
mode = "autonomous"
unrestricted = false
instructions = "You are a senior reviewer. Focus on correctness, security, and performance."

[inks.quick-fix]
provider = "codex"
mode = "autonomous"
unrestricted = true
instructions = "Fix the issue quickly with minimal changes."
```

Use with: `pulpo spawn auth-review --ink reviewer "Review the auth module"`

Instructions routing:
- **Claude**: instructions are passed as `--system-prompt`
- **Other providers** (Codex, Gemini, OpenCode): instructions are prepended to the prompt

## Watchdog

The watchdog monitors sessions for memory pressure, idle detection, and finished session cleanup:

```toml
[watchdog]
enabled = true                # Enable watchdog (default: true)
check_interval_secs = 10      # How often to check (default: 10)
idle_timeout_secs = 600        # Seconds idle before action (default: 600)
idle_action = "alert"          # "alert" (mark idle) or "kill" (default: "alert")
finished_ttl_secs = 0          # Seconds after Finished before kill (0 = disabled)
memory_threshold = 90          # Memory % to trigger intervention (default: 90)
breach_count = 3               # Consecutive breaches before kill (default: 3)
```

See [Session Lifecycle](/operations/session-lifecycle) for how the watchdog drives state transitions.

## Notifications

```toml
[notifications.discord]
webhook_url = "https://discord.com/api/webhooks/..."
events = ["finished", "killed", "lost"]   # default: ["finished", "killed"]

[notifications.webhook]
url = "https://example.com/hooks/pulpo"
events = ["finished", "killed", "lost"]
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
