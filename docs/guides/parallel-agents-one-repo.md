# Parallel Agents On One Repo

This recipe shows how to run multiple coding agents against the same repository
at the same time without having them step on each other.

The key mechanism is git worktrees: each session gets its own checkout and its
own branch, while sharing the same underlying repository history.

## What Problem This Solves

Without worktrees, parallel agents on one repo are fragile:

- they edit the same checkout
- they overwrite each other's working tree changes
- they compete for branch state
- it becomes unclear which agent produced which diff

Pulpo solves that by creating a separate worktree per session.

## When This Recipe Fits

Use this when you want:

- one agent fixing bugs while another updates tests
- one agent implementing a feature while another reviews or documents
- multiple background tasks on one repo without branch collisions

This recipe is especially useful for solo power users and teams doing parallel
implementation work. See [Use Cases](/getting-started/use-cases).

## The Simple Version

Start two agents on the same repo:

```bash
pulpo spawn frontend --workdir ~/repos/my-app --worktree -d -- claude -p "Redesign the settings page"
pulpo spawn backend  --workdir ~/repos/my-app --worktree -d -- codex "Optimize the user query path"
```

What happens:

1. Pulpo creates a `frontend` branch and worktree
2. Pulpo creates a `backend` branch and worktree
3. Each agent runs in its own isolated checkout
4. Both sessions appear in the same Pulpo dashboard and CLI

## A More Realistic Split

Run three sessions for a feature push:

```bash
pulpo spawn fix-auth    --workdir ~/repos/my-api --worktree -d -- claude -p "Fix the auth middleware"
pulpo spawn add-tests   --workdir ~/repos/my-api --worktree -d -- claude -p "Add missing unit tests for auth flows"
pulpo spawn update-docs --workdir ~/repos/my-api --worktree -d -- codex "Update the API auth docs"
```

This gives you:

- one branch per task
- one session per task
- independent output and lifecycle state for each agent

## Watching Progress

Use the CLI:

```bash
pulpo ls
pulpo logs fix-auth --follow
pulpo logs add-tests --follow
```

Or open the web UI and watch the sessions side by side.

Pulpo keeps each session separate, so you can see:

- which task is still `active`
- which one is `idle` and may need input
- which one is `ready`
- which one created a branch or PR

## Reviewing The Result

When the agents finish:

```bash
cd ~/repos/my-api
git branch
```

You should now see branches such as:

```text
fix-auth
add-tests
update-docs
```

From there you can:

- inspect each branch manually
- open PRs from each branch
- merge selected work
- resume a session if it is `ready` or `lost`

## Using Docker Too

Worktrees also work with Docker sessions:

```bash
pulpo spawn risky-refactor --workdir ~/repos/my-api --worktree --runtime docker -d -- claude --dangerously-skip-permissions -p "Refactor the service layer"
```

In that case, Pulpo mounts the worktree path into the container instead of the
original repo path.

## Choosing Good Task Boundaries

Parallel agents work best when tasks are meaningfully separate.

Good examples:

- frontend vs backend changes
- implementation vs tests
- docs vs refactor
- performance work vs bug fix

Poor examples:

- two agents editing the same module in different ways
- ambiguous tasks with overlapping ownership
- multiple agents making broad refactors across the whole repo

Pulpo isolates the filesystem state, but it does not remove semantic merge
conflicts. You still need good task boundaries.

## Related Commands

```bash
pulpo wt ls
pulpo stop frontend
pulpo stop backend
pulpo resume frontend
```

Stopping a session cleans up its worktree and branch according to the worktree
lifecycle behavior described in [Worktrees Guide](/guides/worktrees).

## Summary

This workflow is valuable because it turns "run a few agents in parallel" from a
fragile terminal habit into a controlled, inspectable setup:

- one repo
- multiple branches
- multiple managed sessions
- no shared working tree conflicts
