# Secrets

Secrets are environment variables that get injected into agent sessions. They provide a way to pass sensitive values (API keys, tokens, credentials) to your sessions without hardcoding them in commands or inks.

## Overview

- Secrets are stored as **plaintext in SQLite** with restrictive file permissions (mode `0600`)
- The API **never returns secret values** -- only names, env var mappings, and creation timestamps are exposed
- Secret names must be valid environment variable names: **uppercase alphanumeric + underscores** (e.g., `GITHUB_TOKEN`, `NPM_TOKEN`)
- Each secret has an optional `--env` override: the env var name used when injecting the secret into sessions. Defaults to the secret name.
- Values are trimmed but otherwise unrestricted

## CLI Usage

### Set a secret

```bash
pulpo secret set GITHUB_TOKEN ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

With a custom env var name (useful for multiple tokens that map to the same env var):

```bash
pulpo secret set GH_WORK ghp_work_token --env GITHUB_TOKEN
pulpo secret set GH_PERSONAL ghp_personal_token --env GITHUB_TOKEN
```

### List secrets

```bash
pulpo secret list
# or
pulpo secret ls
```

Output:

```
NAME                     ENV                      CREATED
GITHUB_TOKEN             GITHUB_TOKEN             2026-03-21T12:00
GH_WORK                 GITHUB_TOKEN             2026-03-21T12:05
NPM_TOKEN                NPM_TOKEN                2026-03-20T10:30
```

### Delete a secret

```bash
pulpo secret delete GITHUB_TOKEN
# or
pulpo secret rm GITHUB_TOKEN
```

## Injecting Secrets into Sessions

Use the `--secret` flag on `pulpo spawn` to inject secrets as environment variables:

```bash
# Inject a single secret
pulpo spawn my-task --secret GITHUB_TOKEN -- claude -p "review PRs"

# Inject multiple secrets
pulpo spawn my-task --secret GITHUB_TOKEN --secret NPM_TOKEN -- npm run build

# Using env override: GH_WORK secret is injected as GITHUB_TOKEN
pulpo spawn my-task --secret GH_WORK -- claude -p "review PRs"
```

When injected:
- **tmux sessions**: Secrets are exported as `export KEY='VALUE'` statements inside the `bash -l -c '...'` wrapper. They are baked into the command string, so they persist across resume.
- **Docker sessions**: Secrets should be passed as `-e KEY=VALUE` flags (future enhancement). Currently only tmux injection is fully supported.

## Example Workflow

Using multiple GitHub tokens for different repos:

```bash
# Store tokens with descriptive names, both mapping to GITHUB_TOKEN
pulpo secret set GH_WORK ghp_work_xxxxxxxxxxxx --env GITHUB_TOKEN
pulpo secret set GH_OSS ghp_oss_xxxxxxxxxxxx --env GITHUB_TOKEN

# Use the work token for company repos
pulpo spawn review-backend --secret GH_WORK --workdir /code/backend -- claude -p "review code"

# Use the OSS token for open source repos
pulpo spawn review-oss --secret GH_OSS --workdir /code/oss-project -- claude -p "review code"
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
- **API exposure**: The REST API never returns secret values. `GET /api/v1/secrets` returns only names, env mappings, and creation timestamps. `PUT /api/v1/secrets/{name}` accepts a value but never echoes it back
- **Web UI**: The settings page shows secret names and env mappings but never fetches or displays values. The input field for adding secrets uses `type="password"` with a show/hide toggle
- **Injection**: For tmux, secrets are baked into the shell command string (visible in `ps` output). For sensitive environments, consider Docker runtime which isolates the process.

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/v1/secrets` | List secret names, env mappings, and creation dates |
| `PUT` | `/api/v1/secrets/{name}` | Set a secret (body: `{"value": "...", "env": "..."}`) |
| `DELETE` | `/api/v1/secrets/{name}` | Delete a secret |

All endpoints require authentication (inside the auth middleware).

## Known Limitations

- **Docker resume**: When a Docker session is resumed, the container is recreated without the original secrets. The secret names are not stored on the session itself, so they cannot be re-injected. tmux sessions do not have this limitation because secrets are baked into the wrapped command string.
- **`ps` visibility**: On tmux, exported secrets are visible in `ps` output as part of the shell command. Use Docker runtime for stronger isolation.
