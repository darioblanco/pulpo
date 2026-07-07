# CLI Reference

## Commands

```text
pulpo spawn [NAME] [OPTIONS] [-- <COMMAND...>]  Spawn a new session (auto-attaches)
pulpo handoff <SOURCE> [NAME] [OPTIONS] [-- <COMMAND...>]  Hand off a finished
                                          session's context to a new session (alias: h)
pulpo list                                List sessions (alias: ls)
pulpo logs <NAME> [--follow]              Show session output
pulpo attach <NAME>                       Attach to a session terminal
pulpo input <NAME> [TEXT]                 Send text input to a session
pulpo stop <NAME>                         Stop a running session
pulpo cleanup                             Remove all stopped and lost sessions
pulpo resume <NAME>                       Resume a lost or ready session (auto-attaches)
pulpo interventions <NAME>                Show watchdog interventions
pulpo usage                               Show token/cost burn rate, time-to-cap, and quota
pulpo usage --scan                        Scan ALL local agent history (Claude + Codex + pi):
                                          total spend by agent, model, and repo, no daemon-managed
                                          sessions required
pulpo usage --scan --by-worktree          Like --scan, but keep each git worktree/subdir as
                                          its own row instead of rolling them up to the origin repo
pulpo usage --scan --since <DAYS>         Like --scan, but limited to the last N days
pulpo usage [--scan] --json               Output raw JSON instead of the formatted report
pulpo nodes                               List known nodes/peers
pulpo schedule <SUBCOMMAND>               Manage scheduled sessions (crontab)
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
| `--description <TEXT>` | Human-readable description for the session |
| `--idle-threshold <SECS>` | Per-session idle threshold (`0` = never idle) |
| `--worktree` / `-w` | Create an isolated git worktree for the session |
| `--worktree-base <BRANCH>` | Fork worktree from a specific branch (implies `--worktree`) |
| `--secret <NAME>` | Inject a stored secret as an environment variable |
| `--budget-cost <USD>` | Cost budget; watchdog alerts at 80% and stops the session at 100% |

If no name is provided, Pulpo derives one from the workdir/path context. If no command is provided, Pulpo falls back to `node.default_command`, or finally `$SHELL`.

The command is whatever you want to run — any agent CLI, script, or shell command.

## Handoff

`pulpo handoff <SOURCE> [NAME] [OPTIONS] [-- <COMMAND...>]` (alias `h`) spawns a **new**
session that inherits a finished session's working context — its working directory, and
its git worktree if it has one — so a plan-then-build flow across two agents (or two
models) is one command instead of a manual `cd`/branch dance.

```bash
pulpo spawn plan-auth -w -- claude --model opus -p "Plan the auth refactor, write PLAN.md"
# ...plan-auth finishes...
pulpo handoff plan-auth -- codex "implement PLAN.md"
```

Pulpo never reads or interprets `PLAN.md` (or any other artifact) — it only guarantees
the next command starts in the same directory (and worktree, if any).

| Flag | Description |
|------|-------------|
| `NAME` | New session name (auto-generated as `<source>-2`, `-3`, ... if omitted) |
| `--description <TEXT>` | Human-readable description for the new session |
| `--secret <NAME>` | Inject a stored secret as an environment variable (repeatable) |
| `--budget-cost <USD>` | Cost budget for the new session |
| `--idle-threshold <SECS>` | Per-session idle threshold (`0` = never idle) |
| `--detach` / `-d` | Don't attach to the new session after handoff |

If the source session used a worktree, the new session **adopts it** — no new branch or
checkout is created. A worktree shared this way is only reclaimed once *every* session
referencing it has stopped (via `stop --purge` or `pulpo cleanup`), so purging the source
session early never deletes work a handoff session still needs. See
[Plan Then Build](/guides/plan-then-build) for the full workflow.

If no command is given, the new session opens a login shell in the same directory —
handy for wrapping up manually.

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
| `--description <TEXT>` | Human-readable description |
| `--secret <NAME>` | Inject a stored secret (repeatable) |
| `--worktree` | Create an isolated git worktree for each run |
| `--worktree-base <BRANCH>` | Fork worktree from a specific branch (implies `--worktree`) |
| `--budget-cost <USD>` | Cost budget applied to every session this schedule fires (watchdog alerts at 80%, stops at 100%) |

There is no per-schedule node flag — a schedule always fires on the node that holds it. To
create it on another machine, use the global `--node` flag before the subcommand (see
[Global Options](#global-options)): `pulpo --node gpu-box schedule add ...`.

**Scheduler behavior:** Schedules run in the daemon's machine timezone. The scheduler loop ticks every 60 seconds, so cron expressions more granular than 1 minute won't fire more often. Each schedule fire creates a fresh session with a timestamped name (`<schedule>-YYYYMMDD-HHMM`).

**Worktree schedules:** When `--worktree` is set, each scheduled run creates a fresh git worktree, giving the agent an isolated copy of the repository. The worktree is cleaned up when the session is stopped.

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

### Parallel agents on one repo with worktrees

```bash
pulpo spawn frontend --workdir ~/repos/my-app --worktree -d -- claude -p "Redesign the settings page"
pulpo spawn backend  --workdir ~/repos/my-app --worktree -d -- codex "Optimize the user query path"
```

See [Parallel Agents On One Repo](/guides/parallel-agents-one-repo) for the complete recipe.

### Nightly review with a cost budget

```bash
pulpo schedule add nightly-review "0 3 * * *" \
  --workdir ~/repos/my-api \
  --budget-cost 5.0 \
  -- claude -p "Review the last day's commits for bugs, security issues, and style"
```

See [Nightly Code Review](/guides/nightly-code-review) for the complete recipe.

### Remote private-infra run with secrets

```bash
pulpo --node mac-mini spawn review-backend \
  --workdir ~/repos/backend \
  --secret GH_WORK \
  -- claude -p "Review this service for correctness, security issues, and missing tests."
```

See [Private Infrastructure With Tailscale And Secrets](/guides/private-infra-with-tailscale) for the complete recipe.

### Worktree-isolated risky task

```bash
pulpo spawn risky-refactor \
  --workdir ~/repos/my-api \
  --worktree \
  -- claude --dangerously-skip-permissions -p "Refactor the service layer and simplify the data flow."
```

The `--worktree` flag gives the agent an isolated git worktree on its own branch, so a high-permission run cannot disturb your main checkout. See [Worktrees](/guides/worktrees) for the complete recipe.

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
