# Recovery Guide

Session states:

- `creating`
- `running`
- `completed`
- `dead`
- `stale`

## Common recovery path

```bash
pulpo list
pulpo resume <name>
pulpo logs <name> --follow
```

Use `resume` only for `stale` sessions (record exists, backend session gone).

If state is `dead`, start a new session with `spawn`.

## Interventions

Inspect intervention history:

```bash
pulpo interventions <name>
```

This helps distinguish watchdog action from provider/process failures.
