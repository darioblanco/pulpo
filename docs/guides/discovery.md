# Discovery Guide

Pulpo supports three discovery methods via `[discovery]`.

## mDNS (default)

```toml
[auth]
bind = "public"

[discovery]
method = "mdns"
```

Best for local networks.

## Tailscale

```toml
[discovery]
method = "tailscale"
tag = "pulpo"
interval_secs = 30
```

Best for tailnet-wide discovery.

## Seed

```toml
[discovery]
method = "seed"
seed = "10.0.0.5:7433"
interval_secs = 30
```

Best when one stable node can bootstrap the rest.

## Manual peers

You can always define peers directly in `[peers]`, regardless of discovery method.
