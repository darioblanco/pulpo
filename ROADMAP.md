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

Pulpo's defensible wedge is: **agent runtime control plane with collective culture for trusted self-hosted environments**.

Not unique: prompting UX, code-gen quality, chat interfaces.
Unique: cross-node session lifecycle, watchdog interventions, stale/resume semantics, provider-agnostic operational API, **agent-driven culture accumulation** — agents learn from each other's sessions and improve over time via AGENTS.md-formatted shared learnings.

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
- Inks: 6-field universal roles (description, provider, model, mode, unrestricted, instructions)
- **Culture system** (C1–C4 complete): agent-driven collective learning across nodes
  - C1 — AGENTS.md as the format: markdown files in scoped directories (`culture/`, `repos/<slug>/`, `inks/<ink>/`), bootstrap template, file browser UI, JSON→markdown migration (`4661f4d`, `d7672a9`)
  - C2 — Structured write-back and harvest: agents write `pending/<session>.md` files, harvested on session completion, rule-based extraction removed (`e895aa8`, `b5b5b59`)
  - C3 — Culture lifecycle: relevance scoring via `last_referenced_at`, TTL decay with stale flagging, supersede/contradiction replacement, approve/reject curation, standalone curator fallback (`250d7cf`)
  - C4 — Cross-node sync: background pull loop with rebase-first conflict resolution, selective scope filtering, `Mutex` concurrency guard, `GET /api/v1/culture/sync` status endpoint, culture SSE events (`caba6f7`)
- **Integration polish** (P1–P4 complete):
  - P1 — Real-time culture in web UI: SSE-driven auto-refresh, toast notifications on sync
  - P2 — Discord culture notifications: culture event listener + embed formatting
  - P3 — Node info completeness: real memory + GPU detection in peers endpoint
  - P4 — SPEC.md refresh: culture system, sync, SSE event types documented

## What's Next: Session Lifecycle Hardening

The session state machine has a critical gap: **sessions never automatically reach a terminal state**. When an agent finishes its work, the session stays `Running` forever because `exec bash` keeps the tmux session alive. The `[pulpo] Agent exited` marker is already emitted but never detected. Meanwhile, `Completed` and `Stale` are effectively dead code.

This undermines the entire lifecycle model. The fix is a full state rename (Option C) that makes states user-centric, plus new detection logic.

### State rename: Running/Completed/Dead/Stale → Active/Idle/Finished/Killed/Lost

| Old | New | Meaning |
|-----|-----|---------|
| Creating | Creating | Setting up (keep) |
| Running | **Active** | Agent is working — output is changing |
| _(new)_ | **Idle** | Agent needs user attention — waiting for input or at its prompt |
| Completed | **Finished** | Agent process exited — task is done |
| Dead | **Killed** | Session was terminated (user, watchdog memory, watchdog idle timeout) |
| Stale | **Lost** | tmux process disappeared unexpectedly (crash, reboot) |

**Key semantics:**
- `unrestricted` is a **guard toggle** (pass-through to agent CLI flags like `--dangerously-skip-permissions`), NOT a mode. It's orthogonal to Interactive/Autonomous. Pulpo doesn't enforce permissions — the agent binary does. Pulpo only observes terminal output.
- **Interactive sessions** cycle `Active ⇄ Idle` until the user kills the session or exits the agent. `Idle` fires on permission prompts (if restricted) AND "what's next?" prompts.
- **Autonomous sessions** go `Active → Finished` (if unrestricted) or `Active → Idle → Active → ... → Finished` (if restricted, due to permission prompts).
- **Shell sessions** cycle `Active ⇄ Idle` based on whether a command is running in the bash prompt.
- `Finished` is terminal — no automatic transition back. Resume from `Finished` restarts the agent.
- `Killed` is terminal — no resume. Create a new session.
- `Lost` allows resume (recreate tmux session).

### S1 — State rename (mechanical refactor)

Rename enum variants, serde strings, Display/FromStr, pattern matches across ~25 files. No new logic — pure rename.

