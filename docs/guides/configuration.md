# Configuration Guide

Default config path:

- `~/.pulpo/config.toml`

Minimal example:

```toml
[node]
name = "mac-mini"

[inks.reviewer]
provider = "claude"
unrestricted = false
instructions = "You are a senior reviewer focused on correctness and security."
```

## Important sections

- `[node]`: node identity, bind mode, discovery settings (`tag`, `seed`, `discovery_interval_secs`)
- `[auth]`: token for `public` bind mode (auto-generated, not needed for `local`/`tailscale`/`container`)
- `[watchdog]`: memory/idle intervention policy
- `[inks.*]`: reusable provider/guard configurations
- `[peers]`: manual peer entries
- `[knowledge]`: git-backed knowledge repo for cross-session learning

## Knowledge

Pulpo automatically extracts learnings from completed sessions and stores them in a local git repo at `<data_dir>/knowledge/`. Configure a remote to sync across nodes:

```toml
[knowledge]
remote = "git@github.com:yourorg/pulpo-knowledge.git"
inject = true  # inject context into new sessions (default: true)
```

Knowledge is injected into new sessions as a compact summary. Agents are instructed to write discoveries back to the repo. Use `pulpo knowledge --push` or the web UI to manually push to the remote.

For full field-level details, see [Config Reference](/reference/config).
