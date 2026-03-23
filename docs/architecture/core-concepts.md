# Core Concepts

This page defines the smallest useful vocabulary for Pulpo. If these concepts are clear, the rest of the docs become straightforward.

## 1. Daemon

`pulpod` is the daemon.

It is responsible for:

- creating sessions
- persisting session state
- talking to runtime backends
- running the watchdog
- serving the API and embedded web UI

Everything else in the project depends on the daemon.

## 2. Session

A session is one managed command.

It has:

- a name
- a working directory
- a command
- a runtime
- captured output
- timestamps
- a lifecycle state
- optional metadata such as worktree, ink, and intervention history

Examples:

```bash
pulpo spawn review -- claude -p "review this code"
pulpo spawn lint -- npm run lint
pulpo spawn shell
```

Pulpo is command-agnostic. A session is not tied to one agent vendor.

## 3. Runtime

A runtime is where the session executes.

Current runtimes:

- `tmux`: native long-lived terminal session
- `docker`: containerized execution with the session workdir mounted into the container

The important design point is that the lifecycle model is shared across runtimes.

## 4. Lifecycle

Sessions move through explicit states:

```text
creating -> active <-> idle -> ready
                       \-> stopped
active/idle ----------> lost
```

The most important meanings:

- `active`: the command is running and producing output
- `idle`: the session appears to be waiting for input or has gone quiet long enough to be treated as waiting
- `ready`: the command exited and the session is resumable
- `lost`: the backend disappeared unexpectedly
- `stopped`: the session was terminated and is not resumable

See [Session Lifecycle](/operations/session-lifecycle) for exact transition rules.

## 5. Watchdog

The watchdog is the supervision loop.

It:

- checks output for waiting-for-input patterns
- tracks idle thresholds
- detects exit markers
- enforces memory-pressure interventions
- applies ready TTL cleanup
- can adopt external tmux sessions

Without the watchdog, Pulpo would be a launcher. With it, Pulpo becomes runtime infrastructure.

## 6. Control Surfaces

Control surfaces are ways to operate the same underlying session model.

- CLI
- web UI
- REST API
- SSE stream
- scheduler
- Discord bot
- MCP server

These are not separate products. They are different interfaces to the same daemon-owned state.

## 7. Operational Layers

Some features are important but still secondary to the core model:

- multi-node fleet discovery
- worktrees
- schedules
- secrets
- notifications

They matter operationally, but they are easier to reason about once session/runtime/lifecycle concepts are already clear.

## Read Next

1. [Architecture Overview](/architecture/overview)
2. [Quickstart](/getting-started/quickstart)
3. [Session Lifecycle](/operations/session-lifecycle)
