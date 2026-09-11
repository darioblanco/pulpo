# Pulpo Positioning Memo

Last updated: 2026-09-11

## Category

Pulpo is a self-hosted **meter and breaker box for coding agents**. It is not an agent
model, IDE, prompt framework, or multi-agent planner. It runs your agent sessions as
durable background workers on machines you own, measures exactly what each one costs, and
can stop one before it runs past a budget you set.

## Market Context & Core Problem

The 2026 shakeout settled the orchestration question: Terragon dead, Vibe Kanban dead at
27k stars, Crystal deprecated, Omnara pivoted. First parties absorbed the value — Claude
Code ships native worktrees and Remote Control; Codex ships a desktop command center.
Wrapping tmux/worktrees/guardrails is a losing race.

What nobody ships — and what first parties are structurally unable to ship — is
cross-account, cross-machine, cross-agent cost telemetry and the enforcement that goes
with it. A vendor's `/usage` is one account, one machine, checked after the fact; no
vendor will aggregate spend across *your* accounts, since that helps you arbitrage their
own rate limits. Only the thing actually running a session can stop a runaway before the
wall — that's the gap Pulpo fills. The rest of the gap: SSH-plus-tmux as ad hoc
infrastructure, no clear signal for "working," "blocked on me," "finished," or "dead," and
nothing watching the meter closely enough to pull the plug before it's expensive.

## Target Users

**Primary:** power users and small teams who already run coding agents heavily, often
across more than one account, on always-on machines (a Mac mini, a home server, a spare
Linux box) — and want to know what all of that costs before the invoice or the weekly
quota resets. They care about self-hosting, sovereignty, and vendor independence.

**Secondary:** operators running repeated agent work — nightly reviews, scheduled scans —
who need a budget that actually intervenes, alerts that reach a phone before a runaway
gets expensive, and signals forwarded into infrastructure they already run (Grafana,
Datadog, a SIEM).

## Positioning Statement

For developers and teams who run coding agents unattended on infrastructure they own,
Pulpo is the self-hosted meter and breaker box that measures exactly what every agent
session costs, enforces budgets before the wall, and forwards alerts to the observability
stack they already run.

Unlike vendor `/usage` pages or single-machine cost readers, Pulpo runs the sessions
itself — so a cap actually stops something — and stays sovereign: usage and account data
never leave the machine that generated them.

## Wedge

- It also runs the sessions — a cost reader only tells you after the fact.
- Exact usage metering from the agent's own session files, not scraped output.
- Budgets that alert at 80% and can auto-stop at 100%, plus a burn-velocity governor for
  the runaway a flat cap misses.
- Hook-driven supervision for harnesses with their own lifecycle signals (Claude Code,
  Codex, pi) — real status instead of a scrollback guess, and real resume of the same
  conversation after a reboot.
- Self-hosted, single-node-first: no control plane to operate; `bind = "tailscale"` for
  reaching it from a phone.
- Command-agnostic: any terminal agent, not one vendor.

## What Pulpo Is Not

Not a better model than Claude, Codex, Gemini, or Aider. Not a replacement for
IDE-native coding UX. Not a multi-agent planning framework. Not a fleet control plane —
there's no cross-node orchestration, by design. Not "tmux, but prettier."

## Messaging Guidance

**Lead with:** what every coding agent costs, across every machine and account; a budget
that actually pulls the plug, not a post-hoc invoice; self-hosted and sovereign — your
code and usage data never leave your infrastructure; run any agent, on your machines.

**Avoid leading with:** tmux abstraction or implementation details; "control plane" or
"orchestrator" as the primary frame (that race is lost); multi-node fleet management
(explicitly not a goal); a feature list before the cost problem.

## Proof Points

`pulpo usage --scan` shows real spend across Claude Code, Codex, and pi with zero setup. A
budget cap stops a runaway session and records why. An agent blocked on a permission
prompt shows `needs input (<reason>)` and pings a phone. A session survives a reboot and
resumes the same conversation, not a fresh one. Every alert reaches a webhook or a phone,
not just a dashboard nobody's watching.

## Competitive Framing

- **Cost readers** (ccusage, vendor `/usage`) win on a read-only report, one machine,
  right now. Pulpo also runs the session — a cap can stop something — and aggregates
  across machines and accounts.
- **Native multi-agent UX tools** (Conductor, Claude Code Remote Control, Codex desktop)
  win on the nicest interactive experience on one Mac. Pulpo is self-hostable and
  headless, command-agnostic, and meters/enforces in the daemon, not the terminal app —
  a session survives a closed laptop lid.
- **Hosted coding-agent clouds** win on zero infrastructure. Pulpo keeps the runtime, the
  credentials, and the cost data on hardware the operator administers.

See [Alternatives And Comparisons](docs/getting-started/alternatives.md) for the fully
sourced version.

## Recommended One-Liners

- The self-hosted meter and breaker box for coding agents.
- See — and control — what every coding agent costs, across all your machines and accounts.
- Run any coding agent on your machines. Know exactly what it costs. Pull the plug before
  the wall.

## Documentation & Roadmap Implications

Docs should open with the meter/breaker framing (not control-plane/orchestration), state
the cost problem and primary user early, treat tmux as plumbing rather than the headline,
and be explicit that Pulpo is single-node-first — direct (`--url`) multi-machine access
and shared webhooks for cross-machine visibility, never a fleet control plane.

Roadmap priority favors exact usage metering for more agents, budget/burn-velocity depth,
and hook-driven supervision for more harnesses. Deprioritize cross-node orchestration,
multi-user/team features, and anything agents now handle natively (worktrees, sandboxing,
guardrails).
