# Pulpo Roadmap

Strategic direction for Pulpo: the self-hosted meter and breaker box for coding agents.

## Mission

Pulpo runs coding agents as background workers on your machines, **measures exactly what
every session costs** — across all your agents, accounts, and machines — and **enforces
budgets and quota-aware placement** so you get the maximum out of your subscriptions
without ever blowing a limit.

It works with any command-line agent: Claude Code, Codex, Aider, Goose, OpenCode, or
anything that runs in a terminal. It is not an agent framework, not a prompt tool, and
not a terminal-orchestration UX — agents now handle their own interactive worktree UX,
sandboxing, and guardrails better than any wrapper can. (Spawn-time worktree *isolation*
for unattended sessions stays — that's infrastructure, not UX.) Pulpo is the layer those
agents don't have:
usage telemetry, cost control, and fleet-wide supervision on infrastructure you own.

tmux is plumbing, not product: it is the universal way to run any agent as an
observable, killable, attributable process without modifying it.

## The Bet (June 2026)

The early-2026 shakeout settled the orchestration question: Terragon dead, Vibe Kanban
dead at 27k stars, Crystal deprecated, Omnara pivoted. First parties absorbed the value —
Claude Code ships native worktrees, agent teams, Remote Control (a built-in per-machine
session daemon with mobile push); Codex ships a desktop command center. Wrapping
tmux/worktrees/guardrails is a losing race.

What nobody ships — and what first parties are **incentive-blocked** from ever shipping:

1. **Cross-machine, cross-account, cross-agent cost telemetry.** `/usage` is one account,
   one machine, one vendor, and you have to go look at it. ccusage (~16k stars, proving
   the demand) is explicitly single-machine. No vendor will ever aggregate across your
   accounts, because that means helping you arbitrage their own rate limits.
2. **Enforcement.** Budget caps that auto-stop, pause-on-rate-limit-thrash, alerts before
   the wall. Vendors tell you that you overspent; only the thing running the session can
   prevent it.
3. **Quota-aware placement.** "Spawn this on whichever node/account has the most
   headroom; defer the nightly run until the window resets." Requires fleet state +
   quota data + a scheduler — Pulpo has all three.

The #1 community complaint about parallel agents is that they are a *quota multiplier*
(Max users burning 20% of a weekly allowance in 2 hours). That pain grows with every
model-price increase. This is the gap Pulpo fills.

Sovereignty remains the supporting argument: the daemon reads usage and account identity
from local files and never ships them anywhere except your own controller node. Exactly
the data you'd least want in a third-party relay. (CLOUD Act / EU AI Act / GDPR context
unchanged — see git history of this file for the full sovereignty section.)

## Gauge vs. Control System — what Pulpo answers that `/usage` can't

1. **Attribution** — "the nightly review ink costs €11/week"; per-session, per-ink,
   per-repo. Only the thing managing sessions can tie spend to tasks.
2. **Fleet gauge** — all accounts, machines, and agents on one phone screen.
3. **Projection** — "at this burn rate you hit the weekly cap Thursday 15:00."
4. **Placement** — spawn where there's headroom.
5. **Enforcement** — stop anything that exceeds its budget; recorded as interventions.

## Scope: Keep / Cut

**Keep — everything the meter needs:**

| Component | New role |
|-----------|----------|
| tmux backend | Universal process substrate: run, observe, kill, attribute |
| Session lifecycle + SQLite persistence | Attribution unit; survives reboots |
| Watchdog | The enforcement engine (budgets, idle, memory, thrash) |
| Scheduler | Quota-aware dispatch |
| Worktree spawning (`--worktree`) + cleanup | Isolation primitive: scheduled/parallel sessions on one repo can't trample each other, agent-agnostically; watchdog sweeps litter |
| Inks | Attribution + budget + priority unit |
| Fleet / controller mode | The rollup plane (telemetry + dispatch, not remote terminals) |
| PWA web UI | The gauge; mobile-first |
| CLI, secret store, webhook/web-push notifications | Supporting surface |

**Cut — orchestration we're losing at, plus dead weight (Track R):**

- Worktrees *page* in the web UI — interactive worktree management is commoditized UX
  (Claude Code `--worktree`, Conductor, Codex app); fold branch/diff telemetry into the
  session detail view. Spawn-time worktree creation stays — see Keep table above.
- Docker runtime backend — agents ship their own sandboxing, and containerized agents
  hide their session files from the structured usage readers (Phase A), undermining the
  exact-metering vision.
- Tauri native iOS/Android builds — PWA + web push covers mobile (confirmed 2026-06-12)
- Voice / Siri Shortcuts (confirmed 2026-06-12)
- MCP server — REST is the integration surface
- Discord bot — archive to its own repo

