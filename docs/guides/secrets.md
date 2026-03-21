# Secrets

Secrets are environment variables that get injected into agent sessions. They provide a way to pass sensitive values (API keys, tokens, credentials) to your sessions without hardcoding them in commands or inks.

## Overview

- Secrets are stored as **plaintext in SQLite** with restrictive file permissions (mode `0600`)
- The API **never returns secret values** -- only names and creation timestamps are exposed
- Secret names must be valid environment variable names: **uppercase alphanumeric + underscores** (e.g., `GITHUB_TOKEN`, `NPM_TOKEN`)
- Values are trimmed but otherwise unrestricted

## CLI Usage

### Set a secret

```bash
pulpo secret set GITHUB_TOKEN ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

### List secrets

```bash
pulpo secret list
# or
pulpo secret ls
```

Output:

```
NAME                           CREATED
GITHUB_TOKEN                   2026-03-21T12:00
NPM_TOKEN                      2026-03-20T10:30
```

### Delete a secret

```bash
pulpo secret delete GITHUB_TOKEN
# or
pulpo secret rm GITHUB_TOKEN
```

## Remote Nodes

Secrets are stored per-node. To manage secrets on a remote node, use the `--node` flag:

```bash
pulpo --node mac-mini secret set GITHUB_TOKEN ghp_xxx
pulpo --node mac-mini secret list
pulpo --node mac-mini secret delete GITHUB_TOKEN
```

## Security Model

- **Storage**: Plaintext in SQLite (`~/.pulpo/data/state.db`)
- **File permissions**: Database file is set to mode `0600` (owner read/write only) on Unix systems
- **API exposure**: The REST API never returns secret values. `GET /api/v1/secrets` returns only names and creation timestamps. `PUT /api/v1/secrets/{name}` accepts a value but never echoes it back
- **Web UI**: The settings page shows secret names but never fetches or displays values. The input field for adding secrets uses `type="password"` with a show/hide toggle

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/v1/secrets` | List secret names and creation dates |
| `PUT` | `/api/v1/secrets/{name}` | Set a secret (body: `{"value": "..."}`) |
| `DELETE` | `/api/v1/secrets/{name}` | Delete a secret |

All endpoints require authentication (inside the auth middleware).

## Example Workflow

Setting up a GitHub token for Docker-based agent sessions:

```bash
# Set the token on your build node
pulpo --node build-server secret set GITHUB_TOKEN ghp_xxxxxxxxxxxx

# Spawn a session that uses the token
pulpo --node build-server spawn code-review --runtime docker -- claude -p "review PRs"
```

> **Note:** Secret injection into sessions will be implemented in a future update. Currently, secrets are stored and managed but not yet automatically passed as environment variables to new sessions.
