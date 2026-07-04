# Control Your Agents From Anywhere

Coding agents run for minutes to hours, but a laptop lid closes, wifi drops, and SSH
sessions die — and a plain terminal agent dies with them. Pulpo turns an agent invocation
into a durable session on a machine you leave on, so the agent keeps working and you can
check on it, or take over, from anywhere on your tailnet.

This guide is the daily-driver loop: run one always-on daemon, spawn, detach, and reattach
later from a different machine. It stays on a single node — see
[Private Infrastructure With Tailscale](/guides/private-infra-with-tailscale) if you also
need secrets management, or the [Discovery Guide](/guides/discovery) if you outgrow one box.

## 1. Run `pulpod` On One Always-On Machine

A Mac mini, a home server, a spare Linux box, or a cheap always-on VM you own. Install it
once (see [Install](/getting-started/install)) and the daemon starts automatically via
`brew services` or `systemd` — nothing to babysit.

To reach it from outside your LAN, put it on your tailnet:

```toml
[node]
name = "mac-mini"
bind = "tailscale"
```

`bind = "tailscale"` binds locally and serves HTTPS over the tailnet via `tailscale serve` —
no port-forwarding, no public IP, no extra `pulpo` auth token to manage. Tailscale's own ACLs
are the security boundary.

## 2. Spawn And Detach

```bash
pulpo spawn fix --workdir ~/repos/api -- claude -p "Fix the failing auth tests"
```

`pulpo spawn` auto-attaches so you can watch it start. Detach with `Ctrl-b d` whenever you
want your terminal back — the session keeps running either way. To skip attaching entirely
(scripts, or when you're about to walk away anyway), add `--detach` / `-d`:

```bash
pulpo spawn fix --workdir ~/repos/api -d -- claude -p "Fix the failing auth tests"
```

The agent is now running in `tmux` on the daemon's machine, independent of your terminal,
your SSH connection, or your laptop's power state.

## 3. Reattach From A Laptop Over SSH

Tailscale's MagicDNS makes the machine's hostname resolve from anywhere on your tailnet, no
VPN client juggling required:

```bash
ssh mac-mini
pulpo attach fix
```

`pulpo attach` runs `tmux attach-session` on the machine it executes on — so this only works
from a shell that is actually *on* the daemon's host. That's what the `ssh` step is for;
`pulpo --node mac-mini attach fix` from your laptop will not work, because the tmux session
it needs to attach to doesn't exist on your laptop. Detach again with `Ctrl-b d` as many
times as you like.

## 4. Glance From A Phone Via The Web UI

Open the same tailnet address in a mobile browser:

```
https://mac-mini.<your-tailnet-name>.ts.net
```

Install it as a PWA (share sheet → "Add to Home Screen") for an app icon and offline shell.
Each session card shows live status and renders the agent's output by default — a read-mostly
view built for checking in, not for typing on a phone keyboard. A **Terminal** toggle on the
card opens the same session as a fully interactive terminal over WebSocket if you do need to
type something (approve a prompt, answer a question) without reaching for SSH.

## Status At A Glance

```bash
pulpo list
```

```
NAME   STATUS   AGE   COMMAND
fix    active   4m    claude -p "Fix the failing auth tests"
```

The statuses that matter day to day:

- `active` — the agent is working, output is still changing
- `idle` — the agent is waiting on you, or has gone quiet past the idle threshold
- `ready` — the command exited; the session is done but still resumable
- `lost` — the daemon's machine rebooted or tmux disappeared; resumable
- `stopped` — terminated on purpose; not resumable

## Sessions Survive Disconnects And Reboots

Nothing above depends on your laptop, your SSH connection, or your phone staying connected —
the session lives in `tmux` on the daemon's machine. Close the lid, lose wifi, or just forget
about it; the next `pulpo attach fix` reconnects to whatever state it's in.

A machine reboot or crash is the one case that needs an explicit step:

```bash
pulpo list
# fix   lost   ...

pulpo resume fix
```

`pulpo resume` re-creates the `tmux` session, re-runs the command, and auto-attaches. It
works on `lost` sessions (the backend disappeared) and `ready` sessions (the agent already
finished, but you want the shell back). `pulpod` also auto-resumes sessions that were
`active` when it shut down, the next time it starts — you often won't need to run `resume` by
hand at all. See [Session Lifecycle](/operations/session-lifecycle) and
[Recovery](/guides/recovery) for the exact state machine and detection rules.

## Related Docs

- [Quickstart](/getting-started/quickstart) — the shortest path from install to a running
  session
- [Session Lifecycle](/operations/session-lifecycle) — exact state transitions
- [Recovery](/guides/recovery) — resume semantics after crashes and reboots
- [Discovery Guide](/guides/discovery) — Tailscale bind mode and peer discovery details
- [Private Infrastructure With Tailscale](/guides/private-infra-with-tailscale) — adding
  secrets, and what changes if you ever need more than one machine
