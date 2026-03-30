# Secrets

::: warning Operational Layer
Secrets are an operational feature for running real sessions safely. They are important in practice, but secondary to the core Pulpo model of sessions, runtimes, and lifecycle states.
:::

This guide matters most for:

- users running agents against private services or private repos
- teams keeping runtime execution on infrastructure they control
- Docker-based sessions that still need agent authentication

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

When injected:
- **tmux sessions**: Secrets are written to a temporary file (`/tmp/pulpo-secrets-<session-id>.sh`) with `0600` permissions. The session command sources and immediately deletes it — secret values never appear in the command string, `ps` output, or session logs.
- **Docker sessions**: Secrets are passed as `-e KEY=VALUE` flags to `docker run`.

### Validation rules

- Secret values must **not contain newlines or null bytes** (rejected with 400 error)
- If two `--secret` flags resolve to the **same env var** (e.g., both map to `GITHUB_TOKEN` via `--env`), spawn fails with a clear error — use only one

## Inks with Secrets and Runtime

Inks can bundle secrets and runtime, making them reusable session blueprints:

```toml
[inks.docker-coder]
command = "claude --dangerously-skip-permissions -p 'Implement the changes'"
description = "Docker-isolated coder with work GitHub token"
secrets = ["GH_WORK", "ANTHROPIC_KEY"]
runtime = "docker"
```

Then spawn with just the ink — no `--secret` or `--runtime` flags needed:

```bash
pulpo spawn my-task --ink docker-coder
# Equivalent to:
# pulpo spawn my-task --runtime docker --secret GH_WORK --secret ANTHROPIC_KEY -- claude ...
```

Spawn flags override ink defaults:

```bash
# Use the ink's command and secrets, but override runtime to tmux
pulpo spawn my-task --ink docker-coder --runtime tmux

# Add extra secrets beyond what the ink provides
pulpo spawn my-task --ink docker-coder --secret EXTRA_TOKEN
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
- **Injection (tmux)**: Secrets are written to a temp file with `0600` permissions, sourced by the shell, and immediately deleted. Secret values never appear in `ps` output, command strings, or session logs.
- **Injection (Docker)**: Secrets are passed as `-e` flags to `docker run`. They're visible in `docker inspect` but not in pulpo's session logs.

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/v1/secrets` | List secret names, env mappings, and creation dates |
| `PUT` | `/api/v1/secrets/{name}` | Set a secret (body: `{"value": "...", "env": "..."}`) |
| `DELETE` | `/api/v1/secrets/{name}` | Delete a secret |

All endpoints require authentication (inside the auth middleware).

## Docker Authentication

Docker sessions automatically mount agent authentication directories into containers. This allows agents running inside Docker to authenticate without manual token setup.

### Default Volume Mounts

By default, these directories are mounted read-only into every Docker container:

| Host Path | Container Path | Description |
|-----------|---------------|-------------|
| `~/.claude` | `/root/.claude` | Claude Code auth and settings |
| `~/.codex` | `/root/.codex` | OpenAI Codex auth |
| `~/.gemini` | `/root/.gemini` | Google Gemini auth |

Volumes are mounted as **read-only** (`:ro`) -- agents can read tokens but cannot modify your local credentials.

If a host directory does not exist (e.g., you do not have Codex installed), that mount is silently skipped.

### macOS Keychain Extraction (Claude Code)

On macOS, Claude Code stores OAuth credentials in the system Keychain rather than on disk. When Docker sessions are created, pulpod automatically:

1. Checks if `~/.claude/.credentials.json` exists on disk
2. If not, extracts credentials from the macOS Keychain (`security find-generic-password -s "Claude Code-credentials" -w`)
3. Writes the credentials to a temp file in `~/.pulpo/docker-creds/` by default
4. Mounts that file as `/root/.claude/.credentials.json:ro` inside the container

This is fully automatic -- no configuration needed.

### Customizing Volume Mounts

Override the default volumes in `config.toml`:

```toml
[docker]
# Replace defaults with custom mounts
volumes = [
    "~/.claude:/root/.claude:ro",
    "~/.codex:/root/.codex:ro",
    "~/.gemini:/root/.gemini:ro",
    # Add git/ssh access (use with caution -- grants container access to your keys)
    # "~/.ssh:/root/.ssh:ro",
    # "~/.gitconfig:/root/.gitconfig:ro",
]
```

Set `volumes = []` to disable all default mounts.

### Alternative: Environment Variable Tokens

Instead of mounting auth directories, you can pass tokens via secrets:

```bash
pulpo secret set CLAUDE_TOKEN sk-ant-xxxx --env CLAUDE_CODE_OAUTH_TOKEN
pulpo spawn my-task --runtime docker --secret CLAUDE_TOKEN -- claude -p "review code"
```

See [Secrets](#injecting-secrets-into-sessions) for details.

## Known Limitations

- **Docker resume**: When a Docker session is resumed, the container is recreated without the original secrets. The secret names are not stored on the session itself, so they cannot be re-injected. tmux sessions are not affected because the secrets temp file is sourced at session start.
- **tmux resume**: Secrets are sourced from a temp file that is deleted after the first session start. Resumed tmux sessions do not re-inject secrets — the running shell already has them in its environment from the original start.
- **Env var collision**: If two secrets in a `--secret` list map to the same env var (via `--env`), spawn is rejected. Use only one secret per env var per session.
