# Private Infrastructure With Tailscale

Hosted coding agents run on someone else's machine, which is a problem the moment an agent
needs a private repo, an internal API, or a VPN-only service — the runtime simply can't reach
them. Pulpo keeps the runtime and the reachability entirely on machines you own: one daemon
on a box on your tailnet, reachable from anywhere on that tailnet.

If you haven't set up the daily-driver loop yet (spawn, detach, reattach from elsewhere), see
[Control Your Agents From Anywhere](/guides/remote-control) first — this guide adds a
credentialed run on top of that same single-node setup.

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

## 2. Make The Credential Available On The Node That Will Use It

Pulpo doesn't have a secrets store: every supported agent (Claude Code, Codex, pi, Gemini)
already reads its own credentials from its own config, and a one-off credential a session
needs is just an environment variable — inject it the same way you would for any other
process on that machine. Two options, depending on how long-lived the credential is:

- **Session-scoped**: prefix the command you pass to `spawn` so the variable is only set for
  that one invocation. The value never appears in `pulpo`'s own argv (only inside the shell
  command Pulpo hands to tmux on `mac-mini`), so it doesn't leak into `ps` output on your
  laptop:

  ```bash
  pulpo spawn review-backend \
    --workdir ~/repos/backend \
    -- env GITHUB_TOKEN=ghp_work_xxxxxxxxxxxx claude -p "Review this service for correctness, security issues, and missing tests."
  ```

- **Machine-wide**: export it in the environment `pulpod` itself runs under on `mac-mini`
  (your shell profile for `make dev`, or the `Environment=` directive in the systemd/launchd
  service file) so every session spawned on that node inherits it without repeating the
  `env VAR=value` prefix.

Either way, the credential never leaves `mac-mini` — it's set directly in the session's own
process environment on the machine that runs the work, exactly like running the agent by hand
over SSH.

## 3. Spawn The Session

```bash
pulpo spawn review-backend \
  --workdir ~/repos/backend \
  -- env GITHUB_TOKEN=ghp_work_xxxxxxxxxxxx claude -p "Review this service for correctness, security issues, and missing tests."
```

The agent runs on `mac-mini`, with `GITHUB_TOKEN` set for that command, against a repo and
network that only `mac-mini` can reach.

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
  -- env GITHUB_TOKEN=ghp_work_xxxxxxxxxxxx claude --dangerously-skip-permissions -p "Audit this repository and propose fixes."
```

See [Worktrees](/guides/worktrees) for the full isolation model.

## Operational Notes

- Tailscale is the recommended `bind` mode for reaching a node outside your LAN.
- Set credentials on the node that will actually execute the work, not on whichever machine
  you happen to be typing from.
- `--workdir` (and any credential-backed path) must exist on the node that runs the session,
  not just on your laptop.

## Related Docs

- [Control Your Agents From Anywhere](/guides/remote-control)
- [Worktrees](/guides/worktrees)
- [Use Cases](/getting-started/use-cases)

## Multiple Machines

Everything above is one node. If you have more than one, there is deliberately no control
plane joining them — cross-node orchestration is a dead product lane (see
[ROADMAP.md](https://github.com/darioblanco/pulpo/blob/main/ROADMAP.md) "Phase C"). Instead:

- Run a `pulpod` per box, each on your tailnet (`bind = "tailscale"`).
- Reach each one directly: `pulpo --url <host:port>` from any machine on the tailnet, a saved
  connection in the web UI, or plain SSH + `pulpo attach`.
- Aggregate visibility across machines by pointing every node's `[[webhooks]]` at the same
  collector — see the "Monitoring & event topology" section of the
  [Architecture Overview](/architecture/overview).
