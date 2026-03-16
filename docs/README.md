---
home: true
title: Pulpo Documentation
heroText: Pulpo
heroImage: https://raw.githubusercontent.com/darioblanco/pulpo/main/web/public/logo.png
tagline: Self-hosted control plane for coding agents. Durable sessions, multi-node operations.
actions:
  - text: Install
    link: /getting-started/install
    type: primary
  - text: Quickstart
    link: /getting-started/quickstart
    type: secondary
features:
  - title: Session Lifecycle
    details: Explicit states (active, idle, finished, killed, lost) with resume semantics and crash recovery.
  - title: Multi-Node Operations
    details: Manage agents across machines from a single API/CLI/web surface. Tailscale, mDNS, and seed discovery.
  - title: Command Agnostic
    details: Run any shell command — Claude Code, Codex, Gemini CLI, or your own scripts. Same lifecycle, same controls.
  - title: Watchdog & Monitoring
    details: Memory pressure detection, idle handling, configurable kill policies, and intervention audit trails.
  - title: API First
    details: REST, SSE, MCP, CLI, web UI, and Discord bot — integrate with anything.
---

## Quick Links

- [Install](/getting-started/install)
- [Quickstart](/getting-started/quickstart)
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
