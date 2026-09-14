# Pulpo Documentation

The self-hosted meter and breaker box for coding agents. Run any agent as a durable
session on your own machine, metered exactly and capped before the wall.

[Why Pulpo](getting-started/why-pulpo.md) ·
[Quickstart](getting-started/quickstart.md) ·
[Install](getting-started/install.md) ·
[Use Cases](getting-started/use-cases.md)

- **Hook-driven state** — For Claude Code, Codex, and pi, Pulpo wires the harness's own
  lifecycle hooks at spawn time: real status (including a `needs input (<reason>)` label
  and the real exit code) instead of a scrollback guess, and `pulpo resume` replays the
  harness's own conversation after a reboot. Any other command falls back to the same
  scrollback-pattern watchdog Pulpo always had.
- **Exact metering, per repo** — Reads exact token counts from each agent's own session
  files (Claude Code, Codex, pi) and costs them from your rate table, attributed per
  session and rolled up per repo. Codex reports exact quota rather than a per-token cost.
  `[rates.<model>]` prices new models with no code change. No output-scraping fallback: an
  unsupported agent shows no usage.
- **A cap that stops at 100%** — Per-session and per-schedule cost caps that alert at 80%
  and stop at 100%, recorded as an intervention you can audit.
- **One webhook, your stack** — Canonical events (lifecycle changes, interventions,
  usage/cost alerts) delivered as a plain POST to any number of `[[webhooks]]`, in-memory
  queue with a fixed retry schedule. Pulpo doesn't ship dashboards; your
  Grafana/Datadog/SIEM is the dashboard.
- **Durable, scheduled, isolated** — Each agent runs in a `tmux` session with an explicit
  lifecycle that survives reboots, cron-based schedules for unattended recurring runs, and
  per-session git worktrees so parallel agents never collide. Command-agnostic.

## What Pulpo Is

Pulpo is the **self-hosted meter and breaker box for coding agents**.

It runs any coding agent — Claude Code, Codex, pi, or any other terminal command — as a
durable session on a machine you own, learns that session's real state from the agent's
own hooks instead of scraping terminal output, measures exactly what it costs from the
agent's own session files, enforces a spend cap, and forwards alerts and events to
whatever observability stack *you* run. Single node, sovereign: usage and account data
are read from local files and never relayed to a vendor.

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
- anyone running more than one agent or account on a machine who wants **one** exact gauge for all of them
- operators who need budgets and alerts that actually intervene, not a post-hoc invoice
- teams that require self-hosting, private-network access, and vendor independence

See [Why Pulpo](getting-started/why-pulpo.md) for the full ICP and competitor view, and
[Use Cases](getting-started/use-cases.md) for concrete profiles.

## Example Workflows

- [Control Your Agents From Anywhere](guides/remote-control.md): spawn, detach, and reattach from a laptop or phone over Tailscale
- [Nightly Code Review](guides/nightly-code-review.md): schedule an overnight review with a budget cap and wake to the result + an exact cost
- [Parallel Agents On One Repo](guides/parallel-agents-one-repo.md): split one repository across concurrent sessions, each in its own worktree
- [Private Infrastructure With Tailscale](guides/private-infra-with-tailscale.md): run agents across your own machines, reachable from your phone over the tailnet
- [Worktrees](guides/worktrees.md): give a risky run an isolated git worktree
- [Agent Examples](guides/agent-examples.md): how Pulpo wraps Claude Code, Codex, Gemini CLI, and more

## Where Pulpo Fits

| Category | Best at | Pulpo's difference |
| --- | --- | --- |
| Vendor `/usage` dashboards | One account's spend, after the fact | Pulpo is live and exact, across every account and agent on the machine |
| ccusage and local cost readers | Single-machine, read-only cost display | Pulpo runs the sessions, so it also budgets and *enforces* |
| Hosted coding agents | Managed cloud execution | Pulpo keeps the runtime, the cost data, and the control on infrastructure you own |

## Core Model

1. **`pulpod`** is the daemon that owns session state, metering, and enforcement.
2. A **session** is one managed command with durable metadata and an explicit lifecycle.
3. A **runtime backend** is where the session runs: `tmux`.
4. The **watchdog** reads exact usage, enforces budgets, drives lifecycle, and emits events.

Everything else is a surface over that core: the `pulpo` CLI, the web UI, the REST API
and SSE stream, the scheduler, and the event-forwarding backbone (`[[webhooks]]`).

## Multi-machine

Pulpo is **single-node-first** — each node meters and governs its own sessions with no central
server required. There is deliberately no control plane: reach each node directly with
`pulpo --url <host:port>`, a saved connection in the web UI, or SSH/tmux — see
[Control Your Agents From Anywhere](guides/remote-control.md). For a view across machines, point
every node's event forwarding (`[[webhooks]]`) at the same collector you already run.

## Read In Order

1. [Why Pulpo](getting-started/why-pulpo.md) for positioning, ICPs, and alternatives
2. [Use Cases](getting-started/use-cases.md) for concrete user profiles and workflows
3. [Quickstart](getting-started/quickstart.md) for the shortest hands-on path
4. [Core Concepts](architecture/core-concepts.md) for the vocabulary
5. [Architecture Overview](architecture/overview.md) for the mental model
6. [Session Lifecycle](operations/session-lifecycle.md) for behavior guarantees
7. [Harness Adapters](architecture/harness-adapters.md) for how agent lifecycle events replace scrollback scraping
8. [Configuration Guide](guides/configuration.md) for operational setup
9. [Config Reference](reference/config.md) for every config key, including `[rates.<model>]`
10. [CLI Reference](reference/cli.md) or [API Reference](reference/api.md) for exact commands

## Quick Links

- [Why Pulpo](getting-started/why-pulpo.md)
- [Use Cases](getting-started/use-cases.md)
- [Alternatives And Comparisons](getting-started/alternatives.md)
- [Install](getting-started/install.md)
- [Quickstart](getting-started/quickstart.md)
- [Core Concepts](architecture/core-concepts.md)
- [Architecture Overview](architecture/overview.md)
- [Session Lifecycle](operations/session-lifecycle.md)
- [Harness Adapters](architecture/harness-adapters.md)
- [Configuration Guide](guides/configuration.md)
- [Control Your Agents From Anywhere](guides/remote-control.md)
- [Nightly Code Review](guides/nightly-code-review.md)
- [Parallel Agents On One Repo](guides/parallel-agents-one-repo.md)
- [Private Infrastructure With Tailscale](guides/private-infra-with-tailscale.md)
- [Worktrees](guides/worktrees.md)
- [Agent Examples](guides/agent-examples.md)
- [Recovery Guide](guides/recovery.md)
- [CLI Reference](reference/cli.md)
- [Config Reference](reference/config.md)
- [API Reference](reference/api.md)
- [Examples](https://github.com/darioblanco/pulpo/tree/main/examples)
- [Release and Distribution](operations/release-and-distribution.md)
- [LLM Index](llms.txt)
