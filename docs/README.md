---
home: true
title: Pulpo Documentation
heroText: Pulpo
heroImage: https://raw.githubusercontent.com/darioblanco/pulpo/main/web/public/logo.png
tagline: The self-hosted control plane for background coding agents. Run agents on your own machines with durable sessions, recovery, and remote supervision.
actions:
  - text: Install
    link: /getting-started/install
    type: primary
  - text: Quickstart
    link: /getting-started/quickstart
    type: secondary
features:
  - title: 1. Run On Your Infrastructure
    details: "`pulpod` runs commands as managed sessions on machines you control. Pulpo is command-agnostic: Claude Code, Codex, Gemini CLI, shell scripts, or any other terminal command."
  - title: 2. Keep State Durable
    details: "Sessions move through explicit states: `creating`, `active`, `idle`, `ready`, `stopped`, `lost`. That lifecycle is the core product contract."
  - title: 3. Supervise Background Work
    details: "The watchdog detects waiting-for-input, idle sessions, memory pressure, and lost backends. Sessions can be resumed from `lost` and `ready` states."
  - title: 4. Control A Fleet
    details: "Nodes can be managed individually or as a fleet. Discovery, scheduling, worktrees, secrets, notifications, and the web UI all build on the same session model."
---

## What Pulpo Is

Pulpo is a self-hosted control plane for background coding agents.

It exists for the gap between "I can run an agent in my terminal" and "I can
reliably run agents across my own machines while I am not watching."

Pulpo is infrastructure, not a model, IDE, prompt framework, or orchestration
planner.

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

## Who It Is For

Pulpo is for users who want coding agents to:

- run on servers or always-on machines instead of laptops
- access private repos, VPN-only systems, or internal environments
- remain manageable from a phone or another machine
- survive crashes, reboots, and lost backends with explicit recovery semantics

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
