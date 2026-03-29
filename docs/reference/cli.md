# CLI Reference

## Commands

```text
pulpo spawn [NAME] [OPTIONS] [-- <COMMAND...>]  Spawn a new session (auto-attaches)
pulpo list                                List sessions (alias: ls)
pulpo logs <NAME> [--follow]              Show session output
pulpo attach <NAME>                       Attach to a session terminal
pulpo input <NAME> [TEXT]                 Send text input to a session
pulpo stop <NAME>                         Stop a running session
pulpo cleanup                             Remove all stopped and lost sessions
pulpo resume <NAME>                       Resume a lost or ready session (auto-attaches)
pulpo interventions <NAME>                Show watchdog interventions
pulpo nodes                               List known nodes/peers
pulpo schedule <SUBCOMMAND>               Manage scheduled sessions (crontab)
pulpo ink <SUBCOMMAND>                    Manage ink presets (reusable command templates)
pulpo worktree list                       List worktree sessions (alias: wt ls)
pulpo secret <SUBCOMMAND>                 Manage secrets (env vars for sessions)
pulpo ui                                  Open web UI in browser
```

## Spawn Options

The first positional argument is the session **name** (optional). Everything after `--` is the **command** to run in the session.

```bash
pulpo spawn my-api --workdir ~/repos/my-api -- claude -p "Fix failing auth tests"
```

By default, `spawn` auto-attaches to the session. Use `--detach` / `-d` to skip attachment (useful in scripts and the web UI).

| Flag | Description |
|------|-------------|
| `--workdir <PATH>` | Working directory (default: current) |
| `--detach` / `-d` | Don't attach to the session after spawning |
| `--ink <NAME>` | Ink preset from config (provides a default command) |
| `--description <TEXT>` | Human-readable description for the session |
| `--idle-threshold <SECS>` | Per-session idle threshold (`0` = never idle) |
| `--worktree` | Create an isolated git worktree for the session |
| `--worktree-base <BRANCH>` | Fork worktree from a specific branch (implies `--worktree`) |
| `--runtime <RUNTIME>` | Session runtime: `tmux` (default) or `docker` |
| `--auto` | Auto-select the least loaded node |
| `--secret <NAME>` | Inject a stored secret as an environment variable |

If no name is provided, Pulpo derives one from the workdir/path context. If no command is provided, Pulpo falls back to the ink command, `node.default_command`, or finally `$SHELL`.

The command is whatever you want to run — any agent CLI, script, or shell command. If `--ink` is specified and no command is given after `--`, the ink's command is used.

## Schedule Subcommands

```text
pulpo schedule add <NAME> <CRON> [OPTIONS] [-- <COMMAND...>]   Add a cron job
pulpo schedule install <NAME> <CRON> [OPTIONS] [-- <COMMAND...>]   Alias for add
pulpo schedule list                                             List installed jobs
pulpo schedule pause <ID>                                       Pause a job
pulpo schedule resume <ID>                                      Resume a paused job
pulpo schedule remove <ID>                                      Remove a job
```

| Flag | Description |
|------|-------------|
| `--workdir <PATH>` | Working directory (default: current) |
| `--node <NAME>` | Target node (omit = local, `auto` = least-loaded) |
| `--ink <NAME>` | Ink preset from config |
| `--description <TEXT>` | Human-readable description |
| `--runtime <RUNTIME>` | Session runtime: `tmux` (default) or `docker` |
| `--secret <NAME>` | Inject a stored secret (repeatable) |
| `--worktree` | Create an isolated git worktree for each run |
| `--worktree-base <BRANCH>` | Fork worktree from a specific branch (implies `--worktree`) |

**Scheduler behavior:** Schedules run in the daemon's machine timezone. The scheduler loop ticks every 60 seconds, so cron expressions more granular than 1 minute won't fire more often. Each schedule fire creates a fresh session with a timestamped name (`<schedule>-YYYYMMDD-HHMM`).

**Worktree schedules:** When `--worktree` is set, each scheduled run creates a fresh git worktree, giving the agent an isolated copy of the repository. The worktree is cleaned up when the session is stopped.

## Ink Subcommands

```text
pulpo ink list                             List all ink presets (alias: ls)
pulpo ink get <NAME>                       Show ink details
pulpo ink add <NAME> [OPTIONS]             Add a new ink preset
pulpo ink update <NAME> [OPTIONS]          Update an existing ink preset
pulpo ink remove <NAME>                    Remove an ink preset (alias: rm)
```

| Flag | Description |
|------|-------------|
| `--description <TEXT>` | Human-readable description |
| `--command <CMD>` | Command template |
| `--runtime <RUNTIME>` | Default runtime: `tmux` or `docker` |
| `--secret <NAME>` | Default secrets to inject (repeatable) |

Inks are persisted in `config.toml` and take effect immediately for new sessions. When used with `pulpo spawn --ink <NAME>`, the ink provides defaults for command, description, runtime, and secrets. Explicit spawn flags override ink defaults.

## Secret Subcommands

```text
pulpo secret set <NAME> <VALUE>           Set a secret (env var)
pulpo secret list                         List secret names (alias: ls)
pulpo secret delete <NAME>                Delete a secret (alias: rm)
```

Secrets are environment variables injected into sessions. Names must be uppercase alphanumeric with underscores (e.g., `GITHUB_TOKEN`). Values are never returned by the API.

## Global Options

```text
--node <HOST:PORT>   Target node (default: localhost:7433). Accepts peer names too.
--token <TOKEN>      Auth token (for remote nodes)
```

`--node` accepts either `host:port` or a peer name from your config (e.g., `--node mac-mini` resolves via the local daemon's peer registry).

## Spawn on Remote Nodes

```bash
# By address
pulpo --node mac-mini:7433 spawn my-task -- claude -p "fix bug"

# By peer name (resolved via peer registry)
pulpo --node mac-mini spawn my-task -- claude -p "fix bug"

# Auto-select least loaded node
pulpo spawn my-task --auto -- claude -p "fix bug"
```

## Scripting Recipes

### Approve all idle sessions

```bash
pulpo list | grep idle | awk '{print $1}' | xargs -I{} pulpo input {} "y"
```

### Stop all active sessions

```bash
pulpo list | grep active | awk '{print $1}' | xargs -I{} pulpo stop {}
```

### Spawn agents across multiple repos

```bash
for repo in my-api my-frontend my-infra; do
  pulpo spawn "${repo}-review" --workdir ~/repos/${repo} -d -- claude -p "review code"
done
```

### Follow all sessions in parallel (tmux panes)

```bash
tmux new-session -d -s monitor
for name in $(pulpo list | awk 'NR>1 {print $1}'); do
  tmux split-window -t monitor "pulpo logs ${name} --follow"
  tmux select-layout -t monitor tiled
done
tmux attach -t monitor
```

For full options on any command:

```bash
pulpo --help
pulpo <command> --help
```
