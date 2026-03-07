# Docker Setup (Iteration Scaffold)

This folder provides two images:

- `pulpo-base`: minimal runtime image for `pulpod` + `pulpo` + tmux.
- `pulpo-agents`: extends `pulpo-base` and installs agent CLIs (Claude + Codex by default).

## Why two images

- `pulpo-base` stays lean and policy-safe.
- `pulpo-agents` is the convenience layer where provider tools and credentials are handled.

## Files

- `base/Dockerfile`
- `base/entrypoint.sh`
- `agents/Dockerfile`
- `agents/entrypoint.sh`
- `compose/base.yml`
- `compose/agents.yml`
- `.env.example`

## Quick Start

From repository root:

```bash
cp docker/.env.example docker/.env
# edit docker/.env with real values

# Build base image first
docker compose -f docker/compose/base.yml build

# Build agents image (FROM pulpo-base:dev)
docker compose -f docker/compose/agents.yml build

# Run agents node
docker compose -f docker/compose/agents.yml up -d

# Check health
curl http://localhost:7433/api/v1/health
```

## Runtime env contract

Core Pulpo settings (read by `base/entrypoint.sh`):

- `PULPO_NODE_NAME`
- `PULPO_BIND` (`local`, `tailscale`, `public`, or `container`)
- `PULPO_PORT`
- `PULPO_TOKEN` (recommended when `PULPO_BIND=public`)
- `PULPO_GUARD_PRESET`
- `PULPO_MAX_TURNS` (optional)
- `PULPO_MAX_BUDGET_USD` (optional)
- `PULPO_OUTPUT_FORMAT` (optional)

Optional webhook passthrough:

- `DISCORD_WEBHOOK_URL`
- `DISCORD_EVENTS` (comma-separated)

## Provider credential precedence (`pulpo-agents`)

Claude:

1. `CLAUDE_CODE_OAUTH_TOKEN`
2. `ANTHROPIC_API_KEY`

Codex:

1. `CODEX_OAUTH_TOKEN`
2. `OPENAI_API_KEY`

The agents entrypoint logs which auth mode is detected at startup.

## Git credentials (optional)

- `GIT_AUTHOR_NAME`
- `GIT_AUTHOR_EMAIL`
- `GIT_HTTP_HOST` (default: `github.com`)
- `GIT_HTTP_USERNAME`
- `GIT_HTTP_TOKEN` (PAT/fine-grained token)
- `GIT_SSH_PRIVATE_KEY_B64` (base64-encoded private key)

HTTPS PAT and SSH can both be configured; use whichever your workflow prefers.

## Port mapping and Tailscale

The image uses `bind = "container"` by default, which binds to `0.0.0.0` with **no auth** — it trusts the container network boundary.

**If you run Tailscale on the host**, map the port to localhost only:

```bash
# Safe: only reachable from the host, then expose via Tailscale
docker run -p 127.0.0.1:7433:7433 pulpo-agents
tailscale serve --bg 7433

# Unsafe: open to all network interfaces with no auth
docker run -p 7433:7433 pulpo-agents
```

Mapping to `127.0.0.1` ensures only local processes (and `tailscale serve`) can reach pulpod. Without it, the port is open on all host interfaces — anyone on the network can access it unauthenticated.

**Alternative**: run Tailscale as a sidecar inside Docker Compose and use `PULPO_BIND=tailscale`. The container becomes a first-class Tailscale node with no port mapping needed.

## Notes

- `pulpo-agents` build currently installs `@anthropic-ai/claude-code` and `@openai/codex` via npm.
- For production, prefer Docker secrets or secret managers over plain env vars.
- Use dedicated worker nodes/VMs for stronger sandboxing boundaries.
