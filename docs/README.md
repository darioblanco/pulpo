---
home: true
title: Pulpo Documentation
heroText: Pulpo
heroImage: https://raw.githubusercontent.com/darioblanco/pulpo/main/web/public/logo.png
tagline: The self-hosted meter and breaker box for coding agents. See — and control — what every coding agent costs, across all your machines and accounts.
actions:
  - text: Why Pulpo
    link: /getting-started/why-pulpo
    type: primary
  - text: Quickstart
    link: /getting-started/quickstart
    type: secondary
  - text: Install
    link: /getting-started/install
    type: default
  - text: Use Cases
    link: /getting-started/use-cases
    type: default
features:
  - title: 1. Meter — exactly, everywhere
    details: "Reads exact token counts from each agent's own session files (Claude Code, Codex) and costs them from your rate table, attributed per session and rolled up per account and billing pool — across every machine and agent. Codex reports exact quota rather than a per-token cost. `[rates.<model>]` prices new models with no code change."
  - title: 2. Control — before the wall
    details: "Per-session and per-ink cost caps that alert at 80% and stop at 100%, plus a burn-velocity ($/hr) governor that catches the 2 a.m. runaway a flat budget misses. Alert-first; opt in to auto-stop."
  - title: 3. Monitor — to your own stack
    details: "Signed canonical events (lifecycle changes, usage/cost alerts) delivered to any number of webhooks with a durable outbox, plus a toggleable Prometheus `/metrics` endpoint. Pulpo is the event plane; your Grafana/Datadog/SIEM is the dashboard."
  - title: 4. Run — durable & unattended
    details: "Each agent runs in a `tmux` session with an explicit lifecycle that survives reboots, a watchdog for idle/memory/error/completion, and per-session git worktrees so parallel agents never collide. Command-agnostic."
---

## What Pulpo Is

Pulpo is the **self-hosted meter and breaker box for coding agents**.

It runs your agents as durable background sessions on machines you own, measures
exactly what each one costs across agents, accounts, and machines, enforces budgets,
and forwards alerts and events to whatever observability stack *you* run. Sovereign by
architecture: usage and account data are read from local files and never relayed to a
vendor.

Pulpo is infrastructure — not a model, IDE, prompt framework, or orchestration planner.
Modern agents already handle interactive worktrees, sandboxing, and guardrails; Pulpo is
the layer they lack.

## Why It Matters

Coding agents have become background workers — and a **quota-and-cost multiplier**. Run a
few in parallel and a weekly subscription allowance can vanish in an afternoon. The tools
that could warn you won't:

- a vendor's `/usage` is one account, one machine, one vendor, shown after the fact
- no vendor will aggregate spend across *your* accounts — that helps you arbitrage their limits
- only the thing actually running the session can stop a runaway before the wall
- your code and usage data are exactly what you'd least want in a third-party relay

Pulpo exists for that gap.

## Who Pulpo Is For

- power users running agents on Macs, Linux boxes, or home servers who want to know what they cost
- anyone running more than one agent, account, or machine who wants **one** gauge for all of them
- operators who need budgets and alerts that actually intervene, not a post-hoc invoice
- teams that require self-hosting, private-network access, and vendor independence

See [Why Pulpo](/getting-started/why-pulpo) for the full ICP and competitor view, and
[Use Cases](/getting-started/use-cases) for concrete profiles.

## Example Workflows

- [Nightly Code Review](/guides/nightly-code-review): schedule an overnight review with a budget cap and wake to the result + an exact cost
- [Parallel Agents On One Repo](/guides/parallel-agents-one-repo): split one repository across concurrent sessions, each in its own worktree
- [Private Infrastructure With Tailscale And Secrets](/guides/private-infra-with-tailscale): run agents across your own machines, reachable from your phone over the tailnet
- [Worktrees](/guides/worktrees): give a risky run an isolated git worktree
- [Agent Examples](/guides/agent-examples): how Pulpo wraps Claude Code, Codex, Gemini CLI, and more

## Where Pulpo Fits

| Category | Best at | Pulpo's difference |
| --- | --- | --- |
| Vendor `/usage` dashboards | One account's spend, after the fact | Pulpo is live and aggregates across accounts, machines, and agents |
| ccusage and local cost readers | Single-machine, read-only cost display | Pulpo runs the sessions, so it also projects, budgets, and *enforces* |
| Hosted coding agents | Managed cloud execution | Pulpo keeps the runtime, the cost data, and the control on infrastructure you own |

## Core Model

1. **`pulpod`** is the daemon that owns session state, metering, and enforcement.
2. A **session** is one managed command with durable metadata and an explicit lifecycle.
3. A **runtime backend** is where the session runs: `tmux`.
4. The **watchdog** reads exact usage, enforces budgets, drives lifecycle, and emits events.

Everything else is a surface over that core: the `pulpo` CLI, the web UI / PWA, the REST API
and SSE stream, the scheduler, and the event-forwarding backbone (webhooks + `/metrics`).

## Multi-machine

Pulpo is **single-node-first** — each node meters and governs its own sessions with no central
server required. For a fleet-wide view, point every node's event forwarding (`[[webhooks]]` +
`/metrics`) at a collector you already run. A controller/node control plane also exists but is
**frozen** (maintained, not extended) — see the [Roadmap](https://github.com/darioblanco/pulpo/blob/main/ROADMAP.md).

## Read In Order

1. [Why Pulpo](/getting-started/why-pulpo) for positioning, ICPs, and alternatives
2. [Use Cases](/getting-started/use-cases) for concrete user profiles and workflows
3. [Quickstart](/getting-started/quickstart) for the shortest hands-on path
4. [Core Concepts](/architecture/core-concepts) for the vocabulary
5. [Architecture Overview](/architecture/overview) for the mental model
6. [Session Lifecycle](/operations/session-lifecycle) for behavior guarantees
7. [Configuration Guide](/guides/configuration) for operational setup
8. [Config Reference](/reference/config) for every config key, including `[rates.<model>]`
9. [CLI Reference](/reference/cli) or [API Reference](/reference/api) for exact commands

## Quick Links

- [Why Pulpo](/getting-started/why-pulpo)
- [Use Cases](/getting-started/use-cases)
- [Alternatives And Comparisons](/getting-started/alternatives)
- [Install](/getting-started/install)
- [Quickstart](/getting-started/quickstart)
- [Core Concepts](/architecture/core-concepts)
- [Architecture Overview](/architecture/overview)
- [Session Lifecycle](/operations/session-lifecycle)
- [Configuration Guide](/guides/configuration)
- [Discovery Guide](/guides/discovery)
- [Nightly Code Review](/guides/nightly-code-review)
- [Parallel Agents On One Repo](/guides/parallel-agents-one-repo)
- [Private Infrastructure With Tailscale And Secrets](/guides/private-infra-with-tailscale)
- [Controller + Node Setup](/guides/controller-node-setup)
- [Worktrees](/guides/worktrees)
- [Agent Examples](/guides/agent-examples)
- [Recovery Guide](/guides/recovery)
- [CLI Reference](/reference/cli)
- [Config Reference](/reference/config)
- [API Reference](/reference/api)
- [Examples](https://github.com/darioblanco/pulpo/tree/main/examples)
- [Release and Distribution](/operations/release-and-distribution)
- [LLM Index](/llms.txt)
