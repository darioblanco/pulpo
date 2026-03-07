# Config Reference

Default file:

- `~/.pulpo/config.toml`

Top-level sections:

- `[node]` — name, port, data_dir, bind mode, tag, seed, discovery_interval_secs
- `[auth]` — token (only used with `bind = "public"`)
- `[watchdog]` — memory/idle intervention policy
- `[peers]` — manual peer entries
- `[personas.<name>]` — reusable spawn defaults
- `[notifications.discord]` — webhook notifications

Notes:

- `bind` in `[node]` determines both network exposure and discovery method.
- `[auth]` is only relevant for `bind = "public"`. Other modes skip auth.
- `personas` provide reusable spawn defaults.
- Manual `peers` always coexist with dynamic discovery.

Source of truth for full semantics:

- [SPEC.md](https://github.com/darioblanco/pulpo/blob/main/SPEC.md)
