# Configuration Guide

Default config path: `~/.pulpo/config.toml`

Pulpo runs with zero config — all sections are optional with sensible defaults.

Breaking cleanup note:

- truly unknown config keys now fail at startup instead of being silently ignored; a
  short list of **retired** keys (superseded features — `watchdog.adopt_tmux`,
  `node.tag`, `[controller]`, `[peers]`, `[inks]`, `[docker]`, `[plans]`, the three
  `watchdog.burn_*` keys, ...) is the exception — those still load with a startup warning
  and are ignored (see [Config Reference](/reference/config) "Retired keys"). Nothing
  rewrites the config file on its own, so a retired key stays in the file, still
  ignored, until you remove it by hand.
- the config file is the source of truth — `pulpod` and the web UI only read it (there
  is no settings-editing API or UI). Change anything by editing the file directly and
  restarting `pulpod`.
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
bind = "local"               # "local", "public", "tailscale"
default_command = "claude"   # Optional fallback when spawn has no command
```

Bind modes:
- `local` (default) — binds to `127.0.0.1`, no auth
- `public` — binds to `0.0.0.0`, requires auth token
- `tailscale` — binds locally, auto-serves HTTPS over your tailnet via `tailscale serve`

## Watchdog

The watchdog monitors sessions for idle detection and the budget/burn breakers:

```toml
[watchdog]
enabled = true                # Enable watchdog (default: true)
check_interval_secs = 10      # How often to check (default: 10)
idle_timeout_secs = 600        # Seconds idle before action (default: 600)
idle_action = "alert"          # "alert" (mark idle) or "kill" (default: "alert")
idle_threshold_secs = 60       # Seconds of unchanged output before Active→Idle (default: 60)
waiting_patterns = ["custom prompt>"]  # Extra waiting-for-input patterns (default: [])
```

Per-session idle threshold: `pulpo spawn my-task --idle-threshold 0` (never idle) or `--idle-threshold 120` (2 minutes).

For harnesses with their own lifecycle hooks (Claude Code, Codex, pi), most of this idle
detection stops applying once a session's events start flowing — see
[Harness Adapters](/architecture/harness-adapters). See
[Session Lifecycle](/operations/session-lifecycle) for the full state-transition picture.

## Scheduler

Tuning for the built-in cron scheduler (`pulpo schedule add`):

```toml
[scheduler]
tick_secs = 60   # How often to check for due schedules, in seconds (default: 60, min: 1)
```

There's normally no reason to lower this — it exists mainly so the end-to-end test suite
doesn't have to wait a full minute per schedule check.

## Cost Rates

`[rates.<model>]` prices a model for exact cost accounting (`pulpo usage`). A cap on what a
session or schedule can spend is set directly on it (`--budget-cost`), not in this file —
see [Config Reference](/reference/config) for `[rates.<model>]` field-by-field.

## Notifications

`[[webhooks]]` is the only notification channel. Define one table per delivery endpoint;
each filters the universal event stream by `events` (`<type>.<subtype>` globs) and
`min_severity`:

```toml
[[webhooks]]
name = "ops"
url = "https://example.com/hooks/pulpo"
events = ["lifecycle.*", "usage_alert.*", "intervention.*"]  # empty means all events
min_severity = "warn"                       # info < warn < critical; omit for no floor
```

Delivery is a plain POST (no signing) from an in-memory queue: the initial attempt plus
up to 3 retries (~1s, 3s, 9s), then the event is logged and dropped — there's no durable
outbox, so nothing is retried after a restart. The older `[[notifications.webhooks]]`
form is deprecated but still read for back-compat (unioned with the top-level list). See
the [config reference](/reference/config#webhooks) for the glob forms and the full event
catalogue.

## Auth

Only used with `bind = "public"`. Auto-generated on first run:

```toml
[auth]
token = "auto-generated-base64url-token"  # 32 random bytes, base64url-encoded (43 chars)
```

For `local` and `tailscale` modes, auth is skipped.

## Multiple Machines

There is no `[controller]` section — controller/node relay mode was removed. Every `pulpod`
is standalone. There is also no `[peers]` section — manual peer configuration and Tailscale
peer discovery were removed for the same reason. A leftover `[controller]`, `[peers]`, or
`discovery_interval_secs` key from an older config still loads (it's parsed but ignored) and
is dropped the next time the config is saved — the same treatment already given the retired
`[docker]` runtime section.

To reach another machine, point the CLI or web UI at it directly (`pulpo --url <host:port>`,
a saved web UI connection, or SSH + `pulpo attach`) — see
[Control Your Agents From Anywhere](/guides/remote-control) for the daily workflow. For a
view across machines, point every node's `[[webhooks]]` at the same collector.

## Full Reference

For field-level details, see [Config Reference](/reference/config).
