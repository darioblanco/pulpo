# CLI Reference

## Commands

```text
pulpo spawn [NAME] [OPTIONS] [-- <COMMAND...>]  Spawn a new session (auto-attaches; alias: s)
pulpo handoff <SOURCE> [NAME] [OPTIONS] [-- <COMMAND...>]  Hand off a finished
                                          session's context to a new session (alias: h)
pulpo list [--all]                        List sessions, live only by default (alias: ls; -a/--all includes stopped/lost)
pulpo logs <NAME> [--lines N] [--follow]  Show session output (alias: l; default 100 lines; -f to tail)
pulpo attach <NAME>                       Attach to a session terminal (alias: a)
pulpo input <NAME> [TEXT]                 Send text input to a session (alias: i, send)
pulpo stop <NAME>... [--purge]            Stop one or more sessions (alias: k, kill; -p/--purge also removes from history)
pulpo cleanup                             Remove all stopped and lost sessions
pulpo resume <NAME>                       Resume a lost, ready, or stopped session (alias: r; auto-attaches)
pulpo interventions <NAME>                Show watchdog interventions (alias: iv)
pulpo usage                               Show token/cost burn rate, time-to-cap, and quota
pulpo usage --scan                        Scan ALL local agent history (Claude + Codex + pi):
                                          total spend by agent, model, and repo, no daemon-managed
                                          sessions required
pulpo usage --scan --by-worktree          Like --scan, but keep each git worktree/subdir as
                                          its own row instead of rolling them up to the origin repo
pulpo usage --scan --since <DAYS>         Like --scan, but limited to the last N days
pulpo usage [--scan] --json               Output raw JSON instead of the formatted report
pulpo schedule <SUBCOMMAND>               Manage scheduled sessions (alias: sched;
                                          cron-expression syntax, run by pulpod's own 60s
                                          scheduler loop — not crontab)
pulpo worktree list                       List worktree sessions (alias: wt ls)
pulpo ui                                  Open web UI in browser
pulpo <PATH>                              Quick spawn: spawns a session in that directory
                                          (name auto-generated from the directory basename)
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
pulpo schedule list                                             List installed jobs (alias: ls)
pulpo schedule pause <NAME|ID>                                  Pause a job
pulpo schedule resume <NAME|ID>                                 Resume a paused job
pulpo schedule remove <NAME|ID>                                 Remove a job (alias: rm)
```

| Flag | Description |
|------|-------------|
| `--workdir <PATH>` | Working directory (default: current) |
| `--description <TEXT>` | Human-readable description |
| `--worktree` | Create an isolated git worktree for each run |
| `--worktree-base <BRANCH>` | Fork worktree from a specific branch (implies `--worktree`) |
| `--budget-cost <USD>` | Cost budget applied to every session this schedule fires (watchdog alerts at 80%, stops at 100%) |

