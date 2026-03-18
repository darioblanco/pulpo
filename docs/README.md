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
  - title: Multi-Node Fleet
    details: Spawn and manage sessions across your machine fleet — Mac, Linux, cloud — from a single CLI, web UI, or API call. Tailscale, mDNS, and seed discovery built in.
  - title: Docker Sandbox
    details: Run agents in isolated Docker containers with --sandbox. Safe for --dangerously-skip-permissions — the agent can't touch your host. Configurable sandbox image.
  - title: Agent-Aware Lifecycle
    details: Explicit states (active, idle, ready, killed, lost) with resume semantics, crash recovery, and watchdog supervision. Detects waiting-for-input prompts across 31 patterns for Claude Code, Codex, Gemini CLI, Aider, and more.
  - title: Command Agnostic
    details: Run any shell command — Claude Code, Codex, Gemini CLI, or your own scripts. Same lifecycle, same controls. Not an agent framework — the infrastructure layer beneath any agent.
  - title: Adopts Existing tmux Sessions
    details: Start tmux however you want — pulpo discovers external sessions, captures their full command line, and brings them under management automatically. No migration needed.
  - title: Git Worktrees
    details: --worktree isolates each agent in its own git worktree. Multiple agents work on the same repo without conflicts. Infrastructure-level feature that works with any agent, not just Claude Code.
  - title: Built-in Scheduler
    details: Cron-based schedules with multi-node targeting. Run nightly reviews on the beefy server, auto-select least loaded node. Dashboard shows schedule status, last run, and run history.
  - title: Watchdog Supervision
    details: Memory pressure detection, configurable idle thresholds (global and per-session), kill policies, and intervention audit trails. Agents run overnight without burning your API budget.
  - title: 6 Control Surfaces
    details: CLI, web UI (PWA with push notifications), REST API, SSE event stream, MCP server, and Discord bot. Manage agents from your phone while away from your desk.
---

## Quick Links

- [Install](/getting-started/install)
- [Quickstart](/getting-started/quickstart)
- [Examples](https://github.com/darioblanco/pulpo/tree/main/examples)
- [Configuration Guide](/guides/configuration)
- [Discovery Guide](/guides/discovery)
- [Recovery Guide](/guides/recovery)
- [CLI Reference](/reference/cli)
- [Config Reference](/reference/config)
- [API Reference](/reference/api)
- [Architecture Overview](/architecture/overview)
- [Session Lifecycle](/operations/session-lifecycle)
- [Release and Distribution](/operations/release-and-distribution)
- [LLM Index](/llms.txt)
