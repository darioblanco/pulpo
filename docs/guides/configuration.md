# Configuration Guide

Default config path:

- `~/.pulpo/config.toml`

Minimal example:

```toml
[node]
name = "mac-mini"

[personas.reviewer]
provider = "claude"
model = "sonnet"
guard_preset = "strict"
system_prompt = "You are a senior reviewer focused on correctness and security."
```

## Important sections

- `[node]`: node identity, bind mode, network settings
- `[auth]`: token and bind behavior for non-localhost access
- `[watchdog]`: memory/idle intervention policy
- `[discovery]`: `mdns`, `tailscale`, or `seed`
- `[personas.*]`: reusable provider/model/guard presets
- `[peers]`: manual peer entries

For full field-level details, see [Config Reference](/reference/config).
