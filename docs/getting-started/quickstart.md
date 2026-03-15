# Quickstart

## 1. Install and authenticate an agent provider

Claude Code example:

```bash
npm install -g @anthropic-ai/claude-code
claude login
```

Other supported providers: Codex, Gemini CLI, OpenCode. Install and authenticate whichever you prefer.

## 2. Start daemon

```bash
pulpod
```

The web UI is available at [http://localhost:7433](http://localhost:7433).

## 3. Spawn a session

```bash
pulpo spawn my-api --workdir ~/repos/my-api "Fix failing auth tests"
```

This auto-attaches to the tmux session. Detach with `Ctrl-b d` to return to your shell. Use `--detach` / `-d` to skip auto-attach.

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

## 6. Resume after a crash

If the daemon restarts or your machine reboots, sessions become **lost**. Resume them:

```bash
pulpo list
# my-api   lost   ...

pulpo resume my-api
```

## Next steps

- [Configuration Guide](/guides/configuration) — inks, watchdog, peers
- [Discovery Guide](/guides/discovery) — multi-node setup with Tailscale, mDNS, or seed
- [CLI Reference](/reference/cli) — all commands and flags
- [Session Lifecycle](/operations/session-lifecycle) — state machine, transitions, detection
