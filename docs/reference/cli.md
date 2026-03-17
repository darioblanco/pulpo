# CLI Reference

## Commands

```text
pulpo spawn <NAME> [OPTIONS] [-- <COMMAND...>]  Spawn a new session (auto-attaches)
pulpo list                                List sessions (alias: ls)
pulpo logs <NAME> [--follow]              Show session output
pulpo attach <NAME>                       Attach to tmux session
pulpo input <NAME> [TEXT]                 Send text input to a session
pulpo kill <NAME>                         Kill a running session
pulpo delete <NAME>                       Delete session record (alias: rm)
pulpo resume <NAME>                       Resume a lost or ready session (auto-attaches)
pulpo interventions <NAME>                Show watchdog interventions
pulpo nodes                               List known nodes/peers
pulpo schedule <SUBCOMMAND>               Manage scheduled sessions (crontab)
pulpo ui                                  Open web UI in browser
```

## Spawn Options

The first positional argument is the session **name** (required). Everything after `--` is the **command** to run in the tmux session.

```bash
pulpo spawn my-api --workdir ~/repos/my-api -- claude -p "Fix failing auth tests"
```

By default, `spawn` auto-attaches to the tmux session. Use `--detach` / `-d` to skip attachment (useful in scripts and the web UI).

| Flag | Description |
|------|-------------|
| `--workdir <PATH>` | Working directory (default: current) |
| `--detach` / `-d` | Don't attach to the session after spawning |
| `--ink <NAME>` | Ink preset from config (provides a default command) |
| `--description <TEXT>` | Human-readable description for the session |

The command is whatever you want to run — any agent CLI, script, or shell command. If `--ink` is specified and no command is given after `--`, the ink's command is used.

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
