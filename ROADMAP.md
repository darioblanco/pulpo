# Pulpo Roadmap

Strategic direction for Pulpo: the self-hosted meter and breaker box for coding agents.
For the full narrative behind any individual decision below, see
[Architecture Decision Records](docs/adr/README.md) — this file is the current-state
summary; the ADRs are the durable, dated record of *why*.

## Mission

Pulpo runs any coding agent — Claude Code, Codex, pi, or any other terminal command —
as a durable session on a machine you own. It learns a session's real state from the
harness's own hooks (Claude Code, Codex, pi) instead of scraping terminal output,
meters exactly what it costs from the agent's own session files, rolls that up per
repo, caps spend per session or schedule and stops at the cap, and forwards what
happened over a webhook — sessions survive reboots and resume the same conversation,
and scheduled runs plus per-session git worktrees let you run agents unattended and in
parallel without babysitting them. Each `pulpod` is single-node, standalone
infrastructure — it governs the sessions on the machine it runs on and nothing else,
with no cross-node control plane; for a view across machines, point every node's
webhooks at a collector you already run. It is not an agent framework, a prompt tool,
or a terminal-orchestration UX — agents already handle their own interactive worktree
UX, sandboxing, and guardrails better than any wrapper can. `tmux` is plumbing, not
product: the universal way to run any agent as an observable, killable, attributable
process without modifying it. Scope boundary: Pulpo optimizes the *operation* of agents
on the node you run it on (whether a session runs, what it launches with, when to stop
it) — never the *inference path* (prompt caching, per-request routing, context
trimming; that's the agent's job). It can't make a unit of work cheaper; it makes sure
you don't pay for waste and that you use capacity you've already bought.

## The Bet (June 2026)

The early-2026 shakeout settled the orchestration question: Terragon dead, Vibe Kanban
dead at 27k stars, Crystal deprecated, Omnara pivoted. First parties absorbed the
value — Claude Code ships native worktrees, agent teams, and Remote Control (a
built-in per-machine session daemon with mobile push); Codex ships a desktop command
center. Wrapping tmux/worktrees/guardrails is a losing race (see ADR
[0002](docs/adr/0002-meter-and-breaker-box-positioning.md) for the full repositioning
and everything cut because of it).

What nobody ships — and what first parties are **incentive-blocked** from ever
shipping:

1. **Cross-account, cross-agent cost telemetry, exactly measured.** A vendor's
   `/usage` is one account, one machine, one vendor, checked after the fact; no vendor
   will ever aggregate spend across *your* accounts, since that means helping you
   arbitrage their own rate limits. (ccusage, ~16k stars, proves the demand for the
   read-only version of this.)
2. **Enforcement.** A budget cap that auto-stops before the wall, not a post-hoc
   invoice — only the thing running the session can prevent an overspend. Shipped as
   the flat per-session/per-schedule cap (see "Shipped"); richer enforcement (thrash
   handling, multi-dimension budgets) stays parked until real demand asks for it.
3. **Quota-aware placement** — "spawn this on whichever node/account has the most
   headroom." This needs a cross-node control plane, which Pulpo built and then
   deliberately removed (2026-07) once it was clear a proprietary fleet controller was
   the losing race, not a differentiator. Parked, not planned.

The #1 community complaint about parallel agents is that they are a *quota
multiplier* (Max users burning 20% of a weekly allowance in 2 hours) — structural to
running fleets of agents on metered subscriptions, not tied to any one model. **Model
volatility is the case for being model-agnostic**: models launch, get restricted,
reprice, and get pulled (Fable 5 was withdrawn worldwide in June 2026, months after
launch). A cost-and-control layer welded to one model or vendor inherits that
whiplash; Pulpo meters and governs whatever you're actually allowed to run today via
structured readers (Claude Code, Codex, pi — an unsupported harness simply shows no
usage) plus a built-in rate table that's user-overridable in config
(`[rates.<model>]`), so a new or repriced model never needs a code change.

Sovereignty is the supporting argument: the daemon reads usage from local files and
never ships it anywhere except your own collector — exactly the data you'd least want
in a third-party relay.

## Shipped

Core infrastructure:
- `pulpod` daemon + REST API + embedded web UI (single binary); `pulpo` CLI with
  attach, spawn, resume, stop, logs, schedule, handoff
- SQLite-backed session persistence with a full lifecycle state machine (`creating`,
  `active`, `idle`, `ready`, `stopped`, `lost`; resume from `lost`/`ready`/`stopped`) —
  a five-state replacement (`starting`/`working`/`waiting`/`done`/`lost`) is accepted
  but not yet implemented, see ADR [0009](docs/adr/0009-five-state-session-model.md)
- Harness adapters: hook-driven lifecycle events for Claude Code, Codex, and pi,
  replacing scrollback-only detection with real resume and a `needs input (<reason>)`
  status label — see ADR [0001](docs/adr/0001-hook-driven-agent-state.md) and
  [docs/architecture/harness-adapters.md](docs/architecture/harness-adapters.md)
