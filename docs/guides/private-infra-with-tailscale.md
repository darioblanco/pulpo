# Private Infrastructure With Tailscale And Secrets

This recipe shows how to run Pulpo across your own private machines and keep
agent execution close to private repos, VPN-only services, or internal systems.

It combines:

- Tailscale-based node discovery
- controller/node control-plane routing
- per-node secret management
- one control plane across multiple machines you own

## What Problem This Solves

Hosted coding agents are often inconvenient when:

- the runtime needs private-network access
- your repos or services live behind VPN or Tailscale
- you want agents to run on machines you control
- different machines have different capabilities

Pulpo fits that model by keeping the runtime on your infrastructure while
keeping the control model consistent across nodes.

## Example Setup

Assume:

- `mac-mini` is an always-on machine that will act as the Pulpo controller
- `gpu-box` is a node with access to private repos and internal services
- `laptop` is where you are currently working
- all three machines are already on the same Tailnet

## 1. Configure The Controller

On `mac-mini`, enable controller mode over Tailscale:

```toml
[node]
name = "mac-mini"
bind = "tailscale"
tag = "pulpo"
discovery_interval_secs = 30

[controller]
enabled = true
```

Then start or restart `pulpod`.

This makes the node:

- bind to the local loopback interface and expose itself over the tailnet with `tailscale serve`
- discover peer Pulpo nodes via the local Tailscale API
- act as the canonical fleet control plane
- issue and verify enrolled node identities for fleet membership

Before configuring a managed node, enroll it on the controller and mint its node token:

```bash
pulpo --node mac-mini nodes enroll gpu-box
pulpo --node mac-mini nodes enrolled
```

## 2. Configure A Managed Node

On `gpu-box`, point Pulpo at the controller:

```toml
[node]
name = "gpu-box"
bind = "tailscale"
tag = "pulpo"
discovery_interval_secs = 30

[controller]
address = "https://mac-mini.tailnet-name.ts.net"
token = "node-token-issued-by-controller"
```

Even in tailscale mode, `controller.token` is required. Tailscale protects network reachability; the node token identifies the enrolled node inside the fleet.

Once both nodes are running, the controller should discover the managed node and start receiving node events.

Check from your laptop against the controller:

```bash
pulpo --node mac-mini nodes
pulpo --node mac-mini nodes enrolled
```

## 3. Store Secrets On The Worker That Will Execute The Work

Secrets are stored per-node, which is usually what you want for private
infrastructure.

For example, if `gpu-box` has access to the private repo and should run the
session, store the secret there:

```bash
pulpo --node gpu-box secret set GH_WORK ghp_work_xxxxxxxxxxxx --env GITHUB_TOKEN
pulpo --node gpu-box secret list
```

This keeps the secret associated with the machine that will actually execute the
session.

## 4. Run A Session On The Worker

For cross-node work, target the controller and tell it which node should execute the session:

```bash
pulpo --node mac-mini spawn review-backend \
  --workdir ~/repos/backend \
  --node gpu-box \
  --secret GH_WORK \
  -- claude -p "Review this service for correctness, security issues, and missing tests."
```

From your laptop, you are still in control, but the runtime lives on `gpu-box`. The controller is the canonical fleet view and the cross-node write path.

That means:

- the agent executes near the private repo and services
- the fleet-wide session lifecycle is still visible from the controller
- you can inspect status, logs, and recovery through the same Pulpo interface

## 5. Check Progress Remotely

From your laptop:

```bash
pulpo --node mac-mini ls
pulpo --node mac-mini logs review-backend --follow
```

Or open the controller dashboard and inspect the fleet view. Managed-node dashboards stay local-first and link you back to the controller for fleet control.

This is the practical value of Pulpo's control-plane model: remote execution,
same control semantics.

## 6. Add Docker Isolation If Needed

If the task needs stronger isolation:

```bash
pulpo --node gpu-box spawn risky-audit \
  --workdir ~/repos/backend \
  --runtime docker \
  --secret GH_WORK \
  -- claude --dangerously-skip-permissions -p "Audit this repository and propose fixes."
```

That keeps execution on your infrastructure while still isolating the session in
a container.

## Optional: Reusable Ink

If you run this kind of task often, define an ink on the target node:

```toml
[inks.private-review]
description = "Private review on internal infrastructure"
command = "claude -p 'Review this repository for correctness, security issues, and missing tests.'"
secrets = ["GH_WORK"]
runtime = "docker"
```

Then spawn it through the controller:

```bash
pulpo --node mac-mini spawn review-backend --workdir ~/repos/backend --node gpu-box --ink private-review
```

## Operational Notes

- Tailscale discovery is recommended when you want Pulpo across your own private machines.
- Discovery and enrollment are separate: discovery finds node addresses, enrollment authorizes fleet membership.
- Secrets are per-node, so manage them on the node that will execute the work.
- Remote `--workdir` paths must exist on the target node, not just on your local machine.
- If multiple nodes use different repo paths, prefer node-specific operational conventions rather than assuming one universal path layout.
- Fleet state on the controller is eventually consistent. Sessions keep running on managed nodes even if the controller restarts.
- The controller session index survives restart, but pending queued node commands do not.

## Related Docs

- [Discovery Guide](/guides/discovery)
- [Secrets](/guides/secrets)
- [Nightly Code Review](/guides/nightly-code-review)
- [Use Cases](/getting-started/use-cases)

## Summary

This workflow shows Pulpo's strongest wedge clearly:

- the runtime stays on infrastructure you control
- private-network access stays private
- sessions remain durable and observable
- one laptop can supervise work happening on another machine through a dedicated controller
