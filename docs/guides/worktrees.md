# Worktrees Guide

::: warning Operational Layer
Worktrees are a useful operational feature, but they are not required to understand or use Pulpo. The core model is still session -> runtime -> lifecycle.
:::

This guide matters most for:

- solo users running multiple agents on one repo
- teams using Pulpo for parallel implementation or review tasks
- anyone who wants agent concurrency without branch collisions

Run multiple agents on the same repository without conflicts. Each agent gets its own git worktree — an isolated checkout with its own branch and working directory.

If you want the full end-to-end workflow rather than the feature explanation,
see [Parallel Agents On One Repo](/guides/parallel-agents-one-repo).

## Why worktrees?

Without worktrees, two agents editing the same repo will clobber each other's changes. Git worktrees solve this at the filesystem level: each agent works in a separate directory on a separate branch, all sharing the same `.git` history.

## Usage

Add `--worktree` to any spawn command:

```bash
pulpo spawn fix-auth --workdir ~/repos/my-api --worktree -- claude -p "fix the auth middleware"
```

This:
1. Creates branch `fix-auth` from the current HEAD
2. Checks out a worktree at `~/.pulpo/worktrees/fix-auth/`
3. Runs the command inside that worktree

The session's working directory is set to the worktree path, so the agent sees a normal git checkout.

## Base branch

By default, worktrees branch from the current HEAD. Use `--worktree-base` to fork from a specific branch:

```bash
pulpo spawn fix-auth --workdir ~/repos/my-api --worktree-base main --worktree -- claude -p "fix auth"
```

`--worktree-base` implies `--worktree`, so this also works:

```bash
pulpo spawn fix-auth --workdir ~/repos/my-api --worktree-base main -- claude -p "fix auth"
```

## Branch naming

Worktree branches use the session name directly:

| Session name | Branch |
|-------------|--------|
| `fix-auth` | `fix-auth` |
| `refactor-db` | `refactor-db` |

## Worktree location

All worktrees live under `~/.pulpo/worktrees/`:

```
~/.pulpo/
└── worktrees/
    ├── fix-auth/          # full checkout
    └── refactor-db/       # full checkout
```

## Listing worktree sessions

Use `pulpo worktree list` (or `pulpo wt ls`) to see all sessions with worktrees:

```
NAME                 BRANCH               STATUS     PATH
fix-auth             fix-auth             active     /home/user/.pulpo/worktrees/fix-auth
add-tests            add-tests            idle       /home/user/.pulpo/worktrees/add-tests
```

## Cleanup

A worktree is reclaimed when its session is **purged** — either by stopping with
`--purge`, or by `pulpo cleanup`. A plain `pulpo stop` marks the session `stopped` but
leaves the worktree on disk so you can still inspect it; it is reclaimed on the next purge.

```bash
pulpo stop fix-auth --purge   # stop + remove worktree dir, prune git refs, delete branch
pulpo cleanup                 # reclaim every stopped/lost session's worktree, plus a safe sweep
```

Per-session reclamation (purge) does three things:

1. Removes the worktree directory
2. Runs `git worktree prune` on the parent repo
3. Deletes the worktree branch (`git branch -D <session-name>`)

`pulpo cleanup` additionally runs a **safe orphan sweep**: it removes worktree directories
under `~/.pulpo/worktrees/` that are no longer referenced by *any* session (left behind by
sessions deleted long ago) and deletes leftover per-session output logs. It never touches a
directory still owned by a live session — to reclaim a finished session that is still
`active`/`idle`/`ready` (its tmux pane lingers), stop it first. `pulpo cleanup` reports how
many sessions, worktrees, and log files it removed.

If a stale branch is found when creating a new worktree with the same name, it is automatically cleaned up.

## Example: parallel agents on one repo

Spawn three agents working on different parts of the same codebase:

```bash
pulpo spawn fix-auth    --workdir ~/repos/my-api --worktree -d -- claude -p "fix auth middleware"
pulpo spawn add-tests   --workdir ~/repos/my-api --worktree -d -- claude -p "add missing unit tests"
pulpo spawn update-docs --workdir ~/repos/my-api --worktree -d -- codex "update API docs"
```

Each agent runs in its own worktree on its own branch. When they finish, review the branches:

```bash
cd ~/repos/my-api
git branch
# fix-auth
# add-tests
# update-docs
```

Create PRs from each branch, or merge directly.

## CLI indicator

Sessions using worktrees show a `[wt]` badge in `pulpo list`:

```
fix-auth [wt]   active   2m   claude -p "fix auth middleware"
add-tests [wt]  idle     5m   claude -p "add missing unit tests"
```

## Requirements

- The `--workdir` must point to a git repository (or be inside one)
- Git must be installed and available in PATH
- The branch `<session-name>` must not already exist (stale branches are auto-cleaned on retry)
