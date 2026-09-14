# 0008. Unknown config keys warn and are ignored

- **Status:** Accepted
- **Date:** 2026-09 (landing in parallel with this docs batch, as "batch C" —
  `feat/batch-c-config-unknown-keys`)

## Context

`config.toml` already tolerated a growing list of *specific, named* retired keys —
`[docker]`, `[controller]`, `[inks]`, `[peers]`, `[plans]`, `[metrics]`,
`[notifications.vapid]`, `watchdog.adopt_tmux`, the three retired `burn_*` watchdog
keys, `node.discovery_interval_secs` — each added case-by-case at the time its feature
was removed (see `ROADMAP.md` "Removed" and ADRs
[0002](0002-meter-and-breaker-box-positioning.md)/[0004](0004-one-plain-webhook-channel.md)/[0006](0006-exact-metering-and-flat-budget-cap-only.md)).
There was no *general* rule for a config key `pulpod` has never heard of at all — a
typo, a key meant for a newer version of `pulpod` running against an older binary, or a
key from a fork or local experiment. Historically this either had to be special-cased
per removal or would behave inconsistently (parse error vs. silent drop) depending on
where in the config the key appeared.

## Decision

We will generalize the existing case-by-case tolerance into a standing policy: any
unrecognized config key — top-level or nested inside a known table — logs a startup
warning naming the key, and is otherwise ignored. `pulpod` still boots with defaults
for the surrounding section rather than failing config load over it. This lands as its
own batch of work (parallel to this documentation consolidation) rather than being rolled
into any single removal PR, since it is a policy that applies going forward, not a
cleanup tied to one feature's removal.

## Consequences

- A typo in `config.toml` no longer prevents `pulpod` from starting — it fails soft and
  warns loud in the startup logs instead of refusing to boot.
- A config key meant for a newer `pulpod` (or an experimental/fork-only key) silently
  no-ops on an older binary instead of erroring, which makes running config and binary
  slightly out of lockstep safer during upgrades.
- The tradeoff is discoverability: a typo'd key the operator actually meant to set is
  easy to miss if startup logs aren't checked. This is mitigated by the warning being
  unconditional (always emitted, not gated behind a verbosity flag), but it is a real
  cost relative to a hard failure.
- Every previously-special-cased retired key listed in the Context section becomes a
  special case of this general rule going forward; new removals no longer need their
  own "tolerate this specific retired key" carve-out in the config loader.
