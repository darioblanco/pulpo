#!/usr/bin/env bash
set -euo pipefail

log() {
  printf '[pulpo-agents] %s\n' "$*"
}

# Optional git identity for automated commits.
if [[ -n "${GIT_AUTHOR_NAME:-}" ]]; then
  git config --global user.name "${GIT_AUTHOR_NAME}"
fi
if [[ -n "${GIT_AUTHOR_EMAIL:-}" ]]; then
  git config --global user.email "${GIT_AUTHOR_EMAIL}"
fi

# Optional HTTPS credential injection (username + PAT/token).
# Useful for private repo access without SSH keys.
if [[ -n "${GIT_HTTP_USERNAME:-}" && -n "${GIT_HTTP_TOKEN:-}" ]]; then
  GIT_HTTP_HOST="${GIT_HTTP_HOST:-github.com}"
  GIT_CREDENTIALS_FILE="${GIT_CREDENTIALS_FILE:-/home/pulpo/.git-credentials}"
  git config --global credential.helper "store --file ${GIT_CREDENTIALS_FILE}"
  {
    printf 'protocol=https\n'
    printf 'host=%s\n' "${GIT_HTTP_HOST}"
    printf 'username=%s\n' "${GIT_HTTP_USERNAME}"
    printf 'password=%s\n' "${GIT_HTTP_TOKEN}"
    printf '\n'
  } | git credential approve
  chmod 600 "${GIT_CREDENTIALS_FILE}" || true
  log "Loaded HTTPS git credentials for host ${GIT_HTTP_HOST}"
elif [[ -n "${GIT_HTTP_USERNAME:-}" || -n "${GIT_HTTP_TOKEN:-}" ]]; then
  log 'Incomplete HTTPS git credentials: set both GIT_HTTP_USERNAME and GIT_HTTP_TOKEN'
fi

# Optional SSH key injection (base64-encoded private key).
if [[ -n "${GIT_SSH_PRIVATE_KEY_B64:-}" ]]; then
  mkdir -p /home/pulpo/.ssh
  chmod 700 /home/pulpo/.ssh
  printf '%s' "${GIT_SSH_PRIVATE_KEY_B64}" | base64 -d > /home/pulpo/.ssh/id_ed25519
  chmod 600 /home/pulpo/.ssh/id_ed25519
  touch /home/pulpo/.ssh/known_hosts
  chmod 600 /home/pulpo/.ssh/known_hosts
  log 'Loaded SSH key from GIT_SSH_PRIVATE_KEY_B64'
fi

# Claude auth: OAuth token takes precedence over API key.
if [[ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ]]; then
  log 'Claude auth mode: oauth_token'
elif [[ -n "${ANTHROPIC_API_KEY:-}" ]]; then
  log 'Claude auth mode: api_key'
else
  log 'Claude auth mode: missing credentials'
fi

# Codex auth: OAuth token takes precedence over API key.
if [[ -n "${CODEX_OAUTH_TOKEN:-}" ]]; then
  log 'Codex auth mode: oauth_token'
elif [[ -n "${OPENAI_API_KEY:-}" ]]; then
  log 'Codex auth mode: api_key'
else
  log 'Codex auth mode: missing credentials'
fi

exec /usr/local/bin/pulpo-entrypoint.sh "$@"
