# Agent Examples

Pulpo is command-agnostic.

That means it does not care which coding agent you use, as long as the tool can
be launched from the terminal. Pulpo manages the session lifecycle, runtime,
recovery, and supervision around that command.

This page shows concise examples for common agent tools and one provider-backed
tooling path for GLM-5.

## How To Read These Examples

Each example is intentionally simple.

You can combine the same agent command with Pulpo features such as:

- `--worktree` for isolated git branches
- `--runtime docker` for stronger execution isolation
- `--secret` for credentials
- `pulpo schedule add ...` for recurring runs

The command is the agent. Pulpo is the runtime and control plane around it.

## Claude Code

Basic session:

```bash
pulpo spawn claude-review --workdir ~/repos/my-api -- claude -p "Review this code for bugs and regressions"
```

Worktree variant:

```bash
pulpo spawn claude-fix --workdir ~/repos/my-api --worktree -d -- claude -p "Fix the auth flow"
```

## Codex

Basic session:

```bash
pulpo spawn codex-fix --workdir ~/repos/my-api -- codex "Fix the failing auth tests"
```

Worktree variant:

```bash
pulpo spawn codex-refactor --workdir ~/repos/my-api --worktree -d -- codex "Refactor the session handling"
```

## Gemini CLI

Basic session:

```bash
pulpo spawn gemini-docs --workdir ~/repos/my-api -- gemini "Update the API documentation"
```

Scheduled variant:

```bash
pulpo schedule add nightly-gemini "0 3 * * *" --workdir ~/repos/my-api -- gemini "Review recent changes"
```

## Kimi Code

Basic session:

```bash
pulpo spawn kimi-review --workdir ~/repos/my-api -- kimi "Review this repository for bugs and missing tests"
```

Worktree variant:

```bash
pulpo spawn kimi-refactor --workdir ~/repos/my-api --worktree -d -- kimi "Refactor the auth flow"
```

## GLM-5 Via OpenCode

GLM-5 is best represented here through a compatible coding tool rather than as a
presumed standalone `glm` CLI.

One documented path is OpenCode configured to use Z.AI / GLM-5. Once that is set
up, Pulpo runs it like any other CLI tool.

Basic session:

```bash
pulpo spawn glm-review --workdir ~/repos/my-api -- opencode
```

Safer variant:

```bash
pulpo spawn glm-risky --workdir ~/repos/my-api --worktree --runtime docker -d -- opencode
```

If you want the Docker-focused rationale, see
[Docker-Isolated Risky Tasks](/guides/docker-isolated-risky-tasks).

## Common Patterns

### Add a worktree

```bash
pulpo spawn my-task --workdir ~/repos/my-api --worktree -d -- <agent-command>
```

### Run in Docker

```bash
pulpo spawn my-task --workdir ~/repos/my-api --runtime docker -- <agent-command>
```

### Schedule it

```bash
pulpo schedule add nightly-task "0 3 * * *" --workdir ~/repos/my-api -- <agent-command>
```

### Put it behind an ink

```toml
[inks.agent-review]
description = "Reusable review workflow"
command = "claude -p 'Review this repository for bugs, regressions, and missing tests.'"
runtime = "docker"
```

Then:

```bash
pulpo spawn review --workdir ~/repos/my-api --ink agent-review
```

## What This Page Is Not

This page is not:

- a benchmark comparison
- a claim of official partnership
- an exhaustive compatibility matrix

It is a practical illustration of Pulpo's command-agnostic model.

## Related Docs

- [Quickstart](/getting-started/quickstart)
- [Configuration Guide](/guides/configuration)
- [Nightly Code Review](/guides/nightly-code-review)
- [Parallel Agents On One Repo](/guides/parallel-agents-one-repo)
- [Docker-Isolated Risky Tasks](/guides/docker-isolated-risky-tasks)
