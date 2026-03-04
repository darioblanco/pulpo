# syntax=docker/dockerfile:1.7

ARG BASE_IMAGE=pulpo-base:latest
FROM ${BASE_IMAGE}

USER root

RUN apt-get update \
    && apt-get install -y --no-install-recommends nodejs npm \
    && rm -rf /var/lib/apt/lists/*

ARG INSTALL_CLAUDE=1
ARG INSTALL_CODEX=1
ARG CLAUDE_NPM_PACKAGE=@anthropic-ai/claude-code
ARG CODEX_NPM_PACKAGE=@openai/codex

RUN if [ "$INSTALL_CLAUDE" = "1" ]; then npm install -g "$CLAUDE_NPM_PACKAGE"; fi \
    && if [ "$INSTALL_CODEX" = "1" ]; then npm install -g "$CODEX_NPM_PACKAGE"; fi

COPY docker/agents/entrypoint.sh /usr/local/bin/pulpo-agents-entrypoint.sh
RUN chmod +x /usr/local/bin/pulpo-agents-entrypoint.sh

USER pulpo

ENTRYPOINT ["tini", "--", "/usr/local/bin/pulpo-agents-entrypoint.sh"]
CMD ["pulpod"]
