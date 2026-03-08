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

For full field-level details, see [Config Reference](/reference/config).