There is no per-schedule node flag — a schedule always fires on the node that holds it. To
create it on another machine, use the global `--url` flag before the subcommand (see
[Global Options](#global-options)): `pulpo --url gpu-box schedule add ...`.

**Scheduler behavior:** Schedules run in the daemon's machine timezone. The scheduler loop ticks every 60 seconds, so cron expressions more granular than 1 minute won't fire more often. Each schedule fire creates a fresh session with a timestamped name (`<schedule>-YYYYMMDD-HHMM`).

**Worktree schedules:** When `--worktree` is set, each scheduled run creates a fresh git worktree, giving the agent an isolated copy of the repository. A plain `pulpo stop` on that run's session leaves the worktree on disk; it's reclaimed on the next `pulpo stop --purge`, `pulpo cleanup`, or watchdog intervention. See [Worktrees](/guides/worktrees) for the full cleanup model.

## Hook (internal)

```text
pulpo hook <harness> [--event <NAME>]     Report a harness lifecycle event to the daemon
pulpo hook codex-notify <payload>         Codex's notify variant: payload as an argument, not stdin
```

Not meant to be run by hand — a [harness adapter](/architecture/harness-adapters) (the
Claude Code adapter's `--settings` hooks, the Codex adapter's hooks/notify config, or the
pi adapter's `pulpo.ts` extension) injects this as the command its own hook/event config
invokes, so the harness itself runs it whenever a lifecycle event fires (a turn finishes,
the agent needs a permission decision, the session ends, ...).

- Reads the event JSON from stdin (or treats it as `{}` if stdin is empty/unparseable).
- Resolves the session from the `PULPO_SESSION_ID` environment variable, which the
  session wrapper already exports into every pulpo-managed process. If it's unset (the
  harness is running outside pulpo), the hook exits immediately without a network call.
- Talks to the daemon at `PULPO_URL` (also exported into every session, alongside
  `PULPO_SESSION_ID`/`PULPO_SESSION_NAME`) rather than the CLI's own `--url` default —
  so a hook always reaches the daemon on whatever `[node].port` it's actually configured
  with, not just `7433`.
- `--event <NAME>` fills in `hook_event_name` in the payload when the harness's own
  JSON doesn't already carry one; pulpo's own Claude settings never need it. The Codex
  adapter uses this for every hook it wires (`pulpo hook codex --event SessionStart`,
  `--event Stop`, ...) since Codex's own hook payload has no confirmed field naming
  which event fired. pi's `pulpo.ts` always passes `--event <name>` too, alongside its
  own `event` field in the JSON body — either is enough to identify the event.
- POSTs to `/api/v1/sessions/{id}/harness-events` with a 2-second timeout.
- **Always exits 0 and prints nothing on success** — a hook must never block or break
  the agent it's wired into, regardless of what the daemon does or doesn't do.

**`codex-notify` variant:** Codex's `notify` mechanism delivers its JSON payload as a
trailing argv element rather than stdin, so `harness "codex-notify"` is a special case:
the payload is read from the `<payload>` argument instead. The Codex adapter wires
`notify = ["sh", "-c", "'<pulpo-bin>' hook codex-notify \"$0\""]` in its isolated
`config.toml`, which turns Codex's appended JSON into `$0` and, in turn, this command's
argument. It posts the raw payload, unmodified, to the daemon as harness `"codex"`
(mapped to a single `TurnFinished` event on `agent-turn-complete`) — never a synthetic
`SessionStart` alongside it (an earlier version tried that to learn the harness session id
even when the real `SessionStart` hook never fired, but it flapped the session
Active→Idle on every turn and was removed; a lost session whose `SessionStart` hook never
fired is instead recovered by Codex's rollout-discovery fallback on its next spawn/resume
— see [Harness Adapters](/architecture/harness-adapters#shipped-the-codex-adapter)). Same
always-exit-0/2s-timeout/silent contract as the general form.

## Global Options

```text
--url <HOST:PORT>    Daemon address to talk to (default: localhost:7433)
--token <TOKEN>      Auth token (for remote daemons)
```

`--url` accepts `host:port` or a full URL (e.g. `https://mac-mini.tailnet.ts.net`). A bare
hostname without a port gets the default pulpod port (`7433`) appended.

## Spawn on a Remote Daemon

```bash
pulpo --url mac-mini:7433 spawn my-task -- claude -p "fix bug"
```

## Scripting Recipes

### Approve all idle sessions

```bash
pulpo list | grep idle | awk '{print $2}' | xargs -I{} pulpo input {} "y"
```

(Column 1 is only an 8-char ID prefix — `pulpo input`/`stop`/`logs` need the full ID or
the name, so use column 2, the session name, instead. Since session names are always a
single kebab-case token, `$2` gives you the bare name even for a session whose NAME
column also carries a `[wt]`/`[PR]`/`[!]` badge — awk splits those into their own,
later fields.)

### Stop all active sessions

```bash
pulpo list | grep active | awk '{print $2}' | xargs -I{} pulpo stop {}
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

### Remote private-infra run with a credential

```bash
pulpo --url mac-mini spawn review-backend \
  --workdir ~/repos/backend \
  -- env GITHUB_TOKEN=ghp_work_xxxxxxxxxxxx claude -p "Review this service for correctness, security issues, and missing tests."
```

See [Private Infrastructure With Tailscale](/guides/private-infra-with-tailscale) for the complete recipe.

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
for name in $(pulpo list | awk 'NR>1 {print $2}'); do
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
