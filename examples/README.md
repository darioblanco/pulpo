# Examples

This folder contains runnable examples for common Pulpo workflows.

## Layout

- `config/`: sample `config.toml` files
- `api/`: `curl` scripts for the REST API
- `cli/`: `pulpo` command examples
- `discord-bot/`: quick setup for the contrib Discord bot

## Quick Start

```bash
# From repo root
make dev

# In another terminal, run an API example
bash examples/api/health.sh
```

Most scripts use these environment variables:

- `PULPOD_URL` (default: `http://localhost:7433`)
- `PULPOD_TOKEN` (optional for `local` bind mode, required for `public` bind mode)

Example:

```bash
PULPOD_URL=http://mac-mini:7433 PULPOD_TOKEN=your-token \
  bash examples/api/schedules.sh
```
