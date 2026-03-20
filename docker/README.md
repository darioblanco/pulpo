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
- `compose/tailscale.yml`
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

**Alternative**: run Tailscale as a sidecar inside Docker Compose (see [Tailscale sidecar](#tailscale-sidecar) below). The container becomes a first-class Tailscale node with no host port mapping needed.

## Tailscale sidecar

The `tailscale.yml` compose file runs the agents image with a Tailscale sidecar container. The agent container shares the sidecar's network namespace (`network_mode: service:tailscale`), making it a first-class node on your tailnet with no host port mapping needed.

The agents container still uses `PULPO_BIND=container` (binds `0.0.0.0`, no auth). This is intentional — `BindMode::Tailscale` would try to run `tailscale status --json` for peer discovery, but the `tailscale` CLI lives in the sidecar, not the agents container. Instead, the sidecar handles Tailscale networking, and other bare-metal pulpod instances discover this container via their own Tailscale discovery loop. The container trusts its network boundary (the sidecar).

### Prerequisites

1. **Tailscale auth key**: Generate a reusable, ephemeral auth key from the [Tailscale admin console](https://login.tailscale.com/admin/settings/keys). Tag it with `tag:pulpo` in your ACLs.
2. **ACL tag**: Add `"tag:pulpo"` to your Tailscale ACL policy so pulpo nodes can be tagged and discovered.

### Setup

```bash
cp docker/.env.example docker/.env
# Edit docker/.env:
#   TS_AUTHKEY=tskey-auth-...
#   TS_TAILNET=your-tailnet.ts.net
#   TS_HOSTNAME=pulpo-worker-1
#   PULPO_NODE_NAME=pulpo-worker-1

# Build base + agents images first
docker compose -f docker/compose/base.yml build
docker compose -f docker/compose/agents.yml build

# Run with Tailscale sidecar
docker compose -f docker/compose/tailscale.yml up -d

# Verify the node joined the tailnet
docker exec pulpo-tailscale tailscale status
```

Once running, the node is reachable at `https://pulpo-worker-1.your-tailnet.ts.net` via `tailscale serve`. Other pulpod instances will auto-discover it if they run Tailscale discovery with `tag:pulpo`.

### Scaling multiple workers

Run multiple instances by overriding the node name and container names:

```bash
PULPO_NODE_NAME=pulpo-worker-2 TS_HOSTNAME=pulpo-worker-2 \
  docker compose -f docker/compose/tailscale.yml -p pulpo-worker-2 up -d
```

Each instance gets its own Tailscale identity and appears as a separate peer.

## Architecture

The Tailscale sidecar pattern uses Docker's `network_mode: service:` to share a single network namespace between the Tailscale container and the pulpo agents container:

```
                         Tailnet (WireGuard)
                              │
                    ┌─────────▼──────────┐
                    │  Docker Compose     │
                    │                     │
                    │  ┌───────────────┐  │
                    │  │  tailscale    │  │
                    │  │  sidecar      │  │
                    │  │               │  │
                    │  │  :443 HTTPS ──┼──┼── https://worker-1.tailnet.ts.net
                    │  │    │         │  │
                    │  │    │ serve   │  │
                    │  │    ▼         │  │
                    │  │  127.0.0.1   │  │
                    │  └──────┬───────┘  │
                    │         │ shared   │
                    │         │ netns    │
                    │  ┌──────▼───────┐  │
                    │  │ pulpo-agents │  │
                    │  │              │  │
                    │  │  pulpod      │  │
                    │  │  :7433       │  │
                    │  │  0.0.0.0     │  │
                    │  │              │  │
                    │  │  ┌────────┐  │  │
                    │  │  │  tmux  │  │  │
                    │  │  │ agents │  │  │
                    │  │  └────────┘  │  │
                    │  └──────────────┘  │
                    └────────────────────┘

Bare-metal pulpod nodes (bind = "tailscale") discover
this container via `tailscale status --json` + tag:pulpo.
The container does NOT run discovery itself — it uses
bind = "container" and trusts the sidecar network boundary.
```

**Key points:**

- Both containers share `localhost` — the sidecar's `tailscale serve` proxies `:443` to `127.0.0.1:7433` which reaches pulpod directly.
- No host port mapping needed — all traffic flows through the tailnet.
- The container appears as a normal Tailscale peer to other pulpod nodes.
- Discovery is one-directional: bare-metal nodes find containers, not the reverse.

## Troubleshooting

### Container won't join the tailnet

```bash
# Check sidecar logs
docker logs pulpo-tailscale

# Verify auth key is set
docker exec pulpo-tailscale tailscale status
```

**Common causes:**
- `TS_AUTHKEY` is expired or missing. Auth keys expire after 90 days by default — generate a new one from the [Tailscale admin console](https://login.tailscale.com/admin/settings/keys).
- `TS_AUTHKEY` doesn't have the `tag:pulpo` tag. The key must be authorized to assign the tag in your ACL policy.

### Container joined tailnet but isn't reachable

```bash
# Verify tailscale serve is running
docker exec pulpo-tailscale tailscale serve status

# Check pulpod is listening
docker exec pulpo-agents curl -s http://127.0.0.1:7433/api/v1/health
```

**Common causes:**
- `TS_SERVE_CONFIG` not mounted correctly. Check `docker exec pulpo-tailscale cat /config/serve.json`.
- `TS_TAILNET` env var doesn't match your actual tailnet domain. Find it in the Tailscale admin console under DNS.
- `TS_HOSTNAME` doesn't match `PULPO_NODE_NAME` — they should be the same value.

### Bare-metal nodes don't discover the container

```bash
# On the bare-metal node, check tailscale sees the container
tailscale status | grep pulpo-worker

# Check if the tag filter matches
tailscale status --json | jq '.Peer[] | select(.Tags // [] | any(. == "tag:pulpo"))'
```

**Common causes:**
- The container node doesn't have `tag:pulpo`. Verify `TS_EXTRA_ARGS` includes `--advertise-tags=tag:pulpo`.
- The bare-metal pulpod isn't using `bind = "tailscale"`. Only this mode enables the Tailscale discovery loop.
- ACL policy doesn't allow the tag. Add `"tag:pulpo"` to your `tagOwners` in the Tailscale ACL file.

### ACL policy setup

Add this to your Tailscale ACL policy (Settings > Access Controls):

```json
{
  "tagOwners": {
    "tag:pulpo": ["autogroup:admin"]
  },
  "acls": [
    {
      "action": "accept",
      "src": ["tag:pulpo"],
      "dst": ["tag:pulpo:*"]
    }
  ]
}
```

This allows all `tag:pulpo` nodes to communicate with each other.

### pulpod starts but agents can't authenticate

```bash
docker logs pulpo-agents 2>&1 | grep 'auth mode'
```

If you see `Claude auth mode: missing credentials`, ensure `ANTHROPIC_API_KEY` or `CLAUDE_CODE_OAUTH_TOKEN` is set in your `.env` file. Same for Codex with `OPENAI_API_KEY` / `CODEX_OAUTH_TOKEN`.

### Container hostname conflicts

If you run multiple containers with the same `TS_HOSTNAME`, Tailscale will append a suffix (e.g., `pulpo-worker-1-2`). Always use unique names per instance:

```bash
# Worker 1
PULPO_NODE_NAME=pulpo-worker-1 TS_HOSTNAME=pulpo-worker-1 \
  docker compose -f docker/compose/tailscale.yml -p worker-1 up -d

# Worker 2
PULPO_NODE_NAME=pulpo-worker-2 TS_HOSTNAME=pulpo-worker-2 \
  docker compose -f docker/compose/tailscale.yml -p worker-2 up -d
```

## Notes

- `pulpo-agents` build currently installs `@anthropic-ai/claude-code` and `@openai/codex` via npm.
- For production, prefer Docker secrets or secret managers over plain env vars.
- Use dedicated worker nodes/VMs for stronger isolation boundaries.
