# Private Infrastructure With Tailscale And Secrets

Hosted coding agents run on someone else's machine, which is a problem the moment an agent
needs a private repo, an internal API, or a VPN-only service — the runtime simply can't reach
them. Pulpo keeps the runtime, the credentials, and the reachability entirely on machines you
own: one daemon on a box on your tailnet, reachable from anywhere on that tailnet, with
secrets that never leave the machine that uses them.

If you haven't set up the daily-driver loop yet (spawn, detach, reattach from elsewhere), see
[Control Your Agents From Anywhere](/guides/remote-control) first — this guide adds secrets
on top of that same single-node setup.

## Example Setup

Assume:

- `mac-mini` is an always-on machine with access to a private repo and an internal API
- `laptop` is where you're currently working
- both are already on the same Tailnet

## 1. Put The Node On Your Tailnet

On `mac-mini`:

```toml
[node]
name = "mac-mini"
bind = "tailscale"
tag = "pulpo"
```

Start (or restart) `pulpod`. `bind = "tailscale"` binds locally and serves HTTPS over the
tailnet via `tailscale serve` — no public IP, no port-forwarding, no separate `pulpo` auth
token to manage. Tailscale's own ACLs are the reachability boundary.

## 2. Store The Secret On The Node That Will Use It

Secrets are stored per-node — in plaintext SQLite with `0600` file permissions, never
returned by the API, never transmitted anywhere except injected into the session's own
process environment on that machine. Store it where the work actually happens:

```bash
pulpo secret set GH_WORK ghp_work_xxxxxxxxxxxx --env GITHUB_TOKEN
pulpo secret list
```

See [Secrets](/guides/secrets) for the full security model.

## 3. Spawn The Session

```bash
pulpo spawn review-backend \
  --workdir ~/repos/backend \
  --secret GH_WORK \
  -- claude -p "Review this service for correctness, security issues, and missing tests."
```

The agent runs on `mac-mini`, with `GITHUB_TOKEN` injected from the stored secret, against a
repo and network that only `mac-mini` can reach.

## 4. Check Progress From Your Laptop

SSH in over the tailnet and attach directly, or open the web UI at the node's tailnet
address — both covered in
[Control Your Agents From Anywhere](/guides/remote-control):

```bash
ssh mac-mini
pulpo attach review-backend
```

## 5. Add Worktree Isolation If Needed

For a higher-permission run that shouldn't touch the repo's main working tree:

```bash
pulpo spawn risky-audit \
  --workdir ~/repos/backend \
  --worktree \
  --secret GH_WORK \
  -- claude --dangerously-skip-permissions -p "Audit this repository and propose fixes."
```

See [Worktrees](/guides/worktrees) for the full isolation model.

## Optional: Reusable Ink

If you run this kind of task often, define an ink so the command and its secrets travel
together:

```toml
[inks.private-review]
description = "Private review on internal infrastructure"
command = "claude -p 'Review this repository for correctness, security issues, and missing tests.'"
secrets = ["GH_WORK"]
```

```bash
pulpo spawn review-backend --workdir ~/repos/backend --ink private-review
```

## Operational Notes

- Tailscale is the recommended `bind` mode for reaching a node outside your LAN; use manual
  `[peers]` entries instead if a machine isn't on your tailnet.
- Secrets are per-node — set them on the node that will actually execute the work, not on
  whichever machine you happen to be typing from.
- `--workdir` (and any secret-backed path) must exist on the node that runs the session, not
  just on your laptop.

## Related Docs

- [Control Your Agents From Anywhere](/guides/remote-control)
- [Secrets](/guides/secrets)
- [Discovery Guide](/guides/discovery)
- [Worktrees](/guides/worktrees)
- [Use Cases](/getting-started/use-cases)

## Advanced: Multi-Node Control Plane (Experimental, Not Actively Developed)

Everything above is one node. Pulpo also has a controller/node control plane for fleet-wide
visibility and cross-node spawning from a single machine — but as of June 2026 it is
**frozen**: it works and is maintained, but it is not being extended (no controller-side
rollups, no quota-aware placement). For a fleet-wide view today, point every node's event
forwarding (`[[webhooks]]`, `/metrics`) at a collector you already run instead — see the
"Monitoring & event topology" section of the
[Architecture Overview](/architecture/overview).

If you still want the controller/node setup — for example to drive remote spawns from one
laptop across several enrolled machines — see
[Controller + Node Setup](/guides/controller-node-setup), which carries the same frozen-status
caveat up front.
