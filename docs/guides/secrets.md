# Secrets

::: warning Operational Layer
Secrets are an operational feature for running real sessions safely. They are important in practice, but secondary to the core Pulpo model of sessions, runtimes, and lifecycle states.
:::

This guide matters most for:

- users running agents against private services or private repos
- teams keeping runtime execution on infrastructure they control
- sessions that need agent authentication tokens

Secrets are environment variables that get injected into agent sessions. They provide a way to pass sensitive values (API keys, tokens, credentials) to your sessions without hardcoding them in commands or inks.

For a full multi-node example, see
[Private Infrastructure With Tailscale And Secrets](/guides/private-infra-with-tailscale).

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

When injected, secrets are written to a temporary file (`/tmp/pulpo-secrets-<session-id>.sh`) with `0600` permissions. The session command sources and immediately deletes it — secret values never appear in the command string, `ps` output, or session logs.

### Validation rules

- Secret values must **not contain newlines or null bytes** (rejected with 400 error)
- If two `--secret` flags resolve to the **same env var** (e.g., both map to `GITHUB_TOKEN` via `--env`), spawn fails with a clear error — use only one

## Inks with Secrets

Inks can bundle a command and its secrets, making them reusable session blueprints:

```toml
[inks.work-coder]
command = "claude --dangerously-skip-permissions -p 'Implement the changes'"
description = "Coder with work GitHub token"
secrets = ["GH_WORK", "ANTHROPIC_KEY"]
```

Then spawn with just the ink — no `--secret` flags needed:

```bash
pulpo spawn my-task --ink work-coder
# Equivalent to:
# pulpo spawn my-task --secret GH_WORK --secret ANTHROPIC_KEY -- claude ...
```

Spawn flags extend ink defaults:

```bash
# Add extra secrets beyond what the ink provides
pulpo spawn my-task --ink work-coder --secret EXTRA_TOKEN
```

Ink secrets and request `--secret` flags are merged (deduplicated).

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

- **Storage**: Plaintext in SQLite (`~/.pulpo/state.db` by default)
- **File permissions**: Database file is set to mode `0600` (owner read/write only) on Unix systems
- **API exposure**: The REST API never returns secret values. `GET /api/v1/secrets` returns only names, env mappings, and creation timestamps. `PUT /api/v1/secrets/{name}` accepts a value but never echoes it back
- **Web UI**: The settings page shows secret names and env mappings but never fetches or displays values. The input field for adding secrets uses `type="password"` with a show/hide toggle
- **Injection**: Secrets are written to a temp file with `0600` permissions, sourced by the shell, and immediately deleted. Secret values never appear in `ps` output, command strings, or session logs.

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/v1/secrets` | List secret names, env mappings, and creation dates |
| `PUT` | `/api/v1/secrets/{name}` | Set a secret (body: `{"value": "...", "env": "..."}`) |
| `DELETE` | `/api/v1/secrets/{name}` | Delete a secret |

All endpoints require authentication (inside the auth middleware).

## Agent Authentication via Tokens

You can authenticate agents by passing their auth tokens as secrets, instead of relying on each agent's on-disk login:

```bash
pulpo secret set CLAUDE_TOKEN sk-ant-xxxx --env CLAUDE_CODE_OAUTH_TOKEN
pulpo spawn my-task --secret CLAUDE_TOKEN -- claude -p "review code"
```

See [Injecting Secrets into Sessions](#injecting-secrets-into-sessions) for details.

## Known Limitations

- **tmux resume**: Secrets are sourced from a temp file that is deleted after the first session start. Resumed tmux sessions do not re-inject secrets — the running shell already has them in its environment from the original start.
- **Env var collision**: If two secrets in a `--secret` list map to the same env var (via `--env`), spawn is rejected. Use only one secret per env var per session.
