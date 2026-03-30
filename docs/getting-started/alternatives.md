# Alternatives And Comparisons

This page is intentionally category-based and source-based.

The goal is not to force unlike products into a fake head-to-head. The goal is
to help you decide which layer you actually need:

- a hosted coding agent
- a local session manager
- an orchestration framework
- raw infrastructure
- or Pulpo

All descriptions below are based on public docs, public repositories, or public
product pages as of 2026-03-30.

## Comparison Principles

When comparing Pulpo to other tools, the fairest questions are:

1. Where does the agent runtime live?
2. Who controls the execution environment?
3. Is the product built around one agent vendor or many?
4. Is the main value interactive UX, hosted automation, orchestration, or infrastructure?
5. What happens when the task runs unattended and something goes wrong?

## Category 1: Hosted Coding Agents

Examples:

- OpenAI Codex app
- GitHub Copilot coding agent
- Cursor background agents
- Claude Code web, desktop, and cloud surfaces
- OpenHands Cloud

### What This Category Is Best At

Hosted coding agents are strongest when you want:

- the fastest path to a working background agent
- provider-managed infrastructure
- tight integration with one product ecosystem
- cloud or PR-native workflows with minimal setup

### Public Positioning Signals

- OpenAI describes the Codex app as "a command center for agents" and says it is
  designed to manage multiple agents, run work in parallel, and collaborate over
  long-running tasks.
- GitHub says Copilot coding agent works independently in the background,
  completes tasks in a GitHub Actions-powered environment, and opens pull
  requests for review.
- Cursor documents background agents as asynchronous remote agents that edit and
  run code in a remote environment.
- Anthropic documents Claude Code across terminal, desktop, web, and mobile
  surfaces, including long-running tasks, cloud sessions, and recurring tasks.
- OpenHands markets its cloud product as the open platform for cloud coding
  agents, with cloud, API, and self-hosted deployment options.

### Where Pulpo Differs

Pulpo is not a hosted coding agent. It is the self-hosted control plane for
running agent sessions on infrastructure you control.

The main differences are:

- Pulpo keeps the runtime on your machines instead of a vendor-managed cloud
  environment.
- Pulpo is command-agnostic across agent tools instead of centered on one agent
  product.
- Pulpo emphasizes daemon-owned session lifecycle, recovery, and intervention
  semantics across machines you control.
- Pulpo fits best when private-network access, self-hosting, or bring-your-own
  agent flexibility matter more than managed-cloud convenience.

### Where Hosted Coding Agents May Be A Better Fit

Hosted products may be the better fit when you want:

- the simplest onboarding path
- a deeply integrated vendor workflow
- issue- and PR-native delegation out of the box
- no infrastructure to manage

## Category 2: Local Session Managers

Examples:

- Agent Deck
- other tmux or terminal-first multi-session tools

### What This Category Is Best At

Local session managers are strongest when you want:

- better visibility across many local or terminal sessions
- fast switching between agents
- terminal-first workflows
- convenience features around worktrees, status views, or per-session tooling

### Public Positioning Signals

- Agent Deck describes itself as a "Terminal session manager for AI coding
  agents" and "Your AI agent command center."
- Its README emphasizes one terminal, many agents, visibility, quick switching,
  worktrees, Docker sandboxing, and conductor workflows.

### Where Pulpo Differs

Pulpo overlaps with this category, but aims at a different operating model.

Pulpo centers:

- durable session objects with explicit lifecycle states
- recovery after reboot or backend loss
- multi-node control across machines
- watchdog-driven intervention semantics
- API, web UI, notifications, and scheduling as first-class surfaces

If your main problem is local session sprawl, a local manager may be the better
fit.

If your main problem is that unattended agent work should behave like
infrastructure, Pulpo is closer to that need.

## Category 3: Orchestration Frameworks

Examples:

- multi-agent planners
- task routers
- systems that decompose work across agent roles

### What This Category Is Best At

These tools are strongest when you want:

- multiple agents collaborating on one higher-level workflow
- explicit task decomposition
- routing or assigning work among specialized agents
- orchestration logic above the execution layer

### Where Pulpo Differs

Pulpo does not decide how agents collaborate.

Pulpo is the runtime layer underneath that work:

- where sessions run
- how they are supervised
- how they are resumed or stopped
- how they are observed across machines

These categories are often complementary, not competitive.

## Category 4: Raw Infrastructure

Examples:

- tmux
- SSH
- cron
- Docker scripts
- generic sandbox infrastructure such as Vercel Sandbox

### What This Category Is Best At

Raw infrastructure is strongest when you want:

- maximum flexibility
- full control over implementation details
- no product opinion on lifecycle or workflow
- a primitive you can build your own system on top of

### Public Positioning Signals

- Vercel Sandbox documents itself as an ephemeral compute primitive for running
  untrusted or user-generated code, including AI agent workloads.

### Where Pulpo Differs

Pulpo is opinionated infrastructure for coding-agent operations.

It adds:

- session lifecycle states
- stored output and resumability
- watchdog supervision
- multi-node control surfaces
- scheduling, notifications, and worktree-aware session management

If you want building blocks, use building blocks.

If you want a self-hosted control plane for background coding agents, use
Pulpo.

## Quick Decision Guide

Use hosted coding agents when:

- convenience matters more than runtime control
- your repos and workflow already live comfortably inside one vendor ecosystem
- you do not need the runtime on your own machines

Use a local session manager when:

- you mostly need better local multi-session UX
- your sessions are still primarily terminal-centric and interactive

Use orchestration frameworks when:

- your main problem is coordination between multiple agents

Use raw infrastructure when:

- you want to assemble your own platform from primitives

Use Pulpo when:

- the runtime needs to stay on infrastructure you control
- you want to run any coding agent, not standardize on one vendor
- you need durable sessions, recovery semantics, and remote supervision
- you want one control model across multiple machines

## Sources

- OpenAI Codex app: <https://openai.com/index/introducing-the-codex-app>
- GitHub Copilot coding agent: <https://docs.github.com/en/copilot/concepts/agents/coding-agent/about-coding-agent>
- Cursor background agents: <https://docs.cursor.com/en/background-agents>
- Claude Code overview: <https://code.claude.com/docs/en/overview>
- Claude Code analytics: <https://code.claude.com/docs/en/analytics>
- OpenHands homepage: <https://openhands.dev/>
- OpenHands Cloud docs: <https://docs.openhands.dev/usage/cloud/openhands-cloud>
- Agent Deck repository: <https://github.com/asheshgoplani/agent-deck>
- Vercel Sandbox docs: <https://vercel.com/docs/vercel-sandbox/>
