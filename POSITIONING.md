# Pulpo Positioning Memo

Last updated: 2026-03-30

## Category

Pulpo is a self-hosted control plane for background coding agents.

It is not an agent model, IDE, prompt framework, or multi-agent planner. It is
the infrastructure layer that lets you run coding agents on your own machines
with durable state, explicit lifecycle semantics, and remote supervision.

## Market Context

The market has shifted from "AI pair programmer in my editor" to "background
coding agent that works while I am away." Managed platforms now offer cloud
agents, PR-based delegation, remote sandboxes, and async task execution.

That validates Pulpo's core thesis:

- agents increasingly run unattended
- unattended agents need supervision and recovery
- agents need durable execution environments
- developers need status, alerts, and control without staying attached

The opportunity is not to outdo hosted vendors at model quality or cloud UX. The
opportunity is to be the best way to run agents on infrastructure you control.

## Target Users

### Primary ICP

Individual power users and small engineering teams who:

- already use coding agents heavily
- run private infrastructure or always-on machines
- want agents to work in the background on servers, not laptops
- need access to private repos, VPN-only services, or internal environments
- care about self-hosting, auditability, and vendor independence

### Secondary ICP

Teams adopting coding agents operationally who need:

- repeatable scheduled runs
- per-session policies and recovery behavior
- remote visibility for long-running tasks
- a path from one machine to a small fleet

## Core Problem

Running a coding agent in a terminal is easy.

Running many agents reliably, across machines, while you are not watching is not.

The gap shows up as:

- SSH + tmux as ad hoc infrastructure
- lost state after reboots or crashes
- poor visibility into whether an agent is active, waiting, finished, or dead
- conflicts when multiple agents touch the same repo
- no clean mobile or remote management surface

## Positioning Statement

For developers and teams who want coding agents to run in the background on
their own infrastructure, Pulpo is the self-hosted control plane that runs,
supervises, and recovers agent sessions across machines.

Unlike vendor-hosted coding agents or local-only session managers, Pulpo is
command-agnostic, multi-node aware, durable across failures, and designed for
private infrastructure you control.

## Wedge

Pulpo wins where hosted products and local tools both fall short:

- self-hosted execution on your own machines
- support for any CLI agent, not one vendor
- explicit session lifecycle with resume and intervention semantics
- fleet visibility across multiple nodes
- mobile-friendly remote supervision
- worktree and Docker isolation for concurrent or risky tasks

## What Pulpo Is Not

- not a better model than Claude, Codex, Gemini, or Aider
- not a replacement for IDE-native coding UX
- not a multi-agent planning framework
- not a hosted code-review bot
- not "tmux, but prettier"

## Messaging Guidance

### Lead with

- run coding agents on your servers, not theirs
- self-hosted background agents
- supervise agents from anywhere
- durable sessions across your machines
- private control plane for agent fleets

### Avoid leading with

- tmux abstraction
- implementation details before user value
- "universal runtime" as the primary frame
- feature lists before the core problem

## Proof Points

Pulpo should repeatedly demonstrate these outcomes:

- spawn an agent on a remote machine without SSH
- check status from a phone while away from the desk
- survive reboot or backend loss and resume work
- run multiple agents on one repo without collisions
- schedule recurring agent work on the right machine
- keep risky or high-permission sessions isolated in Docker

## Competitive Framing

### Hosted coding agents

Examples: OpenAI Codex app, GitHub Copilot coding agent, Cursor background
agents, Claude cloud sessions, OpenHands Cloud.

Pulpo should not compete on hosted convenience or model ownership. It should
compete on infrastructure control, private-network access, and bring-your-own
agent flexibility.

### Local session managers

Examples: Agent Deck and similar terminal-first tools.

Pulpo should position beyond "command center" toward "durable control plane":
multi-node, recovery semantics, watchdog behavior, scheduling, notifications,
and API-driven operation.

### Agent orchestration frameworks

Examples: multi-agent planners and task routers.

Pulpo is complementary. Those tools decide what agents should do. Pulpo decides
where and how they run, how they are supervised, and what happens when things go
wrong.

## Recommended One-Liners

- Self-hosted background agents for your own machines.
- Run any coding agent on your servers. Supervise it from anywhere.
- The private control plane for background coding agents.

## Documentation Implications

Top-level docs should:

- open with the control-plane framing
- state the primary user and problem early
- describe tmux and Docker as execution backends, not the headline
- emphasize remote supervision, durability, and multi-machine operation
- treat worktrees, scheduling, secrets, and notifications as operational depth

## Roadmap Implications

Near-term roadmap priority should favor:

- clearer distribution and onboarding
- stronger proof of value in docs and demos
- reliability and policy features that reinforce the control-plane position
- team-readiness features only when they strengthen auditability and governance

Lower priority:

- broadening into orchestration or prompt-layer features
- speculative platform expansion without user pull
