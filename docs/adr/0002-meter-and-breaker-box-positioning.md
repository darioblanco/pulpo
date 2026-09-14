# 0002. The meter-and-breaker-box positioning, and what was cut to get there

- **Status:** Accepted
- **Date:** 2026-06 (the bet) through 2026-07 and 2026-09-11 (the cuts: controller PR
  [#84](https://github.com/darioblanco/pulpo/pull/84), inks PR
  [#91](https://github.com/darioblanco/pulpo/pull/91), and PRs
  [#99](https://github.com/darioblanco/pulpo/pull/99)–[#104](https://github.com/darioblanco/pulpo/pull/104))

## Context

The early-2026 agent-orchestration shakeout settled a question Pulpo's original pitch
depended on: Terragon dead, Vibe Kanban dead at 27k stars, Crystal deprecated, Omnara
pivoted. First parties absorbed the value instead of a wrapper layer — Claude Code
shipped native worktrees, agent teams, and Remote Control (a built-in per-machine
session daemon with mobile push); Codex shipped a desktop command center. Wrapping
`tmux`/worktrees/guardrails, which is what Pulpo originally did, had become a losing
race against the vendors themselves.

What nobody shipped, and what first parties are structurally incentive-blocked from
ever shipping, was cross-account, cross-agent cost telemetry, exactly measured, plus
the enforcement that goes with it — no vendor will aggregate spend across a user's
*other* accounts, because that would help the user arbitrage the vendor's own rate
limits.

## Decision

We will reposition Pulpo as **the self-hosted meter and breaker box for coding
agents** — infrastructure that measures exactly what every agent session costs across
accounts and agents, enforces a budget cap before the wall, and forwards events to the
operator's own observability stack. Not an orchestrator, not a terminal-UX wrapper, not
a fleet control plane.

Everything that competed with what first parties now do natively, or that only existed
to support a cross-node control plane, gets cut rather than kept dormant:

- **Cross-node controller/node control plane** — frozen 2026-06-14, then removed
  outright in July 2026 (PR #84). Cross-node aggregation is already covered by the
  event-forwarding backbone (every node forwards to its own `[[webhooks]]`); a bespoke
  controller was a liability in security review, not an asset.
- **Inks** (`[inks.<name>]` preset registry) — removed July 2026 (PR #91). The
  community standardized agent-side config (AGENTS.md, skills); a pulpo-proprietary
  preset registry was config overhead nobody used. Command now lives directly on
  sessions/schedules.
- **Windows build target** — dropped September 2026 (PR #99). Sessions run in `tmux`,
  which native Windows doesn't have; the Docker session runtime that once justified a
  Windows binary was itself removed earlier (v0.1.0, PR #53).
- **Containerized `pulpod` deployment** (`docker/`, `bind = "container"`) — removed
  September 2026 (PR #101). A containerized `pulpod` can't see the agents' own session
  files that exact usage metering depends on.
- **Watchdog auto-adoption of external tmux sessions** (`adopt_tmux`) — removed
  September 2026 (PR #102). Since harness adapters (ADR
  [0001](0001-hook-driven-agent-state.md)), a session started through `pulpo spawn`
  gets real hooks and resume; an adopted session got none of that, and its interaction
  with the tmux `$N` id space caused zombie sessions after a reboot.
- **Secrets store** — removed September 2026 (PR #103). Every supported agent reads
  its own credentials from its own config; a metering tool has no business storing
  them, and the owner's database had zero secrets stored after five months in
  production.
- **Peer registry, peer health probing, Tailscale peer discovery, `--node` routing** —
  removed September 2026 (PR #104). A read-only list of other nodes' sessions with no
  way to act on them wasn't worth the config surface. `bind = "tailscale"` stays as
  transport, unrelated to peer discovery.

## Consequences

- Pulpo is smaller and more honestly scoped: single-node-first, reached directly
  (`pulpo --url <host:port>`) plus shared `[[webhooks]]` for cross-machine visibility —
  never a fleet control plane.
- If central governance (an org-wide kill switch, say) is ever needed, it starts from
  zero design, not from a resurrection of the removed controller code — that code was
  deleted, not archived, so paying no ongoing maintenance tax on it.
- `POSITIONING.md` and `MISSION.md` are folded into `ROADMAP.md` (the bet, current
  scope) and `docs/getting-started/why-pulpo.md` (ICPs, competitor framing,
  messaging) — this ADR is the durable record of *why* the cuts happened, so the
  narrative doesn't have to be re-derived from commit messages later.
- Every cut here is documented in `ROADMAP.md` → "Removed" with its own rationale;
  this ADR is the one-page summary of the positioning bet that made all of them make
  sense together.
