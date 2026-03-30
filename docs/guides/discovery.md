# Discovery Guide

::: warning Scope
Discovery is an operational layer, not the core Pulpo contract. Learn sessions, runtimes, and lifecycle first, then add discovery when you need multi-node operation.
:::

This guide matters most for:

- users running Pulpo across multiple personal machines
- small teams with private infrastructure
- anyone who wants one control plane across a fleet instead of one host

If that is not you yet, you can safely skip discovery at first.

Pulpo derives the discovery method from the `bind` mode in `[node]`. No separate `[discovery]` section is needed.

## Tailscale (recommended)

```toml
[node]
name = "mac-mini"
bind = "tailscale"
tag = "pulpo"              # optional: filter by ACL tag
discovery_interval_secs = 30  # default: 30
```

Binds to the Tailscale interface IP, skips auth (delegated to WireGuard), and discovers peers via the local Tailscale API.

## mDNS

```toml
[node]
bind = "public"

[auth]
# token is auto-generated on first run
```

Zero-config LAN discovery. Activates when `bind = "public"` and no `seed` is set.

## Seed

```toml
[node]
bind = "public"
seed = "10.0.0.5:7433"
discovery_interval_secs = 30

[auth]
# token is auto-generated on first run
```

Bootstraps from a known peer and discovers its peers transitively.

## Local / Container

No discovery. `bind = "local"` (default) binds to `127.0.0.1`. `bind = "container"` binds to `0.0.0.0` without auth.

## Manual peers

You can always define peers directly in `[peers]`, regardless of bind mode.