**Kept despite earlier plans (owner's call, 2026-06-12):**

- Ocean gamification — stays for now, frozen (no new investment). Its canvas code is
  excluded from web coverage (untestable under jsdom).

## Plan

### Track R — Removals (parallel, one PR each, no dependencies)

Voice, Tauri mobile, MCP server, Docker runtime, worktrees web-UI page, Discord bot.
Each PR shrinks the binary, the test surface, and the README.

### Phase A — Exact usage telemetry (the foundation)

Replace terminal-scraping with structured readers of the agents' own session files.

1. `UsageReader` trait + **Claude reader**: parse
   `~/.claude/projects/<sanitized-workdir>/*.jsonl` `message.usage` records
   (input/output/cache-creation/cache-read tokens + model). Session→file mapping via
   workdir sanitization + spawn-time filtering. TDD against real JSONL fixtures.
2. **Codex reader**: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` — match on
   `session_meta.cwd`, read `token_count` events **including `rate_limits`**
   (`used_percent`, `window_minutes`, `resets_at`, `plan_type`) — exact quota for free.
3. Wire into the watchdog tick (detect → store → API → UI). New `usage_samples`
   migration. Keep keyword-proximity scraping as fallback for unknown agents.
4. **Account identity** per machine (`~/.claude.json` oauth email, `~/.codex/auth.json`),
   attached to sessions. Local-only; rollup metadata goes only to your own controller.
5. Cost = tokens × per-model rates (shipped rate table, user-overridable in config).
   Upgrade CLI USAGE column and UI badges from estimated to exact.

### Phase B — Visibility first; enforcement as a thin credibility proof

**Positioning principle (decided 2026-06-13):** the OSS adoption driver in this category
is *visibility*, not enforcement. ccusage has ~16k stars doing nothing but read-only,
single-machine, Claude-only, post-hoc cost display — people star "show me the number,"
not "stop my agent." So the project's identity is the **live, cross-machine,
cross-account, cross-agent burn-rate gauge** (B1+B2) — the thing ccusage can't do (it
doesn't run your sessions) and first parties won't (it arbitrages their rate limits).
A *minimal* enforcement (B3) earns its place only as the one-line proof that Pulpo is
infrastructure, not a dashboard: "ccusage shows you the bill; Pulpo can also pull the
plug, because it runs your sessions." Elaborate enforcement and thrash handling are
fleet-ops depth nobody stars you for — parked until real fleet usage asks.

**One-liner:** *See and control what every coding agent costs — across all your machines
and accounts. Self-hosted.* Lead with **see**; **control** is the half-sentence that
proves it's a breaker box. Explicit foil: live and fleet-wide, not post-hoc and
single-machine (ccusage).

**Launch set = B1 + B2 + minimal B3.** Target the June 23 Fable subscription cliff.

**B1 — Projection / burn-rate (SHIP — this is the identity).** Burn rate ($/hr, tokens/hr)
and time-to-wall, per-session and per-account. **Codex:** exact, extrapolated from the
`rate_limits` snapshot (`used_percent` → 100% within the window, bounded by `resets_at`).
**Claude:** honest estimation — always show $/hr and tokens/hr; show "% of weekly cap" and
time-to-wall **only if** the user configures `[plans]` allowances (Anthropic doesn't
publish the token allowance), labeled "estimated." `GET /api/v1/usage/projection`, a BURN
column on `pulpo list` / `pulpo usage`, web badges. Read-only, zero config risk. Pure
projection math in a `usage::projection` module → high-value unit tests.

**B2 — Pool attribution (SHIP — cheap, makes rollups honest, the launch talking point).**
Detect `-p`/`--print` in a session command → `usage_pool` = `subscription` (interactive
tmux, our default) vs `headless` (the separate monthly credit pool Anthropic confirmed
effective June 15, 2026). Projection rollups become pool-aware. Documents the structural
advantage: Pulpo's interactive-in-tmux sessions stay on the subscription pool, unlike
SDK-built orchestrators on `claude -p`.

**B3 — Minimal budget guardrail (SHIP — credibility proof, not a headline).** Per-session
and per-ink **cost cap only**: alert at 80% (one-shot, deduped via metadata flag), stop at
100% via the existing intervention path (new `InterventionCode::BudgetExceeded`). Config
on `WatchdogConfig`/`InkConfig` + a `--budget-cost` spawn flag (resolution: spawn > ink >
global). Frame honestly: on subscriptions this *allocates the shared pool* (a runaway
session can starve the rest until reset); on prepaid credits / API keys it protects real
dollars. NOT overdraft prevention on subscriptions.

**Parked (build on real fleet demand, not for launch):**
- Multi-dimension budgets (token caps, quota-% guard, per-day per-node rollup cap)
- Rate-limit thrash handling (pause + auto-resume after `resets_at`) — high complexity
  (new session state + scheduling), narrow benefit, undemoable
- Daily cost digest — cheap (cron + B1 endpoint + existing notifiers) and good for the
  "phone is the gauge" story, but retention not acquisition; post-launch only if cheap

### Phase C — Fleet rollups + placement (the unique part)

1. Controller-side rollups: cost/tokens today per node, per account, per ink.
2. Quota-aware placement: resurrect `--auto` (removed when scoring was naive — now it has
   real data): spawn on the node/account with most headroom; scheduler defers runs until
   a window resets.

Controller mode status (carried over): fleet reads, create/stop/resume, scheduled
dispatch, event push + command polling, per-node bearer tokens, persisted session index
are implemented. Distributed terminal attach stays out of scope. See
[Controller + Node Setup](/guides/controller-node-setup).

### Phase D — Reposition + distribution (gates the payoff)

- README/SPEC rewrite around the vision
- Landing page + demo video: phone → spawn on remote node → budget cap intervenes
  overnight → wake to a PR and an exact cost number
- PR to `andyrewlee/awesome-agent-orchestrators` — submitted 2026-06-12 (PR #61)
- Show HN after Phase B ships: "self-hosted fleet dashboard for coding-agent token burn
  across all your machines and accounts". Target the week of **June 23, 2026** — the day
  Fable 5 leaves Pro/Max plan limits and requires prepaid per-token usage credits
  ($10/$50 per MTok, ~2× Opus burn). That is the moment the audience starts paying
  per-token and goes looking for a meter.
- ~~Verify the June 15 headless billing split~~ — confirmed by Anthropic (2026-06-13).
  Pool attribution is now Phase B item 2; the "interactive-in-tmux stays on your
  subscription pool" advantage goes in the launch messaging.
- Homebrew-core once ≥75 stars

**Sequencing:** R + A start now in parallel. B needs A. C needs B + controller work.
D's launch moment is after B.

Also still planned: **agent completion callbacks** (`PULPO_CALLBACK_URL` env var; Claude
Code hooks can call it) — replaces the 29 waiting-pattern regexes with a reliable signal
and powers fast "agent blocked on permission prompt" push alerts. Babysitting wastes
wall-clock and tokens; this serves the vision and stays.

## Shipped (reference)

Core infrastructure:
- `pulpod` daemon + REST API + embedded web UI (single binary)
- `pulpo` CLI with attach, spawn, resume, stop, logs, schedule, ink, secret
- SQLite-backed session persistence with full lifecycle state machine
  (`creating`, `active`, `idle`, `ready`, `stopped`, `lost`; resume from `lost`/`ready`)
- Watchdog: memory pressure intervention, idle detection, ready TTL cleanup,
  error/failure detection, tmux auto-adopt
- Command-agnostic sessions (any CLI tool, any command)
- Inks: reusable session blueprints (command, description, secrets, runtime defaults)
- Multi-node: Tailscale peer discovery, manual peers, fleet summary, controller mode
  (see Phase C status above)
- SSE event stream, webhook notifications, Web Push, PWA
- Secret store: encrypted-at-rest env vars injected into sessions
- Per-session idle threshold, configurable waiting patterns (29 built-in)
- Scheduling: DB-backed cron schedules (local timezone), CRUD API + CLI, 60s scheduler
- Observability: PR/branch detection, git branch/commit/diff tracking, rate-limit
  detection, token/cost scraping (superseded by Phase A readers), enriched notifications
- Homebrew tap distribution, CLI auto-start daemon, node name resolution

Shipped but scheduled for removal under Track R: Docker runtime, worktrees web-UI page,
Tauri mobile builds, MCP server, Discord bot, voice experiments. (Ocean UI stays, frozen.)

## Parked

Revisit only on real demand:

- Batch manifests (`pulpo run manifest.yml`) — after quota-aware scheduling matures
- Configurable output matchers (user regex → action rules)
- Compliance & governance (audit trail, session ownership, resource policies) — if team
  adoption materializes
- Multi-user auth, Kubernetes backend, cloud VM backend
- Agent-to-agent communication — orchestration frameworks' job, never Pulpo's

## Removed

- ~~mDNS + seed-based discovery~~ (v0.0.41) — Tailscale + manual peers cover real usage
- ~~Provider-specific features, guard rails, culture system~~ — agents handle these
- ~~Per-peer session tabs, fleet click-through, `target_node` on schedules,
  naive `--auto`~~ — replaced by controller mode (placement returns in Phase C with
  real quota data)

## Success Criteria

Pulpo is succeeding if:

- You know exactly what every agent session cost — before you check any vendor dashboard
- You see one gauge for all machines, accounts, and agents, from your phone
- The watchdog stops a runaway session before it burns your weekly quota
- A scheduled overnight run lands on the account with headroom, or waits for the reset
- An agent blocked on a permission prompt pings your phone within seconds
- Sessions survive reboots; you wake up to PRs and an exact cost number, not crashed
  terminals
- Your code and your usage data never leave your infrastructure

## Architectural Principles

- Meter and breaker box, not orchestrator: measure, budget, place — don't wrap agent UX
- Command-agnostic: runs any agent; structured usage readers where available
  (Claude, Codex), output-scraping fallback everywhere else
- Sovereign by architecture: self-hosted, no vendor relay, local-only account data
- Single-node excellence first; fleet via controller promotion
- Mobile-first PWA: the phone is the primary gauge
- Explicit failure semantics: every intervention is observable and auditable
- Zero-config local start, progressive operational depth
