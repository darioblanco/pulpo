#!/usr/bin/env bash
set -euo pipefail

PULPO_CONFIG_PATH="${PULPO_CONFIG_PATH:-/home/pulpo/.pulpo/config.toml}"
PULPO_DATA_DIR="${PULPO_DATA_DIR:-/home/pulpo/.pulpo/data}"
PULPO_NODE_NAME="${PULPO_NODE_NAME:-$(hostname)}"
PULPO_PORT="${PULPO_PORT:-7433}"
PULPO_BIND="${PULPO_BIND:-lan}"
PULPO_GUARD_PRESET="${PULPO_GUARD_PRESET:-standard}"
PULPO_OVERWRITE_CONFIG="${PULPO_OVERWRITE_CONFIG:-0}"

mkdir -p "$(dirname "$PULPO_CONFIG_PATH")" "$PULPO_DATA_DIR"

write_config() {
  {
    echo "[node]"
    echo "name = \"${PULPO_NODE_NAME}\""
    echo "port = ${PULPO_PORT}"
    echo "data_dir = \"${PULPO_DATA_DIR}\""
    echo
    echo "[auth]"
    echo "bind = \"${PULPO_BIND}\""
    if [[ -n "${PULPO_TOKEN:-}" ]]; then
      echo "token = \"${PULPO_TOKEN}\""
    fi
    echo
    echo "[guards]"
    echo "preset = \"${PULPO_GUARD_PRESET}\""

    if [[ -n "${PULPO_MAX_TURNS:-}" ]]; then
      echo "max_turns = ${PULPO_MAX_TURNS}"
    fi
    if [[ -n "${PULPO_MAX_BUDGET_USD:-}" ]]; then
      echo "max_budget_usd = ${PULPO_MAX_BUDGET_USD}"
    fi
    if [[ -n "${PULPO_OUTPUT_FORMAT:-}" ]]; then
      echo "output_format = \"${PULPO_OUTPUT_FORMAT}\""
    fi

    if [[ -n "${DISCORD_WEBHOOK_URL:-}" ]]; then
      echo
      echo "[notifications.discord]"
      echo "webhook_url = \"${DISCORD_WEBHOOK_URL}\""
      if [[ -n "${DISCORD_EVENTS:-}" ]]; then
        IFS=',' read -r -a events <<< "${DISCORD_EVENTS}"
        printf 'events = ['
        local first=1
        for e in "${events[@]}"; do
          # Trim whitespace around items.
          e="${e#${e%%[![:space:]]*}}"
          e="${e%${e##*[![:space:]]}}"
          [[ -z "$e" ]] && continue
          if [[ "$first" -eq 1 ]]; then
            first=0
          else
            printf ', '
          fi
          printf '"%s"' "$e"
        done
        printf ']\n'
      fi
    fi
  } > "$PULPO_CONFIG_PATH"
}

if [[ ! -f "$PULPO_CONFIG_PATH" || "$PULPO_OVERWRITE_CONFIG" == "1" ]]; then
  write_config
fi

if [[ "${1:-}" == "pulpod" ]]; then
  set -- pulpod --config "$PULPO_CONFIG_PATH"
fi

exec "$@"
