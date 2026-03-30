# Why Pulpo

Pulpo is a self-hosted control plane for background coding agents.

If you already know how to run an agent in a terminal, this page answers the
next question: why would you add Pulpo at all?

## The Problem It Solves

Running one agent manually is easy.

Running agents reliably across your own machines, while you are not watching, is
not.

That gap usually looks like:

- SSH plus tmux as ad hoc infrastructure
- sessions disappearing after reboots or crashes
- no clear status for "working", "waiting", "finished", or "lost"
- multiple agents colliding in the same repo
- no clean way to check progress from a phone or another machine

Pulpo turns those commands into durable, supervised sessions with explicit
lifecycle state and remote control.

## Who Pulpo Is For

### 1. The Power User With Servers Or Always-On Machines

You already use Claude Code, Codex, Gemini CLI, Aider, or shell automation. You
want that work to keep running on a Mac mini, Linux box, or home server while
you step away.

Pulpo gives you:

- durable sessions instead of fragile shell state
- remote supervision from CLI, API, or phone-friendly web UI
- recovery after backend loss or reboot
- worktrees for parallel agents on the same repo

### 2. The Private-Infrastructure Team

You need agents near private repos, internal APIs, VPN-only systems, or
self-hosted tools. Hosted coding agents are inconvenient or impossible because
the runtime needs to stay on your network.

Pulpo gives you:

- self-hosted execution on infrastructure you control
- command-agnostic support across agent vendors
- audit-friendly lifecycle and intervention history
- policy depth through watchdogs, secrets, Docker, and scheduling

### 3. The Operator Running Repeated Agent Work

You want more than one-off prompts. You want nightly reviews, scheduled scans,
parallel refactors, and unattended long-running tasks.

Pulpo gives you:

- schedules tied to real session objects
- notifications and fleet visibility
- node-aware execution across multiple machines
- repeatable operational workflows without writing a platform from scratch

## When Pulpo Is The Right Tool

Pulpo is a strong fit when:

- the runtime itself needs to be self-hosted
- you use more than one coding agent tool
- you care about recovery semantics and durable state
- you want to supervise work from outside the active terminal
- you need multi-node visibility or remote spawning

Pulpo is a weaker fit when:

- you only want the easiest hosted coding-agent experience
- you mostly work inside one IDE and do not need remote execution
- one local terminal session is enough
- you need a multi-agent planner more than a runtime layer

## Competitor Matrix

The market is crowded, but the categories are different. The important question
is not "which tool has more features?" It is "which layer does each tool own?"

| Category | Examples | What they are best at | Where Pulpo differs |
| --- | --- | --- | --- |
| Hosted coding agents | Codex app, GitHub Copilot coding agent, Cursor background agents, Claude cloud sessions, OpenHands Cloud | Managed cloud execution, PR-native workflows, provider integration | Pulpo keeps the runtime on your own machines and works across agent vendors |
| Local session managers | Agent Deck, tmux-based command centers | Fast local multi-session UX, terminal-first workflows | Pulpo focuses on durable state, multi-node control, recovery, watchdog behavior, and API-first operation |
| Agent orchestration frameworks | Multi-agent planners and task routers | Decomposing work, assigning tasks, coordinating agents | Pulpo is the runtime layer underneath those workflows, not the planner |
| Generic infrastructure | Raw tmux, cron, SSH, Docker scripts | Maximum flexibility, no product constraints | Pulpo gives you lifecycle semantics, recovery, supervision, and a unified control surface |

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
- a private control plane across multiple machines

## Local Managers Vs Pulpo

Local agent managers solve a real problem: too many sessions, not enough
terminal discipline.

Pulpo overlaps with them, but aims one level deeper:

- a session is a durable object, not just a visible process
- failure and recovery behavior are explicit
- multi-node operation is native, not bolted on
- control surfaces include web, API, notifications, and scheduling

If your main problem is "I need a nicer terminal dashboard," use the best local
manager.

If your main problem is "I need agent work to behave like infrastructure," use
Pulpo.

## What Pulpo Is Not

Pulpo is not:

- a better coding model
- an IDE replacement
- a prompt framework
- a hosted code-review bot
- a multi-agent reasoning engine

It is the runtime and control layer for agent execution on infrastructure you
control.

## The Short Version

Use Pulpo when coding agents stop feeling like interactive tools and start
feeling like jobs that need to run somewhere, be observed, and survive failure.

## Read Next

1. [Install](/getting-started/install)
2. [Quickstart](/getting-started/quickstart)
3. [Core Concepts](/architecture/core-concepts)