- Watchdog: idle detection (runs before budget enforcement each tick), error/failure
  detection, a flat per-session/per-schedule budget breaker (alert 80%, stop 100%,
  `InterventionCode::BudgetExceeded`) — see ADR
  [0006](docs/adr/0006-exact-metering-and-flat-budget-cap-only.md)
- Exact usage telemetry: structured readers for Claude Code, Codex, and pi (tokens,
  cost, cache, Codex quota), per-repo/worktree and cross-agent rollups
  (`pulpo usage`, `pulpo usage --scan`), `[rates.<model>]` config overrides — no
  output-scraping fallback
- Command-agnostic sessions (any CLI tool, any command); per-session git worktrees for
  isolated parallel work on one repo
- DB-backed cron schedules (local timezone), CRUD API + CLI, configurable scheduler
  tick (`[scheduler] tick_secs`, default 60s)
- One plain webhook notification channel (`[[webhooks]]`, bounded delivery, URLs
  redacted from logs) plus the SSE event stream — see ADR
  [0004](docs/adr/0004-one-plain-webhook-channel.md)
- The config file (`~/.pulpo/config.toml`) is the sole source of truth — no
  config-editing API, read-only `GET` views only — see ADR
  [0005](docs/adr/0005-config-file-is-source-of-truth.md); unknown config keys warn
  and are ignored — see ADR
  [0008](docs/adr/0008-unknown-config-keys-warn-and-are-ignored.md)
- Two-tier test strategy: unit tests for logic (TDD), scenario tests
  (`crates/pulpo-e2e`, a real daemon + real tmux + a fake harness) as the behavior
  gate, 98% line coverage as a decay guard — see ADR
  [0003](docs/adr/0003-scenario-tests-as-behavior-gate.md)
- Observability: PR/branch detection, git branch/commit/diff tracking, rate-limit
  detection
- Homebrew tap distribution, CLI auto-start daemon, Tailscale transport
  (`bind = "tailscale"`) for private remote access

Track R (removals, all shipped as their own PRs): Docker session runtime, worktrees
web-UI page, Tauri mobile builds, MCP server, Discord bot, voice experiments — see
"Removed" below for these and every later removal.

## Parked

Revisit only on real demand:

- Multi-dimension budgets (token caps, quota-% guard, per-day per-node rollup cap) and
  rate-limit thrash handling (pause + auto-resume after `resets_at`) — high
  complexity, narrow benefit, undemoable
- Batch manifests (`pulpo run manifest.yml`) — after quota-aware scheduling matures
- Configurable output matchers (user regex → action rules)
- Compliance & governance (audit trail, session ownership, resource policies) — if
  team adoption materializes
- Multi-user auth, Kubernetes backend, cloud VM backend
- Cross-node quota-aware placement — needs a control plane, which was tried and
  removed (see "Removed"); would be rebuilt from zero on real demand, not resurrected
- Agent-to-agent communication — orchestration frameworks' job, never Pulpo's
- **Decoupled dashboard as a reference webhook consumer.** Feasible and on-brand: the
  canonical event envelope + universal `[[webhooks]]` + REST/SSE are exactly the
  substrate a standalone dashboard would consume — a richer version of
  `contrib/examples/webhook-discord`. Keep the embedded single-binary UI as the
  zero-setup default; the decoupled one would be an *example of the pattern*, built on
  demand, not for launch.
- A Go rewrite of `pulpod`/`pulpo` — deferred until after a real dogfood test, see ADR
  [0007](docs/adr/0007-rust-stays-go-decision-deferred.md)

## Removed

