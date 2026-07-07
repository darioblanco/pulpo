# Quickstart

This guide is the shortest path from "I installed Pulpo" to "I know what my agents cost, and
I have a durable session running on infrastructure I control."

If you want the market context first, read [Why Pulpo](/getting-started/why-pulpo).
If you want examples with specific coding agents, see
[Agent Examples](/guides/agent-examples).

## 1. Install

```bash
brew install darioblanco/tap/pulpo
```

## 2. See What Your Agents Already Cost

Before routing anything through Pulpo, point it at the agent history already on disk:

```bash
pulpo usage --scan
```

This reads Claude Code's, Codex's, and pi's own session files (`~/.claude`, `~/.codex`,
`~/.pi`) and reports total tokens and spend by agent, model, and repo — no daemon, no
spawning, nothing routed through Pulpo. It's the fastest way to find out whether you have a
cost problem before you set anything else up.

## 3. Spawn A Session

The daemon starts automatically — no manual setup needed.

```bash
pulpo spawn my-api --workdir ~/repos/my-api -- claude -p "Fix failing auth tests"
```

This auto-attaches to the tmux session. Detach with `Ctrl-b d` to return to your shell. Use `--detach` / `-d` to skip auto-attach.

No agent is required — `pulpo spawn my-shell` opens a managed shell session. Everything after `--` is the command to run.

This is the key shift: instead of launching an agent into disposable shell
state, you are creating a durable session Pulpo can supervise, meter, and recover.

## What Just Happened?

You created one managed session:

- the daemon stored metadata for it
- the session started on the `tmux` runtime
- Pulpo began tracking its lifecycle, output, and (for Claude Code and Codex) exact usage

That is the core product. The rest of the docs mostly explain variations on that theme.

## 4. Watch Progress

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

## 5. Detach And Reattach From Anywhere

Detach with `Ctrl-b d` any time — the session keeps running whether or not anything is
attached. Reattach from the same machine, or SSH in from a laptop over Tailscale first:

```bash
pulpo attach my-api
```

This is the daily-driver loop: spawn, detach, walk away, reattach later from wherever you
are. See [Control Your Agents From Anywhere](/guides/remote-control) for the full pattern,
including checking status from a phone via the web UI.

## 6. Resume After A Crash Or Reboot

If a machine restarts or a backend disappears, Pulpo may show a session as `lost`:

```bash
pulpo list
pulpo resume my-api
```

`ready` sessions are also resumable. `stopped` sessions are not.

## 7. Parallel Agents With Worktrees

Multiple agents on the same repo, no conflicts:

```bash
pulpo spawn frontend --workdir ~/repo --worktree -- claude -p "redesign sidebar"
pulpo spawn backend  --workdir ~/repo --worktree -- codex "optimize queries"
```

Each agent gets an isolated git worktree at `~/.pulpo/worktrees/<name>/`. See the [Worktrees Guide](/guides/worktrees) for details.
For a full end-to-end workflow, see [Parallel Agents On One Repo](/guides/parallel-agents-one-repo).

## 8. Schedule Recurring Runs

```bash
pulpo schedule add nightly-review "0 3 * * *" --workdir ~/repo -- claude -p "review code"
pulpo schedule list
```

For a fuller version of this pattern — including a cost budget — see
[Nightly Code Review](/guides/nightly-code-review).

## 9. Isolated Worktree Runs

For higher-risk runs, give the agent an isolated git worktree on its own branch so it cannot disturb your main checkout:

```bash
pulpo spawn risky-task --workdir ~/repo --worktree -- claude --dangerously-skip-permissions -p "refactor everything"
```

The session gets `~/.pulpo/worktrees/<session-name>/` on a branch matching the session name; the worktree and branch are cleaned up when the session is stopped.

For the full workflow, see [Worktrees](/guides/worktrees).

## 10. Remote Nodes

Spawn on another machine by name:

```bash
pulpo --node mac-mini spawn gpu-task -- python train.py
```

This is where Pulpo starts to feel different from a local session manager: the
runtime can live on another machine, but the control model stays the same.

## 11. Open Dashboard

```bash
open http://localhost:7433
curl -N http://localhost:7433/api/v1/events  # SSE stream
```

## Next Steps

- [Control Your Agents From Anywhere](/guides/remote-control) — the daily-driver spawn/detach/reattach loop, in depth
- [Why Pulpo](/getting-started/why-pulpo) — ICPs, alternatives, and where Pulpo fits
- [Nightly Code Review](/guides/nightly-code-review) — a concrete recurring background-agent workflow
- [Parallel Agents On One Repo](/guides/parallel-agents-one-repo) — a concrete parallel-worktree workflow
- [Worktrees](/guides/worktrees) — isolate higher-risk runs from your main checkout
- [Agent Examples](/guides/agent-examples) — concrete examples with Claude Code, Codex, pi, Gemini CLI, Kimi Code, GLM-5 via OpenCode, and local models
- [Core Concepts](/architecture/core-concepts) — the smallest vocabulary for understanding Pulpo
- [Architecture Overview](/architecture/overview) — the session/runtime/watchdog mental model
- [Session Lifecycle](/operations/session-lifecycle) — exact transition behavior
- [Configuration Guide](/guides/configuration) — watchdog, notifications, peers
- [Examples](https://github.com/darioblanco/pulpo/tree/main/examples) — runnable CLI workflows
- [Discovery Guide](/guides/discovery) — multi-node setup with Tailscale or manual peers
- [CLI Reference](/reference/cli) — all commands, flags, and scripting recipes
