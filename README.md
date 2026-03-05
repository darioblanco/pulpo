# pulpo

[![CI](https://github.com/darioblanco/pulpo/actions/workflows/ci.yml/badge.svg)](https://github.com/darioblanco/pulpo/actions/workflows/ci.yml)

Run coding agents on your machines. Recover from failures. Control what they can access.

Pulpo is a daemon + CLI + web UI that orchestrates coding agents (Claude Code, Codex) running on your machines. It abstracts tmux session management behind a REST API, detects resource exhaustion, and gives you a dashboard to spawn, monitor, and control agents — without SSH or terminal juggling.

## Mission

Pulpo is a self-hosted control plane for running coding agents reliably across your own machines. It focuses on runtime durability, policy guardrails, and observable session lifecycle events. It is infrastructure, not a prompt framework.

See [MISSION.md](MISSION.md) for the mission and non-goals.

```bash
pulpod &                                              # start daemon
pulpo spawn --workdir ~/my-api "Fix the auth bug"      # spawn agent
pulpo logs my-api                                     # check output
# machine reboots...
pulpo list                                            # → my-api: stale
pulpo resume my-api                                   # → agent picks up where it left off
```

Control sessions from **CLI**, **web dashboard** (mobile-friendly), or **Discord bot** — on any machine in your network.

## Why Pulpo

You run agents, they crash, they eat all your RAM, you lose context. Managing across machines means SSH + tmux + hoping you remember which agent was doing what.

|                             | DIY tmux    | claude-squad / aider | Devin / Factory | **Pulpo**                   |
| --------------------------- | ----------- | -------------------- | --------------- | --------------------------- |
| Multi-machine dashboard     | —           | —                    | First-class     | First-class                 |
| Session survives reboot     | —           | —                    | Varies          | First-class (resume)        |
| Resource watchdog           | —           | Limited              | Varies          | First-class (interventions) |
| Environment guardrails      | Manual      | Limited              | Locked down     | Configurable (3 presets)    |
| REST API + MCP              | —           | —                    | Proprietary     | First-class (open)          |
| Audit trail                 | —           | —                    | Varies          | First-class (interventions) |
| Self-hosted / your machines | First-class | First-class          | —               | First-class                 |
| Real-time events (SSE)      | —           | —                    | Varies          | First-class                 |

## Quickstart

Install pulpo, set up an agent provider, start the daemon, spawn a session.

**macOS (Homebrew):**

```bash
brew install darioblanco/tap/pulpo tmux
```

**Linux (pre-built binary):**

```bash
curl -fsSL https://github.com/darioblanco/pulpo/releases/download/VERSION/pulpo-x86_64-unknown-linux-gnu.tar.xz | tar xJ
sudo mv pulpo pulpod /usr/local/bin/
sudo apt install -y tmux
```

<details>
<summary>Build from source (requires Rust 1.82+, Node.js 22+, tmux 3.2+)</summary>

```bash
git clone https://github.com/darioblanco/pulpo.git && cd pulpo
make setup && make build && make install
```

</details>

Need copy-paste examples for config, API, CLI, schedules, and Discord bot? See
[examples/README.md](examples/README.md).

Then set up your agent provider and run:

```bash
# Set up Claude Code (or export ANTHROPIC_API_KEY=sk-ant-...)
npm install -g @anthropic-ai/claude-code
claude login

# Start the daemon
pulpod &

# Spawn your first agent
pulpo spawn --workdir ~/repos/my-api "Fix the failing tests in auth.py"
pulpo list
pulpo logs my-api

# Stream events
curl -N http://localhost:7433/api/v1/events

# Open the dashboard
open http://localhost:7433
```

<details>
<summary>Run as a service</summary>

**launchd (macOS):**

```bash
brew services start pulpo
```

**systemd (Linux):**

```bash
mkdir -p ~/.config/systemd/user
cp contrib/pulpo.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now pulpo
```

</details>

## Workflows

### Nightly code review on a remote node

Define a persona in `~/.pulpo/config.toml`:

```toml
[personas.reviewer]
provider = "claude"
model = "sonnet"
guard_preset = "strict"
system_prompt = "You are a senior code reviewer. Focus on correctness, security, and test coverage."
```

Spawn the review from your laptop, targeting the mac-mini:

```bash
pulpo --node mac-mini:7433 spawn \
  --persona reviewer \
  --workdir ~/repos/my-api \
  "Review all changes since yesterday"

# Check status from CLI
pulpo --node mac-mini:7433 logs my-api

# Or stream lifecycle events
curl -N http://mac-mini:7433/api/v1/events
# data: {"kind":"session","session_name":"my-api","status":"running","node_name":"mac-mini",...}
```

Check from your phone: open `http://mac-mini:7433` — the web UI is mobile-friendly.

### Schedule a recurring run

Use the crontab wrapper for recurring autonomous work:

```bash
# Every day at 02:00
pulpo schedule install nightly-review "0 2 * * *" \
  --workdir ~/repos/my-api \
  "Review changes from the last 24 hours"

pulpo schedule list
pulpo schedule pause nightly-review
pulpo schedule resume nightly-review
pulpo schedule remove nightly-review
```

This writes tagged lines to your user crontab. Each run spawns a fresh `pulpo spawn --auto` session.

### Spawn from Discord, track in SSE, kill from phone

```
# In Discord:
/spawn repo:/home/user/repos/api prompt:Fix the auth bug

# Discord notification channel shows lifecycle events automatically
# Check progress:
/logs name:api

# Done — kill from Discord (works from your phone):
/kill name:api
```

The web UI at `http://your-node:7433` also works on mobile if you prefer a browser. See [contrib/discord-bot/README.md](contrib/discord-bot/README.md) for Discord bot setup.

### Recover after a reboot

Your machine rebooted — the tmux session is gone, but pulpo's SQLite store remembers everything:

```bash
pulpo list
# NAME      STATUS   PROVIDER  PROMPT
# my-api    stale    claude    Fix the auth bug

pulpo resume my-api
# → Creates fresh tmux session, feeds the original prompt + conversation ID back to Claude

pulpo logs my-api
# → Agent picks up where it left off
```

What happened: `pulpod` started, checked SQLite for active sessions, found no matching tmux session for `my-api`, marked it `stale`. The `resume` command creates a new tmux session and resumes the Claude conversation with the stored context.

**Session states at a glance:**

| Status      | Meaning                    | Action                        |
| ----------- | -------------------------- | ----------------------------- |
| `creating`  | tmux session being set up  | Wait                          |
| `running`   | Agent is active            | `logs`, `attach`, `kill`      |
| `completed` | Exited cleanly (exit 0)    | `delete` or keep for history  |
| `dead`      | Crashed or was killed      | `spawn` new or `delete`       |
| `stale`     | DB record, no tmux session | `resume`                      |

`resume` only works for **stale** sessions. Dead sessions need a fresh `spawn`. See [SPEC.md](SPEC.md#failure--recovery) for full recovery flows.

## Features

- **Single binary** — `pulpod` embeds the web UI. No runtime dependencies besides tmux.
- **Resource watchdog** — monitors memory pressure, kills runaway agents, logs interventions.
- **Idle detection** — detects sessions with no output for a configurable timeout.
- **Guard presets** — standard, strict, or unrestricted modes control agent permissions.
- **Session persistence** — sessions survive daemon restarts; resume after reboot.
- **Multi-provider** — Claude Code and OpenAI Codex out of the box.
- **Multi-node dashboard** — see agents across all machines in one view.
- **Personas** — pre-configured agent profiles (provider, model, guard, tools, system prompt).
- **SSE events** — real-time session lifecycle events via Server-Sent Events.
- **Discord integration** — webhook notifications + bot with slash commands.
- **Scheduling** — crontab wrapper for recurring agent runs (install/list/pause/resume/remove).
- **Output capture** — periodic terminal snapshots for offline viewing.
- **MCP server** — session management as MCP tools for agent-to-agent orchestration.
- **Trusted-network design** — works on any LAN, VPN, or Tailscale network. No TLS required when your network handles encryption.
- **Token authentication** — bearer token auth for non-localhost deployments (auto-generated on first run).
- **mDNS discovery** — automatic peer detection on LAN (activates in `public` bind mode).
- **Remote access** — use `tailscale serve` to expose pulpod over your tailnet (see [SPEC.md](SPEC.md#remote-access-via-tailscale)).

## What Pulpo Guarantees

**Guarantees:**

- Session state persists across daemon restarts (SQLite-backed)
- Guard presets are enforced at spawn time — env vars sanitized, tool flags set
- Stale sessions are detectable and resumable with conversation context
- Every watchdog intervention is logged with reason and timestamp
- SSE events fire for session status transitions

See [SPEC.md](SPEC.md#interventions) for details on intervention tracking and recovery flows.

**Does not guarantee:**

- Provider CLI stability — Claude Code and Codex may change flags or behavior between versions
- OS-level sandboxing — guards configure the agent, not the OS
- Output completeness — terminal snapshots are periodic, not streaming-to-disk

## Configuration

Pulpo works with zero configuration. Optionally, create `~/.pulpo/config.toml`:

```toml
[node]
name = "mac-mini"

[personas.reviewer]
provider = "claude"
model = "sonnet"
guard_preset = "strict"
system_prompt = "You are a code reviewer."

[notifications.discord]
webhook_url = "https://discord.com/api/webhooks/..."
```

See [SPEC.md](SPEC.md#configuration) for all options (watchdog, guards, auth, peers).

## CLI Reference

All subcommands have short aliases for quick access.

```
pulpo attach <NAME>                  Attach to a session's tmux terminal (alias: a)
pulpo input <NAME> [TEXT]            Send input to a session (alias: i)
pulpo spawn [OPTIONS] <PROMPT>       Spawn a new agent session (alias: s)
  --workdir <PATH>                  Working directory (required)
  --name <NAME>                     Session name (auto-derived if omitted)
  --provider <claude|codex>         Agent provider (default: claude)
  --guard <strict|standard|unrestricted>  Guard preset (default: standard)
  --model <MODEL>                   Model override (e.g. opus, sonnet)
  --system-prompt <TEXT>            System prompt to append
  --allowed-tools <TOOL,...>        Allowed tools (comma-separated)
  --persona <NAME>                  Use a persona from config
  --max-turns <N>                   Maximum agent turns before stopping
  --max-budget <USD>                Maximum budget in USD before stopping
  --output-format <FORMAT>          Output format (e.g. json, stream-json)
  --auto                            Autonomous mode (fire-and-forget)

pulpo list                           List all sessions (alias: ls)
pulpo logs <NAME> [--follow/-f]       View session output (alias: l)
pulpo kill <NAME>                    Kill a session (alias: k)
pulpo delete <NAME>                  Permanently remove a session from history (alias: rm)
pulpo resume <NAME>                  Resume a session after reboot (alias: r)
pulpo interventions <NAME>           View intervention history for a session (alias: iv)
pulpo schedule <SUBCOMMAND>          Manage cron schedules (alias: sched)
  install <NAME> <CRON> --workdir <PATH> [--provider] <PROMPT>
                                     Install a cron schedule
  list                               List installed schedules (alias: ls)
  remove <NAME>                      Remove a schedule (alias: rm)
  pause <NAME>                       Pause a schedule (comments crontab line)
  resume <NAME>                      Resume a paused schedule
pulpo nodes                          List all known nodes on the network (alias: n)
pulpo ui                             Open the web dashboard in your browser
```

Target a remote node:

```bash
pulpo --node macbook:7433 spawn --workdir ~/repos/ml-model "Train the model"
pulpo --node macbook:7433 list
```

## Roadmap

- [x] Single-node MVP: daemon, CLI, web UI, tmux backend, Claude Code
- [x] Live terminal: WebSocket streaming, xterm.js, session resume
- [x] Multi-node: manual peer config, aggregated dashboard, remote spawning
- [x] Guard presets: standard, strict, unrestricted permission control
- [x] Token auth, mobile-friendly web UI
- [x] Resource watchdog: memory pressure detection, safe intervention, audit trail
- [x] Idle detection: detect and act on sessions with no output
- [x] mDNS discovery: automatic peer detection on LAN (activates in `public` bind mode)
- [x] Persona system: configurable agent personas with model, tools, and system prompt
- [x] SSE event stream: real-time session lifecycle events
- [x] MCP server: session management as MCP tools for agent-to-agent orchestration
- [x] Discord integration: webhook notifications + bot with slash commands
- [x] Scheduling: crontab wrapper for recurring agent runs
- [ ] Per-process kill: kill runaway child processes without killing the agent
- [ ] Tailscale API discovery: automatic peer detection via Tailscale
- [ ] Provider adapter registry: trait-based, config-driven provider system
- [ ] One-command remote deploy: `pulpo deploy user@host`
- [ ] Optional TLS: self-signed certificate generation for untrusted networks

See [ROADMAP.md](ROADMAP.md) for detailed project sequencing.

## Development

Requires **Rust 1.82+**, **Node.js 22+**, and **tmux 3.2+**.

```bash
git clone https://github.com/darioblanco/pulpo.git && cd pulpo
make setup          # First-time: install Rust tools, git hooks, web deps
make build          # Build release binaries with embedded web UI
make install        # Copy pulpo and pulpod to /usr/local/bin
```

### Useful targets

```bash
make dev            # Run the daemon from source (port 7433)
make dev-web        # Run the web UI dev server (port 5173, proxies to pulpod)
make all            # Format + lint + test (runs on every commit via pre-commit hook)
make test           # Run all tests (cargo test + vitest)
make test-web-watch # Web tests in watch mode
make coverage       # Run tests with 100% line coverage enforcement
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full local development workflow and [CLAUDE.md](CLAUDE.md) for detailed conventions and project structure.

## Why "Pulpo"?

Pulpo is Spanish for octopus.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
