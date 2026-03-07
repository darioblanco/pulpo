# Quickstart

## 1. Install and authenticate an agent provider

Claude Code example:

```bash
npm install -g @anthropic-ai/claude-code
claude login
```

Codex is also supported if installed and authenticated.

## 2. Start daemon

```bash
pulpod
```

## 3. Spawn a session

```bash
pulpo spawn --workdir ~/repos/my-api "Fix failing auth tests"
```

## 4. Watch progress

```bash
pulpo list
pulpo logs my-api --follow
```

## 5. Open UI and events stream

```bash
open http://localhost:7433
curl -N http://localhost:7433/api/v1/events
```
