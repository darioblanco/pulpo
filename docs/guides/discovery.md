# Discovery Guide

::: warning Scope
Discovery is an operational layer, not the core Pulpo contract. Learn sessions, runtimes, and lifecycle first, then add discovery when you need multi-node operation.
:::

Pulpo supports two methods for multi-node operation: **Tailscale auto-discovery** and **manual peer configuration**. The discovery method is derived from the `bind` mode in `[node]`. No separate `[discovery]` section is needed.

## No control plane

Discovery tells nodes about each other. It does **not** make any node authoritative over the
others — there is deliberately no control plane. Every `pulpod` is standalone:

- **Local sessions** are only visible and manageable on the node that owns them.
- **Remote sessions** are reached by pointing your CLI, web UI, or SSH session directly at
  that node — `pulpo --node <name>`, a saved connection in the web UI, or `ssh <node> &&
  pulpo attach <name>`.
- **`pulpo nodes`** lists known peers (name, address, status, session count) for convenience,
  but it is a directory, not a fleet-wide control surface.

> **Note:** Distributed discovery methods (mDNS, seed-based gossip) were removed to simplify the codebase. They may return in a future version. Use Tailscale discovery or manual `[peers]` config instead.

For a full example that combines discovery with remote execution and secrets,
see [Private Infrastructure With Tailscale And Secrets](/guides/private-infra-with-tailscale).

## Tailscale (recommended)

```toml
[node]
name = "mac-mini"
bind = "tailscale"
tag = "pulpo"              # optional: filter by ACL tag
discovery_interval_secs = 30  # default: 30
```

Binds locally, exposes itself over the tailnet via `tailscale serve`, and discovers peers via the local Tailscale API.

## Manual peers

You can define peers directly in `[peers]`, regardless of bind mode:

```toml
[peers]
mac = "10.0.0.1:7433"

[peers.linux]
address = "10.0.0.2:7433"
token = "secret"
```

Use this when nodes are not on the same Tailnet, or when you need explicit control over the peer list.

## Local / Container

No discovery. `bind = "local"` (default) binds to `127.0.0.1`. `bind = "container"` binds to `0.0.0.0` without auth.
