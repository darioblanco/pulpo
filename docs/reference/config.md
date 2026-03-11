# Config Reference

Default file:

- `~/.pulpo/config.toml`

Top-level sections:

- `[node]` — name, port, data_dir, bind mode, tag, seed, discovery_interval_secs
- `[auth]` — token (only used with `bind = "public"`)
- `[watchdog]` — memory/idle intervention policy
- `[peers]` — manual peer entries
- `[inks.<name>]` — reusable spawn defaults
- `[notifications.discord]` — webhook notifications
- `[culture]` — culture extraction and sync

Notes:

- `bind` in `[node]` determines both network exposure and discovery method.
- `[auth]` is only relevant for `bind = "public"`. Other modes skip auth.
- `inks` are universal named roles. Each ink can set: `description`, `provider`, `model`, `mode`, `unrestricted`, and `instructions`. Instructions are passed as system prompt for providers that support it (Claude), or prepended to the prompt for others (Codex, Gemini, OpenCode).
- `[culture]` configures the git-backed culture repo. Set `remote` to a git URL for cross-node sync. Set `inject = false` to disable automatic context injection at session spawn. Culture is stored at `<data_dir>/culture/` as JSON files committed to a local git repo.
- Manual `peers` always coexist with dynamic discovery.

Source of truth for full semantics:

- [SPEC.md](https://github.com/darioblanco/pulpo/blob/main/SPEC.md)