Newest first. Every September 2026 entry landed the same week as harness adapters (PR
#97) and is independent of it, except `adopt_tmux` (see below) and the entries dated by
PR number (#111-#118), which landed later the same month, independently of each other.
See ADR [0002](docs/adr/0002-meter-and-breaker-box-positioning.md) for the positioning
bet that motivated the June–July and September cuts together, ADR
[0004](docs/adr/0004-one-plain-webhook-channel.md) for the notifications collapse, and
ADR [0006](docs/adr/0006-exact-metering-and-flat-budget-cap-only.md) for the
metering/enforcement simplification.

- ~~Watchdog config hot-reload channel~~ (September 2026, PR #118) — `PUT
  /api/v1/watchdog` was the only sender on the watchdog's runtime-config `watch`
  channel; once the config-editing API was removed (#117, ADR 0005) the channel had no
  writer left. The watchdog now reads `WatchdogRuntimeConfig` once at startup;
  changing `[watchdog]` takes effect on the next `pulpod` restart. The same PR swapped
  the per-tick order so `check_idle_sessions` runs before `budget::enforce_budgets` (a
  session that crosses its budget mid-tick is caught that tick, not one tick late),
  gave hook-ended sessions their recorded exit code, and bounded webhook delivery
  (connect/total timeouts, 16 concurrent in-flight cap, URLs stripped from failure
  logs).
- ~~Config-editing API, the settings tabbar UI, and the PWA service worker~~
  (September 2026, PR #117) — see ADR
  [0005](docs/adr/0005-config-file-is-source-of-truth.md).
- ~~Burn-velocity governor (M2), usage projection (B1), pool attribution (B2), and the
  output-scraping usage fallback~~ (September 2026, PR #115) — see ADR
  [0006](docs/adr/0006-exact-metering-and-flat-budget-cap-only.md).
- ~~Watchdog memory-pressure intervention + Ready-session TTL auto-purge~~ (September
  2026, PR #113) — `watchdog/memory.rs` probing never actually fired for the
  unattended agent loop the watchdog targets; idle and budget already cover a runaway
  session. `InterventionCode::MemoryPressure` stays on the wire (never emitted by new
  code) for historical rows. `NodeInfo.memory_mb` (general system info) was kept.
- ~~Web Push, `/api/v1/metrics`, and the durable webhook outbox~~ (September 2026, PR
  #111) — see ADR [0004](docs/adr/0004-one-plain-webhook-channel.md).
- ~~Peer registry, peer health probing, Tailscale peer discovery, `--node` CLI
  routing~~ (September 2026, PR #104) — only ever produced a read-only list of other
  nodes' sessions with no way to act on them. `bind = "tailscale"` and
  `tailscale serve` stay as the remote-access transport, reached with
  `pulpo --url <host:port>`. See ADR
  [0002](docs/adr/0002-meter-and-breaker-box-positioning.md).
- ~~Secrets store~~ (September 2026, PR #103) — every supported agent reads its own
  credentials from its own config; the owner's database had zero secrets stored after
  five months in production.
- ~~Watchdog auto-adoption of external tmux sessions (`adopt_tmux`)~~ (September 2026,
  PR #102) — since harness adapters (#97, ADR 0001) a `pulpo spawn`-started session
  gets real hooks, a preset session id, and resume; an adopted session got none of
  that, and adoption's interaction with the tmux `$N` id space was implicated in
  zombie sessions seen after a reboot.
- ~~Containerized `pulpod` deployment~~ (September 2026, PR #101) — a containerized
  `pulpod` can't see the agents' own session files that exact usage metering depends
  on. Distinct from the Docker *session runtime* removed earlier (v0.1.0, PR #53).
- ~~Windows build target~~ (September 2026, PR #99) — sessions run in `tmux`, which
  native Windows doesn't have. WSL2 users run the Linux binary and are unaffected.
- ~~Ocean gamification UI~~ (September 2026) — the canvas-based octopus/session
  visualization was frozen since 2026-06-12 and is now extracted to a separate
  `pulpo-ocean` repo, with its git history intact.
- ~~Controller/node control plane~~ (July 2026, PR #84) — `[controller]` config,
  controller/node roles, the fleet/enrollment/event-push/node-commands API surface.
  Cross-node orchestration was a dead product lane (see "The Bet"); direct
  `pulpo --url <host:port>` access over Tailscale plus shared `[[webhooks]]` cover
  real usage.
- ~~Inks (`[inks.<name>]` preset registry)~~ (July 2026, PR #91) — the community
  standardized agent-side config (AGENTS.md, skills); command is set directly per
  session/schedule, and the recurring cost budget moved onto the schedule itself
  (`pulpo schedule add --budget-cost <USD>`). `Session.ink`/`Schedule.ink` remain on
  the wire for historical rows only.
- ~~mDNS + seed-based discovery~~ (v0.0.41) — Tailscale + manual peers covered real
  usage at the time; manual peers were themselves removed later (see above).
- ~~Provider-specific features, guard rails, culture system~~ — agents handle these
  natively now.
- ~~Per-peer session tabs, fleet click-through, `target_node` on schedules, naive
  `--auto`~~ — replaced by controller mode, itself removed (see above); cross-node
  placement is not coming back.

## Success Criteria

Pulpo is succeeding if:

- You know exactly what every agent session cost, per session and rolled up per repo —
  before you check any vendor dashboard
- You can reach any node's dashboard from your phone over Tailscale, with zero setup
- The watchdog stops a runaway session at its budget cap before it burns your weekly
  quota
- A scheduled overnight run alerts you at 80% of its budget and stops at 100%, instead
  of a surprise on the invoice
- An agent blocked on a permission prompt shows `needs input (<reason>)` and fires a
  webhook you can route to your phone
- Sessions survive reboots; you wake up to PRs and an exact cost number, not crashed
  terminals
- Your code and your usage data never leave your infrastructure

## Decisions

Significant architectural and product decisions are recorded as
[Architecture Decision Records](docs/adr/README.md) under `docs/adr/` — see the index
there for the full list and template. New decisions of similar weight (a removal, a
positioning shift, a test-strategy or config-contract change) should get a new ADR
rather than only a mention here.
