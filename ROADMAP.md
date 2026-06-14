# Pulpo Roadmap

Strategic direction for Pulpo: the self-hosted meter and breaker box for coding agents.

## Mission

Pulpo runs coding agents as background workers on your machines, **measures exactly what
every session costs** — across all your agents, accounts, and machines — **monitors and
alerts** on cost/quota/waste, and **optimizes the things it controls** (kills waste, runs
work on the cheapest pool with headroom, right-sizes defaults) so you get the maximum out
of your subscriptions without ever blowing a limit.

Scope boundary: Pulpo optimizes the *operation* of agents (when/whether/where a session
runs, what it launches with, when to stop it, which pool it draws from) — never the
*inference path* (no prompt caching, per-request routing, or context trimming; that's the
agent's job, not ours). It can't make a unit of work cheaper; it makes sure you don't pay
for waste and that you use capacity you've already bought.

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
model-price increase, and it is **not tied to any one model** — it's structural to running
fleets of agents on metered subscriptions and API keys. This is the gap Pulpo fills.

**Model volatility is the case for being model-agnostic.** Models launch, get restricted,
reprice, and get pulled — Fable 5 was withdrawn worldwide in June 2026, months after
launch. A cost-and-control layer welded to one model or vendor inherits that whiplash;
Pulpo doesn't. It meters and governs whatever you're actually allowed to run today
(Claude Opus/Sonnet/Haiku, Codex, and any future CLI agent) via structured readers with an
output-scraping fallback, and a built-in rate table that is **user-overridable in config**
so a new or repriced model never needs a code change. "Don't bet your tooling on one
model" is itself a positioning line.

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
5. Cost = tokens × per-model rates. **Shipped:** a built-in rate table (Opus/Sonnet/Haiku;
   a now-inert Fable row is retained only so historical sessions priced before the
   worldwide withdrawal still resolve), unknown models emit tokens without a misleading
   cost. **Model-agnostic follow-up (next):** a `[rates.<model>]` config section so
   operators add or reprice models without a code change — the concrete embodiment of
   "don't depend on a built-in model list." Upgrade CLI USAGE column and UI badges from
   estimated to exact.

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

**Launch set = B1 + B2 + minimal B3.** Ship when the surface is ready — the launch is no
longer pinned to a model-specific date (the Fable cliff is moot; Fable was pulled). The
durable hook is evergreen: agent cost/quota burn across machines and accounts.

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

### Phase M — Monitoring, alerting & operational optimization (a first-class pillar)

The measurement (B1/B2) and the blunt breaker (B3 stop-at-budget) are the floor. This
pillar turns the signals into **real notifications** and into **operational optimizations
Pulpo controls** — never the inference path. Everything here is alert-first and
non-destructive by default; any auto-action (stop/pause/defer) is opt-in config.

**M1 — Make alerts real (DONE).** `UsageAlert` event on the bus, delivered via SSE +
in-app toast; emitted on the budget 80% crossing (deduped). External-channel delivery is
folded into the event-forwarding backbone below.

### Event-forwarding backbone (the monitoring system) — finalized 2026-06-13

Pulpo becomes a universal event/control plane: it forwards **alerts and important events**
to wherever you run observability. Model-agnostic and sovereign (data goes to *your*
collector, not a vendor relay). Decisions locked:

- **Canonical event envelope + taxonomy/severity.** One header (`event_id` idempotency key,
  `schema_version`, `type`, `severity`, `occurred_at`, `node`, `session_id?`, `payload`).
  Types: `lifecycle` (ready/stopped/lost/error/rate-limited), `intervention` (memory/idle/
  budget stop), `usage_alert` (budget/burn/quota/rate-limit), `fleet` (node/peer health).
  `severity` (info/warn/critical) is the universal filter knob.
- **`EventSink` trait + one shared dispatcher** (owns bus subscription, filtering,
  serialization, retries) replacing the per-notifier loops.
- **Durable outbox (decided).** Persist events to a SQLite `events` table; deliver with
  retry + **exponential backoff**; mark delivered; survive restarts. Generalizes
  `intervention_events`. This is what makes it a monitoring backbone you can rely on.
- **Universal webhooks (the headline).** `[[webhooks]]` — multiple endpoints, each with a
  type/`min_severity` filter, **HMAC-signed payloads**, idempotency key (at-least-once,
  dedup on the receiver), per-endpoint backoff. Web-push stays as a sink (phone alerts).
- **Discord descoped** (owner's call, 2026-06-13 — "always a vanity example"). Remove the
  Discord webhook notifier + `[notifications.discord]` config + its config-API surface;
  tolerate a leftover `[notifications.discord]` section so old configs still boot.
- **`/metrics` Prometheus endpoint (decided), toggleable, off by default.** Pull-based,
  stateless (active sessions by status, $/hr, cost today, quota %, budget-breach +
  intervention counters — computed on scrape, nothing stored). Gated by bind mode.
  Push (webhooks) for discrete events; pull (`/metrics`) for continuous dashboard state.
- **Scope boundary:** Pulpo emits events + exposes metrics; it is **not** a TSDB or log
  store — forward to the user's stack (collector, Slack webhook, ntfy, Datadog, …).
- **Topology:** standalone node forwards to its own sinks; in controller mode managed nodes
  forward important events to the controller, which fans out to centrally-configured sinks
  and exposes a fleet event feed. Same event model, two deployment shapes.

**Webhook message contract (locked 2026-06-13).** One canonical envelope for *every*
event — session state changes (idle/active/ready/stopped/lost) are first-class `lifecycle`
events alongside interventions, usage alerts, and fleet events.

```
POST <endpoint-url>
  Content-Type: application/json
  User-Agent: pulpo/<version>
  X-Pulpo-Event: lifecycle.idle            # "<type>.<subtype>" for quick routing
  X-Pulpo-Event-Id: <uuid>                 # stable across retries (idempotency key)
  X-Pulpo-Signature: sha256=<hex hmac>     # HMAC-SHA256(raw body, endpoint secret)

{
  "schema_version": 1,
  "event_id": "<uuid>",
  "type": "lifecycle",        // lifecycle | intervention | usage_alert | fleet
  "subtype": "idle",          // the specific event within the type
  "severity": "warn",         // info | warn | critical
  "occurred_at": "2026-06-13T12:00:00Z",
  "node": "mac-mini",
  "session": {                // present for session-scoped events
    "id": "...", "name": "fix-auth", "status": "idle", "ink": "coder",
    "git_branch": "...", "pr_url": null,
    "cost_usd": 2.5, "total_tokens": 1234000, "pool": "subscription"
  },
  "payload": { }              // type-specific extras (budget_usd, quota_used_percent,
                              // intervention_reason, ...)
}
```

Event catalogue (`type.subtype` → severity):
- `lifecycle.{creating,active,idle,ready,stopped,error,rate_limited}` (info/warn),
  `lifecycle.lost` (critical)
- `intervention.{memory_pressure,idle_timeout,budget_exceeded,user_stop}` (warn/critical),
  `payload.intervention_reason`
- `usage_alert.{budget_threshold,burn_ceiling,quota_threshold,rate_limit}` (warn/critical),
  `payload.{cost_usd,budget_usd,quota_used_percent}`
- `fleet.{node_up,node_down,peer_unreachable}` (warn/critical)

Per-endpoint filter: `events = ["lifecycle.idle", "usage_alert.*", "intervention.*"]`
(glob on `type.subtype`) plus `min_severity`. Delivery is at-least-once from the outbox
with exponential backoff; receivers dedupe on `event_id` and verify `X-Pulpo-Signature`.
The `X-Pulpo-Event` header lets a receiver route/drop without parsing the body.

Build order (non-breaking; existing `[notifications.webhooks]` maps onto the new model):
**0)** descope Discord ✅ (#56) · **1)** canonical event model + `EventSink` dispatcher ✅
(#57; session lifecycle + usage alerts both flow to webhooks) · **2)** durable SQLite
outbox + retry/backoff + HMAC + idempotency ✅ (#58) · **3)** universal `[[webhooks]]`
config + `type.subtype`/severity routing ✅ (#60) · **4)** `/metrics` toggle ✅ (#59) ·
**5)** controller aggregation + fleet event feed ✅ **already satisfied** by existing
machinery: managed nodes' event-push loop forwards *all* events to the controller, the
controller re-broadcasts them onto its bus (`event_push.rs`), and the step-1 dispatcher
fans them out to the controller's `[[webhooks]]` (durable outbox) and its SSE feed. The
controller's SSE `/api/v1/events` is the live fleet feed.

**Backbone status: COMPLETE.** Optional additive polish, parked (not core; build on demand):
a *persistent/queryable* fleet event log (`GET /events?since=` history vs the live SSE);
fleet-wide `/metrics` aggregated from the controller session index (today `/metrics` is
per-node). Pre-existing M2 (burn-velocity governor, alert-only) remains the open optimizer.

**M2 — Burn-velocity governor (the marquee optimizer).** A configurable `$/hr` (and/or
tokens/hr) ceiling on the watchdog: crossing it **alerts** by default; **opt-in** to pause
or stop. Catches the catastrophic runaway/loop ("$90 at 2am") that flat budgets miss
because they only trip at the total. Smart mode (N× a session's own median) is a follow-up.

**M3 — Waste elimination.** Rate-limit thrash → pause until `resets_at` instead of
retry-burn (was parked B4); stuck/idle-beyond-threshold reclamation. Direct recovery of
wall-clock and retry tokens.

**M4 — Cheaper-by-default policy (cheap, do anytime).** Ink fields for a recommended model
and effort default so routine jobs (nightly lint, triage) don't run on the most expensive
model; reserve the top tier for hard work. Pulpo templates the launch command — this is
policy, not per-request routing.

**M5 — Cheapest-pool-first placement (needs Phase C controller).** Spawn on the
subscription pool that still has headroom before spilling to paid API credits; defer
non-urgent runs until the quota window resets (exact `resets_at` for Codex, estimated for
Claude). The optimization only Pulpo can do because only Pulpo sees every pool's headroom.

**Config-overridable rates** (the model-agnostic follow-up from Phase A) belongs here too:
`[rates.<model>]` so cost/burn math never needs a code change when a model reprices or a
new one ships — directly serving the "monitor cost accurately for any model" goal.

Sequence: M1 → M2 (+ config rates) → M3/M4 anytime → M5 with the controller. M1+M2 are the
visible "Pulpo watches your spend and catches runaways" story.

### Phase D — Reposition + distribution (gates the payoff)

- README/SPEC rewrite around the vision
- Landing page + demo video: phone → spawn on remote node → budget cap intervenes
  overnight → wake to a PR and an exact cost number
- PR to `andyrewlee/awesome-agent-orchestrators` — submitted 2026-06-12 (PR #61)
- Show HN after Phase B ships: "self-hosted fleet dashboard for coding-agent token burn
  across all your machines and accounts." No longer timed to the Fable cliff (Fable was
  pulled worldwide). Time it to a durable hook instead — a fresh price/quota change on any
  major model, the headless-pool billing split, or just "we shipped." The angle that
  *gained* from Fable's removal: models get banned and pulled; your cost-control layer
  shouldn't depend on any one of them — Pulpo is model- and vendor-agnostic.
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
