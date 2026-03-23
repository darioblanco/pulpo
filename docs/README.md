---
home: true
title: Pulpo Documentation
heroText: Pulpo
heroImage: https://raw.githubusercontent.com/darioblanco/pulpo/main/web/public/logo.png
tagline: Agent session runtime. Run coding agents in tmux or Docker across your machines — with lifecycle management, crash recovery, and watchdog supervision.
actions:
  - text: Install
    link: /getting-started/install
    type: primary
  - text: Quickstart
    link: /getting-started/quickstart
    type: secondary
features:
  - title: 1. Run Sessions
    details: "`pulpod` runs commands as managed sessions in tmux or Docker. Pulpo is command-agnostic: Claude Code, Codex, Gemini CLI, shell scripts, or any other terminal command."
  - title: 2. Track State
    details: "Sessions move through explicit states: `creating`, `active`, `idle`, `ready`, `stopped`, `lost`. This lifecycle is the core product contract."
  - title: 3. Recover and Supervise
    details: "The watchdog detects waiting-for-input, idle sessions, memory pressure, and lost backends. Sessions can be resumed from `lost` and `ready` states."
  - title: 4. Operate Across Machines
    details: "Nodes can be managed individually or as a fleet. Discovery, scheduling, worktrees, secrets, notifications, and the web UI sit on top of the same session model."
---

## Core Model

Pulpo is easiest to understand as four pieces:

1. **`pulpod`** is the daemon that owns session state and backends.
2. A **session** is one managed command with durable metadata and an explicit lifecycle.
3. A **runtime backend** is where the session runs: `tmux` or `docker`.
4. The **watchdog** observes output and enforces recovery/intervention rules.

Everything else is a control surface or an operational convenience around that core:

- `pulpo` CLI
- web UI / PWA
- REST API and SSE
- scheduler
- peer discovery
- Discord bot
- MCP server

## What Is Core vs Optional

**Core, expected behavior**

- spawn a session
- inspect status and output
- send input
- stop it
- resume from `lost` or `ready`
- watchdog-driven lifecycle transitions

**Optional operational layers**

- multi-node fleet view
- Docker runtime
- worktrees
- schedules
- secrets
- notifications

**Experimental / convenience surfaces**

- Discord bot
- MCP server
- ocean UI and other presentation layers

The project is still experimental overall, but the session/runtime/lifecycle model is the part to learn first.

## Read In Order

1. [Quickstart](/getting-started/quickstart) for the shortest hands-on path
2. [Core Concepts](/architecture/core-concepts) for the vocabulary
3. [Architecture Overview](/architecture/overview) for the mental model
4. [Session Lifecycle](/operations/session-lifecycle) for behavior guarantees
5. [Configuration Guide](/guides/configuration) for operational setup
6. [CLI Reference](/reference/cli) or [API Reference](/reference/api) for exact commands

## Quick Links

- [Install](/getting-started/install)
- [Quickstart](/getting-started/quickstart)
- [Core Concepts](/architecture/core-concepts)
- [Architecture Overview](/architecture/overview)
- [Session Lifecycle](/operations/session-lifecycle)
- [Configuration Guide](/guides/configuration)
- [Discovery Guide](/guides/discovery)
- [Recovery Guide](/guides/recovery)
- [CLI Reference](/reference/cli)
- [Config Reference](/reference/config)
- [API Reference](/reference/api)
- [Examples](https://github.com/darioblanco/pulpo/tree/main/examples)
- [Release and Distribution](/operations/release-and-distribution)
- [LLM Index](/llms.txt)
