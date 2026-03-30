# Private Infrastructure With Tailscale And Secrets

This recipe shows how to run Pulpo across your own private machines and keep
agent execution close to private repos, VPN-only services, or internal systems.

It combines:

- Tailscale-based node discovery
- remote session control
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

- `mac-mini` is an always-on machine with access to private repos and internal services
- `laptop` is where you are currently working
- both machines are already on the same Tailnet

## 1. Configure The Remote Node

On `mac-mini`, set Pulpo to bind to Tailscale:

```toml
[node]
name = "mac-mini"
bind = "tailscale"
tag = "pulpo"
discovery_interval_secs = 30
```

Then start or restart `pulpod`.

This makes the node:

- bind to the Tailscale interface
- discover peer Pulpo nodes via the local Tailscale API
- trust Tailnet-level access instead of separate public auth

## 2. Configure Your Local Node

On `laptop`, use the same model:

```toml
[node]
name = "laptop"
bind = "tailscale"
tag = "pulpo"
discovery_interval_secs = 30
```

Once both nodes are running, Pulpo should discover them as peers.

Check from your laptop:

```bash
pulpo nodes
```

## 3. Store Secrets On The Right Node

Secrets are stored per-node, which is usually what you want for private
infrastructure.

For example, if `mac-mini` has access to the private repo and should run the
session, store the secret there:

```bash
pulpo --node mac-mini secret set GH_WORK ghp_work_xxxxxxxxxxxx --env GITHUB_TOKEN
pulpo --node mac-mini secret list
```

This keeps the secret associated with the machine that will actually execute the
session.

## 4. Run A Remote Session

Now launch a task on the remote node:

```bash
pulpo --node mac-mini spawn review-backend \
  --workdir ~/repos/backend \
  --secret GH_WORK \
  -- claude -p "Review this service for correctness, security issues, and missing tests."
```

From your laptop, you are still in control, but the runtime lives on `mac-mini`.

That means:

- the agent executes near the private repo and services
- the session lifecycle is still visible from your laptop
- you can inspect status, logs, and recovery through the same Pulpo interface

## 5. Check Progress Remotely

From your laptop:

```bash
pulpo --node mac-mini ls
pulpo --node mac-mini logs review-backend --follow
```

Or open the dashboard and inspect the fleet view.

This is the practical value of Pulpo's control-plane model: remote execution,
same control semantics.

## 6. Add Docker Isolation If Needed

If the task needs stronger isolation:

```bash
pulpo --node mac-mini spawn risky-audit \
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

Then spawn it remotely:

```bash
pulpo --node mac-mini spawn review-backend --workdir ~/repos/backend --ink private-review
```

## Operational Notes

- Tailscale discovery is recommended when you want Pulpo across your own private machines.
- Secrets are per-node, so manage them on the node that will execute the work.
- Remote `--workdir` paths must exist on the target node, not just on your local machine.
- If multiple nodes use different repo paths, prefer node-specific operational conventions rather than assuming one universal path layout.

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
- one laptop can supervise work happening on another machine
