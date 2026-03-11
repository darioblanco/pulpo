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

## Shipped

- `pulpod` daemon + REST API + embedded web UI
- `pulpo` CLI
- SQLite-backed session persistence
- Session lifecycle: `creating`, `running`, `completed`, `dead`, `stale`
- Resume flow after stale detection
- Watchdog interventions (memory + idle) with live config reload via watch channel
- Machine-readable intervention reason codes (`InterventionCode` enum)
- Binary guard toggle (`unrestricted` on/off)
- Claude Code + Codex + Gemini + OpenCode + Shell (bare tmux) provider support
- Provider availability detection and compatibility matrix (`GET /api/v1/providers`)
- Graceful 400 error when provider binary is missing at spawn
- Multi-node support (manual peers + mDNS in `public` mode)
- Full config surface via API/UI (watchdog, notifications, per-session overrides, bind mode)
- SSE events (`/api/v1/events`)
- MCP server mode (`pulpod mcp`)
- Scheduling via crontab wrapper
- Discord integration in `contrib/`
- **Culture system**: git-backed culture repo with extraction, injection, and human CRUD
  - Extraction: rule-based summaries and failure learnings from completed sessions
  - Storage: JSON files in a local git repo (`<data_dir>/culture/`), optional remote sync
  - Injection: context breadcrumbs + write-back instructions injected into new sessions at spawn
  - CRUD API: `GET/PUT/DELETE /api/v1/culture/{id}`, `POST /culture/push`
  - CLI: `pulpo culture` with `--get`, `--delete`, `--push`, `--context` flags
  - Web: `/culture` page with filtering, deletion, and push-to-remote
  - Inks: 6-field universal roles (description, provider, model, mode, unrestricted, instructions)

## What's Next?

The control-plane fundamentals are solid. The question now is: what would make Pulpo meaningfully more useful day-to-day, rather than adding enterprise features nobody needs yet?

Open questions to guide the next phase:

- Is the culture system actually producing useful learnings, or is it noise?
- Are there friction points in the spawn → monitor → kill/resume loop?
- Is multi-node orchestration a real daily need, or a theoretical one?
- Would better integration with existing tools (git workflows, CI, etc.) matter more than new Pulpo features?

The roadmap should be driven by real usage friction, not speculative feature lists.

## Parked (revisit when demanded by real usage)

- Node labels/tags and scheduling constraints — useful at fleet scale, premature now
- Per-ink policy bundles — inks already cover the common case; per-ink budgets/limits add complexity without clear demand
- SLO metrics endpoint — observability for its own sake; the dashboard already shows what matters
- Team-friendly multi-user auth — only if real users demand it
- Docker deployment profiles — only if self-hosted adoption grows
- Restart-required UI for bind/port changes — narrow edge case, self-diagnosing

## De-prioritized / Removed

- Voice-command surfaces as a core product track
- Broad chat-platform feature expansion beyond control-plane needs
- Competing directly with IDE-native agent UX
- Building a monolithic all-in-one agent framework
- Event replay/export endpoint — speculative, no clear consumer
- Adapter contract tests against real provider binaries — fragile, environment-dependent

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
