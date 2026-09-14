# 0008. Unknown config keys warn and are ignored

- **Status:** Implemented (PR [#123](https://github.com/darioblanco/pulpo/pull/123))
- **Date:** 2026-09-14

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
warning naming the key (`config: unknown key '<path>' ignored`), and is otherwise
ignored. `pulpod` still boots with defaults for the surrounding section rather than
failing config load over it. This lands as its own batch of work (parallel to the
documentation consolidation batch) rather than being rolled into any single removal
PR, since it is a policy that applies going forward, not a cleanup tied to one
feature's removal.

**Implemented as:** parse the file into a `toml::Value`, build a schema tree by
serializing a fully-populated example `Config` (including one representative
`[rates.<model>]` entry and one `[[webhooks]]` entry so their nested field sets are
known too), walk the parsed value against that tree collecting/stripping any key
absent from it, then deserialize the cleaned value into `Config`. `[rates.<model>]`'s
model-name level is free-form (any model name is accepted); each entry's own fields
are still validated against the representative sub-schema, as are
`[[webhooks]]`/`[[notifications.webhooks]]` array elements. `#[serde(deny_unknown_fields)]`
stays on every config struct as a safety net — stripping already guarantees no
unknown key reaches deserialization, so a real bug in the walker fails loudly
(CI-visible) instead of silently masking itself. A recognized key with an invalid
value (e.g. `bind = "container"`) still fails loudly, since that's a mistake in a
real setting, not an unrecognized one. This single generic rule **replaced** the
~15 hand-maintained named "retired key" special cases (`[docker]`, `[controller]`,
`[inks]`, `[peers]`, `discovery_interval_secs`, `node.tag`, `watchdog.adopt_tmux`,
`[metrics]`, `[notifications.vapid]`, per-webhook `secret`, `[plans]`,
`watchdog.burn_*`, `memory_threshold`, `breach_count`, `ready_ttl_secs`) that earlier
removals (ADRs [0002](0002-meter-and-breaker-box-positioning.md),
[0004](0004-one-plain-webhook-channel.md),
[0006](0006-exact-metering-and-flat-budget-cap-only.md)) had added one at a time —
those retired keys now fall under this same general rule rather than their own
carve-outs, so the "Retired keys" tables in `docs/reference/config.md` and
`docs/guides/configuration.md` were deleted and replaced with a short paragraph
describing the generic rule.

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
