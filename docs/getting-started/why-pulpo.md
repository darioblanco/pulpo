# Why Pulpo

Pulpo is a self-hosted **meter and breaker box for coding agents**: it runs your agent
sessions as durable background jobs on machines you own, measures what each one actually
costs, and can stop one before it runs past a budget you set.

If you already know how to run an agent in a terminal, this page answers the
next question: why would you add Pulpo at all?

## The Daily Loop

Before anything else, Pulpo answers "what is this costing me?" with zero setup:

```bash
pulpo usage --scan
```

That reads the agent history already on your disk (Claude Code, Codex, pi) and reports spend
by agent, model, and repo — no daemon, no session routed through Pulpo. From there, the loop
that matters day to day is spawn, detach, reattach from wherever you are:

```bash
pulpo spawn fix --workdir ~/repos/api -- claude -p "Fix the failing auth tests"
# Ctrl-b d to detach — the session keeps running
pulpo attach fix   # from the same box, or over SSH/Tailscale from a laptop
```

Budgets and a burn-rate governor sit on top of that same durable session, alerting at 80% of
a cap and — if you opt in — stopping it before it runs past 100%. See
[Quickstart](/getting-started/quickstart) and
[Control Your Agents From Anywhere](/guides/remote-control) for the hands-on version of this
loop.

## The Problem It Solves

Running one agent manually is easy, and so is checking what one prompt cost afterward.

Running several agents, across accounts and machines, unattended, while you are not
watching — and knowing what all of it costs before the bill or the weekly quota resets — is
not. That gap usually looks like:

- a vendor's `/usage` page: one account, one machine, shown after the fact
- no vendor aggregating spend across *your* accounts — that would help you arbitrage their limits
- SSH plus tmux as ad hoc infrastructure, with sessions disappearing after reboots or crashes
- no clear status for "working", "waiting", "finished", or "lost"
- multiple agents colliding in the same repo
- nothing watching the meter closely enough to stop a runaway before it's expensive

Pulpo turns agent commands into durable, supervised sessions with explicit lifecycle state,
exact-where-possible cost metering, and budgets that actually intervene — not a post-hoc
invoice.

## Who Pulpo Is For

### 1. The Power User With Servers Or Always-On Machines

You already use Claude Code, Codex, pi, Gemini CLI, Aider, or shell automation, possibly
across more than one account. You want that work to keep running on a Mac mini, Linux box,
or home server while you step away — and you want to know what it costs.

Pulpo gives you:

- one gauge for spend across agents, models, and repos (`pulpo usage --scan`, zero setup)
- durable sessions instead of fragile shell state, reachable again from a laptop or phone
- recovery after backend loss or reboot
- worktrees for parallel agents on the same repo, without collisions

### 2. The Private-Infrastructure Team

You need agents near private repos, internal APIs, VPN-only systems, or self-hosted tools.
Hosted coding agents are inconvenient or impossible because the runtime needs to stay on
your network — and so does your usage data.

Pulpo gives you:

- self-hosted execution on infrastructure you control, reachable over your own tailnet
- command-agnostic support across agent vendors
- secrets and cost data that never leave your machines
- policy depth through watchdogs, secrets, worktrees, and budgets

### 3. The Operator Running Repeated Agent Work

You want more than one-off prompts. You want nightly reviews, scheduled scans, parallel
refactors, and unattended long-running tasks — with a budget cap so an overnight run can't
surprise you in the morning.

Pulpo gives you:

- schedules and budgets tied to real session objects
- signed events forwarded to your own webhooks/Grafana/Datadog, plus a `/metrics` endpoint
- a burn-velocity governor that catches a runaway before a flat budget would trip
- repeatable operational workflows without writing a platform from scratch

## When Pulpo Is The Right Tool

Pulpo is a strong fit when:

- you want to know what your coding agents cost, across accounts and machines, before the invoice
- the runtime itself needs to be self-hosted
- you use more than one coding agent tool
- you care about recovery semantics and durable state
- you want to supervise work, and enforce a budget, from outside the active terminal

Pulpo is a weaker fit when:

- you only want the easiest hosted coding-agent experience
- you mostly work inside one IDE and do not need remote execution or cost visibility
- one local terminal session is enough
- you need a multi-agent planner more than a metering and runtime layer

## Competitor Matrix

The market is crowded, but the categories are different. The important question
is not "which tool has more features?" It is "which layer does each tool own?"

| Category | Examples | What they are best at | Where Pulpo differs |
| --- | --- | --- | --- |
| Vendor `/usage` dashboards | Anthropic, OpenAI account usage pages | One account's spend, after the fact | Pulpo aggregates across accounts, machines, and agents, live |
| Local cost readers | ccusage and similar | Multi-agent, single-machine, read-only cost display | Pulpo also runs the session: cross-machine rollups, budgets, and enforcement |
| Hosted coding agents | Codex app, GitHub Copilot coding agent, Cursor background agents, Claude cloud sessions, OpenHands Cloud | Managed cloud execution, PR-native workflows, provider integration | Pulpo keeps the runtime, the cost data, and the control on your own machines |
| Local session managers | Agent Deck, tmux-based command centers | Fast local multi-session UX, terminal-first workflows | Pulpo adds cost metering, budgets, durable recovery, and remote access |
| Agent orchestration frameworks | Multi-agent planners and task routers | Decomposing work, assigning tasks, coordinating agents | Pulpo is the metering/runtime layer underneath those workflows, not the planner |
| Generic infrastructure | Raw tmux, cron, SSH, Docker scripts | Maximum flexibility, no product constraints | Pulpo gives you lifecycle semantics, recovery, cost metering, and budgets on top |

For a more detailed and explicitly source-based version, see
[Alternatives And Comparisons](/getting-started/alternatives).

## Hosted Agents Vs Pulpo

Hosted coding agents are getting better quickly. They usually win on:

- time to first success
- integrated cloud sandbox UX
- deep coupling to one vendor workflow
- PR and issue-native delegation

Pulpo wins when you need:

- your machines, not theirs
- your network access, not a hosted sandbox
- your choice of agent, not one provider
- daemon-owned recovery semantics after failure
- to see and cap what all of that actually costs, across vendors, in one place

## Local Managers Vs Pulpo

Local agent managers solve a real problem: too many sessions, not enough
terminal discipline.

Pulpo overlaps with them, but aims one level deeper:

- a session is a durable, metered object, not just a visible process
- failure and recovery behavior are explicit
- cost, budgets, and burn-rate alerts are built in, not bolted on
- control surfaces include web, API, notifications, and scheduling

If your main problem is "I need a nicer terminal dashboard," use the best local
manager.

If your main problem is "I need to know what agent work costs and stop it before it runs
away," use Pulpo.

## What Pulpo Is Not

Pulpo is not:

- a better coding model
- an IDE replacement
- a prompt framework
- an orchestration planner for multi-agent workflows
- a hosted code-review bot

It is the meter, breaker, and durable runtime layer for agent execution on infrastructure
you control.

## The Short Version

Use Pulpo when you need to know what coding agents are costing you before the bill does, and
when they stop feeling like interactive tools and start feeling like jobs that need to run
somewhere, be observed, and survive failure.

## Read Next

1. [Quickstart](/getting-started/quickstart)
2. [Control Your Agents From Anywhere](/guides/remote-control)
3. [Install](/getting-started/install)
4. [Alternatives And Comparisons](/getting-started/alternatives)
5. [Core Concepts](/architecture/core-concepts)
