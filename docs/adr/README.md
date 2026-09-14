# Architecture Decision Records

An ADR is a short document that captures one significant architectural or product
decision: the context that forced it, what was decided, and the consequences accepted
along with it. Pulpo records decisions here — new ones going forward, and the
September 2026 decisions backfilled below — instead of letting the rationale live only
in commit messages, PR descriptions, or `ROADMAP.md` prose.

New ADRs follow the MADR-style template in [0000-template.md](0000-template.md):
Title, Status, Context, Decision, Consequences, Date. Number sequentially, never reuse
or renumber a number, and mark a superseded ADR's status accordingly (point at the ADR
that replaces it) rather than deleting it — an ADR is a historical record, not living
documentation.

## Index

| ADR | Title | Status | Date |
|-----|-------|--------|------|
| [0000](0000-template.md) | Template | — | — |
| [0001](0001-hook-driven-agent-state.md) | Hook-driven agent state instead of scrollback scraping | Accepted | 2026-09-10 |
| [0002](0002-meter-and-breaker-box-positioning.md) | The meter-and-breaker-box positioning, and what was cut to get there | Accepted | 2026-06 – 2026-09-11 |
| [0003](0003-scenario-tests-as-behavior-gate.md) | Scenario tests as the behavior gate; unit tests for logic; 98% coverage as a decay guard | Accepted | 2026-09-13 |
| [0004](0004-one-plain-webhook-channel.md) | One plain webhook channel | Accepted | 2026-09-13 |
| [0005](0005-config-file-is-source-of-truth.md) | The config file is the source of truth; no config-editing API | Accepted | 2026-09-13 |
| [0006](0006-exact-metering-and-flat-budget-cap-only.md) | Exact metering plus a flat budget cap — nothing else on the enforcement surface | Accepted | 2026-09-13 |
| [0007](0007-rust-stays-go-decision-deferred.md) | Rust stays for now; a Go rewrite is deferred until after a real dogfood test | Accepted | 2026-09-13 |
| [0008](0008-unknown-config-keys-warn-and-are-ignored.md) | Unknown config keys warn and are ignored | Implemented | 2026-09-14 |
| [0009](0009-five-state-session-model.md) | Five-state session model: starting / working / waiting / done / lost | Implemented | 2026-09-14 |

See also [ROADMAP.md](https://github.com/darioblanco/pulpo/blob/main/ROADMAP.md) for
the strategic narrative these decisions sit inside, and `AGENTS.md` → "record decisions
as ADRs in `docs/adr`" for when to add a new one.
