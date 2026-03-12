# CLI Reference

## Commands

```text
pulpo spawn [OPTIONS] [PROMPT...]     Spawn a new agent session
pulpo list                            List sessions (alias: ls)
pulpo logs <NAME> [--follow]          Show session output
pulpo attach <NAME>                   Attach to tmux session
pulpo input <NAME> [TEXT]             Send text input to a session
pulpo kill <NAME>                     Kill a running session
pulpo delete <NAME>                   Delete session record (alias: rm)
pulpo resume <NAME>                   Resume a lost or finished session
pulpo interventions <NAME>            Show watchdog interventions
pulpo culture [OPTIONS]               Query and manage culture entries
pulpo nodes                           List known nodes/peers
pulpo schedule <SUBCOMMAND>           Manage scheduled sessions (crontab)
pulpo ui                              Open web UI in browser
```

## Spawn Options

| Flag | Description | Providers |
|------|-------------|-----------|
| `--workdir <PATH>` | Working directory (default: current) | All |
| `--name <NAME>` | Session name (auto-generated if omitted) | All |
| `--provider <NAME>` | Agent provider | All |
| `--auto` | Autonomous mode (fire-and-forget) | All |
| `--ink <NAME>` | Ink preset from config | All |
| `--unrestricted` | Disable safety guardrails | Claude, Gemini |
| `--model <MODEL>` | Model override | Claude, Codex, Gemini |
| `--worktree` | Git worktree isolation | Claude |
| `--system-prompt <TEXT>` | System prompt | Claude |
| `--allowed-tools <TOOLS>` | Allowed tools (comma-separated) | Claude |
| `--max-turns <N>` | Max agent turns | Claude |
| `--max-budget <USD>` | Max budget in USD | Claude |
| `--output-format <FMT>` | Output format | Claude, Gemini, OpenCode |

### Providers

Available providers: `claude`, `codex`, `gemini`, `opencode`, `shell`

Provider availability is checked at spawn time. Use `shell` for a bare tmux session without any agent.

### Worktree Isolation

The `--worktree` flag (Claude only) gives each session its own git worktree — an isolated copy of the repo on a separate branch:

```bash
# Two agents working on the same repo in parallel
pulpo spawn --worktree --workdir ~/myproject "add caching layer"
pulpo spawn --worktree --workdir ~/myproject "refactor auth module"
```

Worktrees are created at `<repo>/.claude/worktrees/<session-name>`. Other providers can work in a Claude-created worktree by pointing `--workdir` at it.

## Culture Options

```text
pulpo culture                              List all entries
pulpo culture --context                    Show compiled context (what agents receive)
pulpo culture --context --repo <PATH>      Context scoped to a repo
pulpo culture --context --ink <NAME>       Context scoped to an ink
pulpo culture --get <ID>                   Get a specific entry
pulpo culture --delete <ID>                Delete an entry
pulpo culture --push                       Push culture repo to remote
pulpo culture --kind <KIND>                Filter by kind (summary, failure)
pulpo culture --session <ID>               Filter by session
```

## Schedule Subcommands

```text
pulpo schedule install <CRON> [SPAWN_ARGS]   Install a cron job
pulpo schedule list                           List installed jobs
pulpo schedule pause <ID>                     Pause a job
pulpo schedule resume <ID>                    Resume a paused job
pulpo schedule remove <ID>                    Remove a job
```

## Global Options

```text
--host <HOST>     pulpod host (default: localhost)
--port <PORT>     pulpod port (default: 7433)
--token <TOKEN>   Auth token (for remote nodes)
--json            Output as JSON
```

For full options on any command:

```bash
pulpo --help
pulpo <command> --help
```
