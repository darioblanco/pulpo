# 0006. Exact metering plus a flat budget cap — nothing else on the enforcement surface

- **Status:** Accepted
- **Date:** 2026-09-13 (PR [#115](https://github.com/darioblanco/pulpo/pull/115))

## Context

The metering/enforcement surface had grown four features deep on top of exact
per-session usage: a burn-velocity governor (M2 — a configurable $/hr and tokens/hr
ceiling, alert by default with an opt-in pause/stop), a usage-projection module (B1 —
$/hr, tokens/hr, time-to-wall, and an estimated "% of weekly cap" for Claude fed by a
`[plans]` allowance config), pool attribution (B2 — classifying a session as
`subscription` vs `headless` by detecting `-p`/`--print`, which in turn justified
reading `auth_provider`/`auth_plan`/`auth_email` out of each agent's local credential
file), and an output-scraping keyword-proximity usage fallback for harnesses without a
structured reader. Each addition was individually defensible, but together they were a
second, weaker, estimation-heavy product surface sitting next to the one thing that was
actually exact: per-session usage read from the agent's own files.

## Decision

We will remove all four and keep only **exact per-session/per-repo usage** (from the
Claude Code, Codex, and pi structured readers) plus the **flat per-session/per-schedule
budget cap** (alert at 80%, hard stop at 100%, `InterventionCode::BudgetExceeded`) as
the entirety of Pulpo's cost-control surface. Concretely removed: `watchdog/burn.rs`
and `BurnAction`/`BurnConfig`/the three `burn_*` watchdog config keys/the
`usage_alert.burn_ceiling` event/`InterventionCode::BurnRate`; `usage/projection.rs`,
`GET /api/v1/usage/projection`, the `$/hr`/tokens-per-hour/time-to-cap columns, and the
`[plans]` config; `usage/pool.rs` (`detect_pool`), `AccountRollup`, and the
credential-file readers (`extract_claude_auth`/`extract_codex_auth`/
`extract_gemini_auth`) — `auth_info.rs` was trimmed to just
`agent_provider_for_command`, still needed to route a session to its usage reader; and
the keyword-proximity extractor in `watchdog/output_patterns.rs`
(`extract_agent_usage`, `KEYWORD_RULES`, `COST_KEYWORDS`).

## Consequences

- A harness without a structured usage reader (anything other than Claude Code, Codex,
  or pi) now shows **no usage** instead of a scraped guess — a stricter but more honest
  guarantee: every recorded cost is now exact by construction.
- `pulpod` never reads agent account credentials or identity off disk again — the
  removed pool attribution was its only consumer, along with a since-removed web "auth
  plan" badge. A metering tool has no business holding that data.
- No near-real-time runaway-*rate* detection remains — only the flat, total-cost cap.
  This is an accepted tradeoff: the cap still bounds a runaway session, just at the
  total rather than at the peak spend rate; a genuinely fast, expensive burst within
  budget is not separately flagged.
- A historical `intervention_code = 'burn_rate'` DB row (or any other now-unrecognized
  value) degrades to `None` on read instead of failing the query — the same tolerance
  already given to the retired `runtime = 'docker'` rows — so no migration was needed
  for the enum change.
- The project's one-liner changed with it: not "the live burn-rate gauge across your
  fleet," but "see, and cap, exactly what every coding agent costs."