- `SessionStatus::Running → Active`, `Completed → Finished`, `Dead → Killed`, `Stale → Lost`
- Add `SessionStatus::Idle` variant
- Update all Rust crates, web components, CSS vars, ocean sprites, Discord bot, tests
- Ocean visual mapping: Active = lavender (was running), Idle = amber (was stale), Finished = emerald (was completed), Killed = red (was dead), Lost = red recolor (same sprite as Killed)
- Ocean behavior: Idle gets minimal movement / small radius (was stale behavior)
- Update `waiting_for_input` flag → remove it, replaced by `Idle` state
- DB migration: update `status` column default from `'creating'` to `'creating'`, rename existing status values in-place
- Config: `notification_events` default changes from `["completed", "dead"]` to `["finished", "killed"]`

### S2 — Idle detection (Active ⇄ Idle transitions)

Promote the existing `waiting_for_input` detection into real state transitions.

- **Active → Idle**: watchdog detects output unchanged for 1 tick AND (waiting patterns matched OR agent at its prompt). Piggybacks on existing output snapshot comparison — no performance cost.
- **Idle → Active**: watchdog detects output changed since last tick. Transition back to Active.
- **Shell idle**: detect bash prompt idle (no running command) → Idle. Command running → Active.
- Remove `waiting_for_input` DB column and session field (replaced by `Idle` status)
- SSE events emitted on all transitions (web UI needs them). Discord bot filters to only notify on Finished/Killed/Lost.

### S3 — Finished detection (agent exit)

Detect the `[pulpo] Agent exited` marker and transition to `Finished`.

- **Detection**: watchdog checks for `[pulpo] Agent exited` in captured output → `Active/Idle → Finished`
- **Culture extraction**: trigger on `Finished` (same as current kill/stale paths)
- **Keep `exec bash`**: tmux shell stays alive for inspection, but session state reflects agent is done
- **Resume from Finished**: allowed — restarts agent command in new tmux session

### S4 — Lost refinement and cleanup

- **Finished + TTL → cleanup**: configurable auto-kill of tmux shell after grace period once Finished. Keeps the process around briefly for inspection, then cleans up.
- **Resume semantics**: Lost → recreate tmux + restart agent. Finished → restart agent. Killed → blocked.
- **Dashboard**: filters for terminal states (Finished/Killed/Lost) are now accurate

### S5 — Session lifecycle documentation

- `docs/session-lifecycle.md`: full state machine diagram, transition rules, detection mechanisms, mode × guard matrix, corner cases, visual mapping
- Update SPEC.md session lifecycle section
- Update CLAUDE.md if conventions change

## Future Directions

After session lifecycle is solid, potential next phases:

### Culture quality

- Improve what agents write back (better prompts, validation, deduplication)
- Culture effectiveness metrics: do agents with culture produce better results?
- Automated culture pruning based on staleness and contradiction detection

### Fleet observability

- Aggregated metrics across nodes (session counts, culture growth, sync health)
- Cross-node session routing and load balancing
- Fleet-wide dashboard view

### Packaging and distribution

- Homebrew formula for macOS
- Docker image for Linux deployment
- Streamlined onboarding (guided setup wizard)
- README and documentation for open-source readiness

### Real-world hardening

- Multi-node stress testing with concurrent sessions
- Edge case handling for network partitions during culture sync
- Provider binary upgrade resilience (agent binary updated mid-session)

## Parked (revisit when demanded by real usage)

- MCP server expansion — the existing `pulpod mcp` STDIO server (12 tools, 4 resources) works and is well-tested, but the industry is trending toward REST APIs over MCP for agent integration. Keep as-is; no new MCP tools until demand proves otherwise. STDIO-only transport means no additional attack surface beyond local process access.
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
- Culture entries are actionable (agents produce better results with culture than without)
- Write-back rate: agents actually write pending culture files in a meaningful percentage of sessions
- Culture noise ratio decreases over time as lifecycle pruning takes effect

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
