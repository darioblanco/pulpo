# Quickstart

## 1. Start daemon

```bash
pulpod
```

The web UI is available at [http://localhost:7433](http://localhost:7433) (installable as a PWA).

## 2. Spawn a session

```bash
pulpo spawn my-api --workdir ~/repos/my-api -- claude -p "Fix failing auth tests"
```

This auto-attaches to the tmux session. Detach with `Ctrl-b d` to return to your shell. Use `--detach` / `-d` to skip auto-attach.

No agent is required — `pulpo spawn my-shell` opens a managed shell session. Everything after `--` is the command to run.

## 3. Watch progress

```bash
pulpo list
pulpo logs my-api --follow
```

## 4. Parallel agents with worktrees

Multiple agents on the same repo, no conflicts:

```bash
pulpo spawn frontend --workdir ~/repo --worktree -- claude -p "redesign sidebar"
pulpo spawn backend  --workdir ~/repo --worktree -- codex "optimize queries"
```

Each agent gets an isolated git worktree at `<repo>/.pulpo/worktrees/<name>/`.

## 5. Schedule recurring runs

```bash
pulpo schedule add nightly-review "0 3 * * *" --workdir ~/repo -- claude -p "review code"
pulpo schedule list
```

## 6. Docker runtime

Run agents in isolated containers — safe for unrestricted permissions:

```bash
pulpo spawn risky-task --runtime docker -- claude --dangerously-skip-permissions -p "refactor everything"
```

The agent runs in a Docker container with only the workdir mounted. Configure the image in `~/.pulpo/config.toml`:

```toml
[docker]
image = "my-agents-image:latest"
```

## 7. Remote nodes

Spawn on another machine by name:

```bash
pulpo --node mac-mini spawn gpu-task -- python train.py
```

Or auto-select the least loaded node:

```bash
pulpo spawn review --auto -- claude -p "security audit"
```

## 8. Resume after a crash

Sessions survive daemon restarts. If a machine reboots:

```bash
pulpo list
# my-api   lost   ...

pulpo resume my-api
```

## 9. Open dashboard

```bash
open http://localhost:7433
curl -N http://localhost:7433/api/v1/events  # SSE stream
```

## Next steps

- [Examples](https://github.com/darioblanco/pulpo/tree/main/examples) — 10 runnable CLI workflows
- [Configuration Guide](/guides/configuration) — inks, watchdog, notifications, peers
- [Discovery Guide](/guides/discovery) — multi-node setup with Tailscale, mDNS, or seed
- [CLI Reference](/reference/cli) — all commands, flags, and scripting recipes
- [Session Lifecycle](/operations/session-lifecycle) — state machine, transitions, detection
