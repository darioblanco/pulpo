# Worktrees Guide

Run multiple agents on the same repository without conflicts. Each agent gets its own git worktree — an isolated checkout with its own branch and working directory.

## Why worktrees?

Without worktrees, two agents editing the same repo will clobber each other's changes. Git worktrees solve this at the filesystem level: each agent works in a separate directory on a separate branch, all sharing the same `.git` history.

## Usage

Add `--worktree` to any spawn command:

```bash
pulpo spawn fix-auth --workdir ~/repos/my-api --worktree -- claude -p "fix the auth middleware"
```

This:
1. Creates branch `pulpo/fix-auth` from the current HEAD
2. Checks out a worktree at `~/repos/my-api/.pulpo/worktrees/fix-auth/`
3. Runs the command inside that worktree

The session's working directory is set to the worktree path, so the agent sees a normal git checkout.

## Branch naming

Worktree branches follow the pattern `pulpo/<session-name>`:

| Session name | Branch |
|-------------|--------|
| `fix-auth` | `pulpo/fix-auth` |
| `refactor-db` | `pulpo/refactor-db` |

## Worktree location

All worktrees live under the repo's `.pulpo/worktrees/` directory:

```
~/repos/my-api/
├── .pulpo/
│   └── worktrees/
│       ├── fix-auth/          # full checkout
│       └── refactor-db/       # full checkout
├── src/
└── ...
```

Add `.pulpo/` to your `.gitignore` — it's a local workspace directory, not something to commit.

## Cleanup

Worktrees are removed automatically when you kill or delete the session:

```bash
pulpo kill fix-auth    # removes worktree + prunes git references
pulpo delete fix-auth  # same cleanup if the session was already killed
```

The cleanup runs `git worktree prune` on the parent repo to keep git's worktree list clean.

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
git branch | grep pulpo/
# pulpo/fix-auth
# pulpo/add-tests
# pulpo/update-docs
```

Create PRs from each branch, or merge directly.

## Docker runtime

Worktrees work with `--runtime docker` too. The worktree directory is mounted into the container instead of the original repo:

```bash
pulpo spawn risky-fix --workdir ~/repos/my-api --worktree --runtime docker -- claude --dangerously-skip-permissions -p "refactor"
```

## CLI indicator

Sessions using worktrees show a `[wt]` badge in `pulpo list`:

```
fix-auth [wt]   active   2m   claude -p "fix auth middleware"
add-tests [wt]  idle     5m   claude -p "add missing unit tests"
```

## Requirements

- The `--workdir` must point to a git repository (or be inside one)
- Git must be installed and available in PATH
- The branch `pulpo/<session-name>` must not already exist
