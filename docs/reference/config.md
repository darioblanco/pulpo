# Config Reference

Default file:

- `~/.pulpo/config.toml`

Top-level sections:

- `[node]`
- `[auth]`
- `[watchdog]`
- `[discovery]`
- `[peers]`
- `[personas.<name>]`
- `[notifications.discord]`

Notes:

- `watchdog` controls intervention behavior.
- `discovery` controls how nodes find each other.
- `personas` provide reusable spawn defaults.
- manual `peers` always coexist with dynamic discovery.

Source of truth for full semantics:

- [SPEC.md](../../SPEC.md)
