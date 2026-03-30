# Docker-Isolated Risky Tasks

This recipe shows how to run higher-risk agent work inside Docker while keeping
the overall session managed by Pulpo.

It is useful when a task needs:

- stronger isolation than a plain tmux session
- broader filesystem or shell permissions
- protection against accidental damage to your main working environment

## What This Solves

Some agent workflows are routine and low-risk. Others are not.

Examples of higher-risk work:

- broad refactors across many files
- commands that use `--dangerously-skip-permissions`
- large dependency updates
- codebase-wide search-and-replace tasks
- aggressive cleanup or migration work

Pulpo's Docker runtime gives you a cleaner boundary:

- the session is still tracked by Pulpo
- the task runs in a container
- the repo or worktree is mounted into `/workspace`
- secrets and auth can still be injected in controlled ways

## When This Recipe Fits

Use this when you want:

- to let an agent run more freely without giving it your host shell directly
- a safer environment for unattended or high-impact tasks
- the same Pulpo lifecycle semantics with better runtime isolation

## The Simple Version

Run a risky refactor in Docker:

```bash
pulpo spawn risky-refactor \
  --workdir ~/repos/my-api \
  --runtime docker \
  -- claude --dangerously-skip-permissions -p "Refactor the service layer and simplify the data flow."
```

What happens:

1. Pulpo creates a managed session as usual
2. the session runs in a Docker container instead of tmux
3. your workdir is mounted into `/workspace`
4. the session is still visible in the same CLI, UI, API, and logs

## Safer Version With Worktree

For stronger isolation, combine Docker with a worktree:

```bash
pulpo spawn risky-refactor \
  --workdir ~/repos/my-api \
  --worktree \
  --runtime docker \
  -d \
  -- claude --dangerously-skip-permissions -p "Refactor the service layer and keep the changes coherent."
```

That gives you two boundaries:

- container isolation for execution
- worktree isolation for git state

This is a strong default for risky background tasks.

## Add Secrets Cleanly

If the task needs tokens or credentials, inject them through Pulpo instead of
putting them in the command string:

```bash
pulpo spawn risky-audit \
  --workdir ~/repos/my-api \
  --runtime docker \
  --secret GH_WORK \
  --secret ANTHROPIC_KEY \
  -- claude --dangerously-skip-permissions -p "Audit this repository and propose fixes."
```

See [Secrets](/guides/secrets) for the full model.

## Reusable Ink Version

If you do this kind of task often, define an ink:

```toml
[inks.risky-refactor]
description = "Docker-isolated high-permission refactor"
command = "claude --dangerously-skip-permissions -p 'Refactor the target area and keep the changes coherent.'"
runtime = "docker"
secrets = ["GH_WORK", "ANTHROPIC_KEY"]
```

Then spawn with:

```bash
pulpo spawn risky-refactor --workdir ~/repos/my-api --ink risky-refactor
```

Or combine it with a worktree:

```bash
pulpo spawn risky-refactor --workdir ~/repos/my-api --worktree --ink risky-refactor
```

## Watching The Session

The operational model stays the same:

```bash
pulpo ls
pulpo logs risky-refactor --follow
pulpo stop risky-refactor
pulpo resume risky-refactor
```

That is the point: Docker changes the runtime boundary, not the session model.

## What To Be Careful About

Docker isolation is useful, but it is not magic.

Keep in mind:

- the mounted repo is still writable by the containerized task
- mounted auth directories or extra Docker volumes expand what the agent can access
- if you use `--workdir` without `--worktree`, the container still writes to your main checkout

So the safest practical combination for risky work is often:

- `--runtime docker`
- `--worktree`
- explicit `--secret` usage

## Good Fits

This recipe is especially good for:

- repo-wide refactors
- migration prep work
- large documentation rewrites
- dependency churn
- exploratory audit or cleanup tasks

## Related Docs

- [Secrets](/guides/secrets)
- [Worktrees Guide](/guides/worktrees)
- [Parallel Agents On One Repo](/guides/parallel-agents-one-repo)
- [Private Infrastructure With Tailscale And Secrets](/guides/private-infra-with-tailscale)

## Summary

This workflow gives you a practical middle ground:

- not just raw host execution
- not a hosted vendor sandbox
- still fully managed by Pulpo

For high-impact or high-permission tasks, that is often the right tradeoff.
