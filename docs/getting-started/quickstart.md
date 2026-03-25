# Quickstart

## 1. Install

```bash
brew install darioblanco/tap/pulpo
```

## 2. Spawn a session

The daemon starts automatically — no manual setup needed.

```bash
pulpo spawn my-api --workdir ~/repos/my-api -- claude -p "Fix failing auth tests"
```

This auto-attaches to the tmux session. Detach with `Ctrl-b d` to return to your shell. Use `--detach` / `-d` to skip auto-attach.

No agent is required — `pulpo spawn my-shell` opens a managed shell session. Everything after `--` is the command to run.

## What just happened?

You created one managed session:

- the daemon stored metadata for it
- the session started on the default runtime (`tmux`)
- Pulpo began tracking its lifecycle and output

That is the core product. The rest of the docs mostly explain variations on that theme.

## 3. Watch progress

```bash
pulpo list
pulpo logs my-api --follow
```

The important statuses to know early are:

- `active`: the command is still working
- `idle`: the command is waiting for input or has gone quiet
- `ready`: the command finished, but the session is still resumable
- `lost`: the backend disappeared and the session may need resume
- `stopped`: the session was terminated and is not resumable

## 4. Resume after a crash or reboot

If a machine restarts or a backend disappears, Pulpo may show a session as `lost`:

```bash
pulpo list
pulpo resume my-api
```

`ready` sessions are also resumable. `stopped` sessions are not.

## 5. Parallel agents with worktrees

Multiple agents on the same repo, no conflicts:

```bash
pulpo spawn frontend --workdir ~/repo --worktree -- claude -p "redesign sidebar"
pulpo spawn backend  --workdir ~/repo --worktree -- codex "optimize queries"
```

Each agent gets an isolated git worktree at `~/.pulpo/worktrees/<name>/`. See the [Worktrees Guide](/guides/worktrees) for details.

## 6. Schedule recurring runs

```bash
pulpo schedule add nightly-review "0 3 * * *" --workdir ~/repo -- claude -p "review code"
pulpo schedule list
```

## 7. Docker runtime

Run agents in isolated containers — safe for unrestricted permissions:

```bash
pulpo spawn risky-task --runtime docker -- claude --dangerously-skip-permissions -p "refactor everything"
```

The agent runs in a Docker container with the session workdir mounted, plus any configured Docker volumes. Configure the image in `~/.pulpo/config.toml`:

```toml
[docker]
image = "my-agents-image:latest"
```

## 8. Remote nodes

Spawn on another machine by name:

```bash
pulpo --node mac-mini spawn gpu-task -- python train.py
```

Or auto-select the least loaded node:

```bash
pulpo spawn review --auto -- claude -p "security audit"
```

## 9. Open dashboard

```bash
open http://localhost:7433
curl -N http://localhost:7433/api/v1/events  # SSE stream
```

## Next steps

- [Core Concepts](/architecture/core-concepts) — the smallest vocabulary for understanding Pulpo
- [Architecture Overview](/architecture/overview) — the session/runtime/watchdog mental model
- [Session Lifecycle](/operations/session-lifecycle) — exact transition behavior
- [Configuration Guide](/guides/configuration) — inks, watchdog, notifications, peers
- [Examples](https://github.com/darioblanco/pulpo/tree/main/examples) — runnable CLI workflows
- [Discovery Guide](/guides/discovery) — multi-node setup with Tailscale, mDNS, or seed
- [CLI Reference](/reference/cli) — all commands, flags, and scripting recipes
