# Use Cases

This page maps Pulpo's features to concrete users and jobs.

If "self-hosted control plane for background coding agents" sounds right but you
want to know whether it matches your workflow, start here.

## 1. Solo Developer With A Home Server Or Mac Mini

You already use coding agents heavily and want them to keep working after you
close the laptop.

Typical setup:

- one always-on Mac mini or Linux box
- one or more local repos
- Claude Code, Codex, Gemini CLI, Aider, or shell automation

What Pulpo is doing for you:

- keeping agent sessions durable
- letting you check progress from your phone
- making reboot and crash recovery explicit
- enabling multiple parallel agents with worktrees

Best docs to read next:

- [Quickstart](/getting-started/quickstart)
- [Parallel Agents On One Repo](/guides/parallel-agents-one-repo)
- [Worktrees Guide](/guides/worktrees)
- [Session Lifecycle](/operations/session-lifecycle)

## 2. Small Team With Private Infrastructure

Your repos, services, or credentials are not a clean fit for hosted sandboxes.
You want agents to run near internal systems and stay under your control.

Typical setup:

- private repos or internal APIs
- VPN-only or Tailscale-only network access
- multiple machines with different capabilities
- more than one agent vendor in active use

What Pulpo is doing for you:

- keeping the runtime on infrastructure you control
- providing one control surface across machines
- supporting secrets, Docker isolation, and explicit recovery behavior
- allowing teams to adopt agent workflows without committing to one vendor

Best docs to read next:

- [Discovery Guide](/guides/discovery)
- [Secrets](/guides/secrets)
- [Alternatives And Comparisons](/getting-started/alternatives)

## 3. Operator Running Recurring Agent Work

You are past ad hoc prompting. You want repeatable, unattended jobs such as
nightly review, weekly security scans, documentation sweeps, or migration
rehearsals.

Typical setup:

- scheduled work on one or more repos
- long-running sessions
- need for notifications, status checks, and quick intervention

What Pulpo is doing for you:

- turning each run into a managed session instead of a disposable command
- exposing lifecycle state and output over CLI, UI, and API
- making scheduled background work visible and debuggable

Best docs to read next:

- [Quickstart](/getting-started/quickstart)
- [Configuration Guide](/guides/configuration)
- [Nightly Code Review](/guides/nightly-code-review)
- [Architecture Overview](/architecture/overview)

## 4. User Evaluating Pulpo Against Hosted Agents

You are comparing Pulpo with Codex app, Copilot coding agent, Cursor background
agents, Claude cloud sessions, or OpenHands Cloud.

The key decision is not "which tool is more advanced?" It is "where should the
runtime live, and who should control it?"

Choose Pulpo if you need:

- self-hosted execution
- private-network access
- command-agnostic agent support
- fleet-style supervision across your own machines

Choose hosted products if you need:

- the fastest path to a managed cloud workflow
- deep integration with one provider's product surface
- minimal operational setup

Best docs to read next:

- [Why Pulpo](/getting-started/why-pulpo)
- [Alternatives And Comparisons](/getting-started/alternatives)

## 5. User Evaluating Pulpo Against Local Session Managers

You may already have a good terminal workflow and want to know whether Pulpo is
an upgrade or just a different shape of tool.

If your problem is mostly:

- too many local sessions
- awkward terminal navigation
- wanting a better session dashboard

then a local session manager may be enough.

If your problem is:

- agents should run remotely or across machines
- sessions should survive failure with explicit semantics
- watchdog and intervention behavior should be daemon-owned
- a phone or web UI should be a real control surface

then Pulpo is aimed more directly at that problem.

Best docs to read next:

- [Alternatives And Comparisons](/getting-started/alternatives)
- [Architecture Overview](/architecture/overview)

## Quick Decision Table

| If you need... | Start here |
| --- | --- |
| One agent running on your own server tonight | [Quickstart](/getting-started/quickstart) |
| A recurring overnight review workflow | [Nightly Code Review](/guides/nightly-code-review) |
| Parallel agents on the same repository | [Parallel Agents On One Repo](/guides/parallel-agents-one-repo) |
| Multiple agents on one repo safely | [Worktrees Guide](/guides/worktrees) |
| Multi-node control over Tailscale or LAN | [Discovery Guide](/guides/discovery) |
| Secrets and safer Docker runs | [Secrets](/guides/secrets) |
| Objective comparison with alternatives | [Alternatives And Comparisons](/getting-started/alternatives) |
