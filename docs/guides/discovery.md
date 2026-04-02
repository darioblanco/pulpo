# Discovery Guide

::: warning Scope
Discovery is an operational layer, not the core Pulpo contract. Learn sessions, runtimes, and lifecycle first, then add discovery when you need multi-node operation.
:::

Pulpo supports two methods for multi-node operation: **Tailscale auto-discovery** and **manual peer configuration**. The discovery method is derived from the `bind` mode in `[node]`. No separate `[discovery]` section is needed.

## Control-plane boundary

Discovery tells nodes about each other. It does **not** make every node an equally capable control plane.

- **Standalone node**: local-only view and actions.
- **Managed node**: local sessions remain visible and manageable on that node, but fleet-wide view and cross-node actions belong to the controller.
- **Controller node**: canonical fleet-wide visibility and cross-node control surface, including remote create and targeted schedule execution.

In the web UI, managed nodes should point you at the configured controller instead of pretending their fleet view is authoritative. The node dashboard remains useful for local sessions, but any fleet-wide create, stop, resume, or scheduled execution flow should go through the controller.

Before a node can participate in the fleet, enroll it on the controller and issue its node token:

```bash
pulpo --node controller-node nodes enroll gpu-box
pulpo --node controller-node nodes enrolled
```

Discovery tells the controller where a node is. Enrollment tells the controller which nodes are trusted members of the fleet.

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

Binds locally, exposes itself over the tailnet via `tailscale serve`, discovers peers via the local Tailscale API, and still uses enrolled node tokens for node-to-controller identity.

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
