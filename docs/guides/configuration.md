# Configuration Guide

Default config path: `~/.pulpo/config.toml`

Pulpo runs with zero config — all sections are optional with sensible defaults.

Breaking cleanup note:

- unknown or deprecated config keys now fail at startup instead of being ignored
- pre-`sqlx` legacy databases are no longer upgraded in place; if Pulpo reports an unsupported legacy schema, delete `~/.pulpo/state.db` and restart

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
- `tailscale` — binds locally, auto-serves HTTPS via `tailscale serve`, peer discovery via Tailscale API
- `container` — binds to `0.0.0.0`, no auth (trusts container network isolation)

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

Define one `[[webhooks]]` table per delivery endpoint. Each filters the universal event
stream by `events` (`<type>.<subtype>` globs) and `min_severity`:

```toml
[[webhooks]]
name = "ops"
url = "https://example.com/hooks/pulpo"
events = ["lifecycle.*", "usage_alert.*", "intervention.*"]  # empty means all events
min_severity = "warn"                       # info < warn < critical; omit for no floor
secret = "optional-hmac-signing-secret"     # signs requests with X-Pulpo-Signature
```

The older `[[notifications.webhooks]]` form is deprecated but still read for back-compat
(unioned with the top-level list). See the [config reference](/reference/config#webhooks)
for the glob forms and the full event catalogue.

**Web Push** is the other delivery channel — standard Web Push (VAPID + ECE), straight to
the browser, no relay. Every subscriber gets lifecycle changes, budget/burn alerts, and
interventions; budget/burn alerts additionally carry a **"Stop session" action button**
right on the phone notification, so you can kill a runaway session from the lock screen
without opening the app. See the [Push Notifications reference](/reference/push) for the
subscribe flow, payload schema, and the action-token endpoint.

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

## Multi-Node

There is no `[controller]` section — controller/node relay mode was removed. Every `pulpod`
is standalone. A leftover `[controller]` section from an older config still loads (it's
parsed but ignored) and is dropped the next time the config is saved — the same treatment
already given the retired `[docker]` runtime section.

To reach another machine, point the CLI or web UI at it directly (`pulpo --node <name>`, a
saved web UI connection, or SSH + `pulpo attach`) — see the
[Discovery Guide](/guides/discovery) for peer setup and
[Control Your Agents From Anywhere](/guides/remote-control) for the daily workflow. For a
view across machines, point every node's `[[webhooks]]` at the same collector.

## Full Reference

For field-level details, see [Config Reference](/reference/config).
