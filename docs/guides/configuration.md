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
data_dir = "~/.pulpo"        # Data directory (default: ~/.pulpo)
bind = "local"               # "local", "public", "tailscale", "container"
default_command = "claude"   # Optional fallback when spawn has no command
```

Bind modes:
- `local` (default) — binds to `127.0.0.1`, no auth, no discovery
- `public` — binds to `0.0.0.0`, requires auth token. Use manual `[peers]` config for multi-node.
- `tailscale` — binds to Tailscale IP, auto-serves HTTPS via `tailscale serve`, peer discovery via Tailscale API
- `container` — binds to `0.0.0.0`, no auth (trusts container network isolation)

## Inks

Inks are reusable command templates with optional runtime and secret defaults:

```toml
[inks.reviewer]
description = "Code reviewer focused on correctness and security"
command = "claude -p 'review this code for correctness, security, and performance'"

[inks.quick-fix]
description = "Quick fix with Codex"
command = "codex --quiet 'Fix the issue quickly with minimal changes.'"

[inks.docker-coder]
description = "Docker-isolated coder with secrets"
command = "claude --dangerously-skip-permissions -p 'Implement the changes'"
runtime = "docker"
secrets = ["GH_WORK", "ANTHROPIC_KEY"]
```

| Field | Type | Description |
|-------|------|-------------|
| `description` | string | Human-readable description |
| `command` | string | Command template to run |
| `runtime` | string | Default runtime: `tmux` or `docker` (overridden by `--runtime` on spawn) |
| `secrets` | string[] | Default secrets to inject (merged with `--secret` on spawn) |

Use with: `pulpo spawn auth-review --ink reviewer`

Inks can also be managed via CLI: `pulpo ink list`, `pulpo ink add <NAME> --command ...`, etc.

## Watchdog

The watchdog monitors sessions for memory pressure, idle detection, and ready session cleanup:

```toml
[watchdog]
enabled = true                # Enable watchdog (default: true)
check_interval_secs = 10      # How often to check (default: 10)
idle_timeout_secs = 600        # Seconds idle before action (default: 600)
idle_action = "alert"          # "alert" (mark idle) or "kill" (default: "alert")
idle_threshold_secs = 60       # Seconds of unchanged output before Active→Idle (default: 60)
ready_ttl_secs = 0             # Seconds after Ready before stop (0 = disabled)
memory_threshold = 90          # Memory % to trigger intervention (default: 90)
breach_count = 3               # Consecutive breaches before stop (default: 3)
adopt_tmux = true              # Auto-adopt external tmux sessions
waiting_patterns = ["custom prompt>"]  # Extra waiting-for-input patterns (default: [])
```

Per-session idle threshold: `pulpo spawn my-task --idle-threshold 0` (never idle) or `--idle-threshold 120` (2 minutes).

See [Session Lifecycle](/operations/session-lifecycle) for how the watchdog drives state transitions.

## Notifications

```toml
[notifications.discord]
webhook_url = "https://discord.com/api/webhooks/..."
events = ["ready", "stopped", "lost"]      # empty means all events

[[notifications.webhooks]]
name = "primary"
url = "https://example.com/hooks/pulpo"
events = ["ready", "stopped", "lost"]
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

## Docker Runtime

Run agents in Docker containers for isolation:

```toml
[docker]
image = "my-agents-image:latest"   # Docker image with agent tools installed
volumes = [
  "~/.claude:/root/.claude:ro",
  "~/.codex:/root/.codex:ro",
]
```

Use with `--runtime docker` flag:

```bash
pulpo spawn risky --runtime docker -- claude --dangerously-skip-permissions -p "refactor"
```

The workdir is mounted at `/workspace` inside the container. The agent can read and write the session repo, plus any extra host paths you mount with `volumes`.

## Full Reference

For field-level details, see [Config Reference](/reference/config).
