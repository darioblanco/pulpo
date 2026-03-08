# Pulpo Roadmap

Strategic direction for Pulpo as an open-source control plane for coding agents.

## Mission

Pulpo is infrastructure for agent runtime operations on your own machines.

It is not trying to be the best coding agent. It is the layer that makes fast-moving agents (Claude Code, Codex, and others) reliable, observable, and controllable across nodes.

## Market Reality (2026)

Agent capabilities are commoditizing quickly:

- Claude Code is a full-featured terminal/IDE/web workflow with MCP and multi-agent patterns.
- Codex CLI is a local coding agent in terminal form.
- GitHub Copilot now includes coding-agent workflows tied to issues/PRs.
- OpenHands offers CLI, local GUI, cloud, and enterprise deployment paths.
- Aider remains a strong terminal-first pair-programming tool.
- Continue is pushing source-controlled agent checks in CI.
- SWE-agent is focused on benchmarked issue-to-fix automation.

Implication: Pulpo should not compete on core agent intelligence or IDE UX. It should win on runtime operations for self-hosted, multi-node agent execution.

## Is Pulpo Helpful? Is It Unique?

### Helpful when

Pulpo is high-value if you:

- run agents on multiple machines,
- need sessions to survive daemon restarts/reboots,
- want intervention and recovery semantics you can audit,
- need a unified API/CLI/web surface independent of provider churn.

### Not especially helpful when

Pulpo is lower-value if you:

- use one laptop and one agent surface,
- only need pair-programming inside an IDE,
- do not need operational controls or history beyond local agent tooling.

### Unique wedge

Pulpo's defensible wedge is: **agent runtime control plane for trusted self-hosted environments**.

Not unique: prompting UX, code-gen quality, chat interfaces.
Unique: cross-node session lifecycle, watchdog interventions, stale/resume semantics, provider-agnostic operational API.

## Product Thesis

Pulpo should be the "Kubernetes-lite for coding agent sessions" on personal/team infrastructure:

- predictable lifecycle,
- explicit failure states,
- policy and budget guardrails,
- audit-friendly event streams,
- provider adapter portability.

## Shipped (Current Baseline)

- `pulpod` daemon + REST API + embedded web UI
- `pulpo` CLI
- SQLite-backed session persistence
- Session lifecycle: `creating`, `running`, `completed`, `dead`, `stale`
- Resume flow after stale detection
- Watchdog interventions (memory + idle)
- Binary guard toggle (`unrestricted` on/off)
- Claude Code + Codex + Gemini + OpenCode provider support
- Multi-node support (manual peers + mDNS in `public` mode)
- SSE events (`/api/v1/events`)
- MCP server mode (`pulpod mcp`)
- Scheduling via crontab wrapper
- Discord integration in `contrib/`
- **Knowledge extraction**: rule-based extraction of summaries and failure learnings when sessions end, stored in SQLite, queryable via API (`/api/v1/knowledge`, `/api/v1/knowledge/context`) and CLI (`pulpo knowledge`)

## Refactored Roadmap

## Phase A (Next 1-2 releases): Double down on control-plane fundamentals

### 1. Config surface parity (highest priority)

- Expose watchdog settings via API/UI (`GET/PUT /api/v1/watchdog`)
- Expose notifications settings via API/UI (`GET/PUT /api/v1/notifications`)
- Expose per-session overrides in API (`max_turns`, `max_budget_usd`, `output_format`)
- Expose bind mode controls in settings (with explicit restart_required semantics)

Why: reduce SSH/TOML edits and make operational policy manageable from one surface.

### 2. Reliability and auditability hardening

- Standardize intervention reason taxonomy (machine-readable codes + human text)
- Add event replay/export endpoint for postmortems (bounded window)
- Tighten stale/dead edge-case tests across daemon restart and failed kills

Why: reliability semantics are Pulpo's core moat.

### 3. Provider adapter stability layer

- Add adapter contract tests to detect CLI flag/behavior drift
- Publish provider compatibility matrix in docs (last verified versions)
- Add safer degradation paths when provider binaries/auth are invalid

Why: provider churn is guaranteed; Pulpo should absorb it.

## Phase B (Following 2-4 releases): Fleet operations for serious usage

### 4. Multi-node operations primitives

- Node labels/tags (e.g., `high-mem`, `gpu`, `cheap`)
- Scheduling constraints for spawn (`--node-label`, fallback behavior)
- Node drain/cordon semantics (stop new sessions, preserve running)

Why: this separates Pulpo from single-machine wrappers.

### 5. Policy as configuration (without platform bloat)

- Per-ink policy bundles (unrestricted toggle + budget + tool allowlists + runtime limits)
- Global policy defaults with local override visibility
- Policy dry-run endpoint to explain effective settings before spawn

Why: predictable governance without becoming an enterprise platform.

### 6. SLO-oriented observability

- Runtime metrics endpoint (session starts, failure rate, intervention rate, resume success)
- Basic dashboard trend views (24h/7d)
- Health checks that include dependency readiness (tmux/provider availability)

Why: proves operational value and prevents silent drift.

## Phase C (Longer-term): Optional collaboration layer

### 7. Team-friendly mode (re-evaluate based on demand)

- Scoped multi-user auth and ownership only if demanded by real users
- Keep single-user default unchanged

Why: only pursue if it strengthens control-plane adoption, not as a default direction.

### 8. Deployment ergonomics

- One-command upgrade/migration path for self-hosted nodes
- Official Docker deployment profiles for daemon-first production use

Why: improve operational lifecycle without changing product identity.

## De-prioritized / Removed From Active Roadmap

- Voice-command surfaces as a core product track
- Broad chat-platform feature expansion beyond control-plane needs
- Competing directly with IDE-native agent UX
- Building a monolithic all-in-one agent framework

These may exist as contrib experiments, but they are not core sequencing drivers.

## Success Criteria (to validate the thesis)

Pulpo is succeeding if we can show:

- Resume success rate after restart/reboot is consistently high
- Low false-positive watchdog kills and clear intervention explanations
- Time-to-recover from agent/node failure is materially reduced
- Users run mixed providers through one operational surface
- Multi-node usage increases vs single-node-only usage

If these metrics do not improve, the roadmap should be reconsidered.

## Architectural Principles

- Infrastructure/runtime layer, not agent intelligence layer
- Provider-agnostic adapters over provider lock-in
- Reliability before feature breadth
- Explicit, auditable failure semantics
- Zero-config local start, progressive operational depth

## Sources informing this refactor

- OpenAI Codex CLI repo/docs: https://github.com/openai/codex
- Anthropic Claude Code docs: https://docs.anthropic.com/en/docs/claude-code/overview
- GitHub Copilot agents docs: https://docs.github.com/en/copilot/how-tos/use-copilot-agents
- OpenHands docs/repo: https://docs.all-hands.dev/ and https://github.com/All-Hands-AI/OpenHands
- Aider docs/repo: https://aider.chat/docs/ and https://github.com/Aider-AI/aider
- Continue repo/docs: https://github.com/continuedev/continue
- SWE-agent repo: https://github.com/SWE-agent/SWE-agent
