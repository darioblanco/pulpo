# Agent Examples

Pulpo is command-agnostic.

That means it does not care which coding agent you use, as long as the tool can
be launched from the terminal. Pulpo manages the session lifecycle, runtime,
recovery, and supervision around that command.

This page shows concise examples for common agent tools, one provider-backed
tooling path for GLM-5, and pointing agents at a self-hosted local model.

## How To Read These Examples

Each example is intentionally simple.

You can combine the same agent command with Pulpo features such as:

- `--worktree` for isolated git branches
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

## pi

pi (`@mariozechner/pi-coding-agent`) is BYOK and provider-agnostic — point it at whichever
model you have API access to.

Basic session:

```bash
pulpo spawn pi-fix --workdir ~/repos/my-api -- pi "Fix the failing auth tests"
```

`pulpo usage --scan` picks up pi's own session files and reports the exact tokens *and* the
exact dollar cost pi itself computed from its model catalog — no `[rates.<model>]` entry
needed for pi. That's scan-only: there is no live per-session cost projection reader for pi
yet, so a pi session spawned through Pulpo won't appear in `pulpo usage` (the live
per-session gauge, with budgets and burn alerts) or get budget/burn enforcement — only in
`--scan`, which reads whatever pi has already written to its own session files, including a
still-running session.

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
pulpo spawn glm-risky --workdir ~/repos/my-api --worktree -d -- opencode
```

If you want the worktree-isolation rationale, see
[Worktrees](/guides/worktrees).

## Local Models (Ollama, LM Studio)

Run an open-weight model on one box on your tailnet and point agents on other machines at
it, so that class of session costs nothing instead of drawing down a paid plan.

Serve the model on the machine that has the GPU (or enough RAM):

```bash
ollama serve
ollama pull qwen3-coder
```

Ollama exposes an OpenAI-compatible endpoint on port `11434`. From any other machine on your
tailnet, that's reachable at:

```
http://<tailnet-host>:11434/v1
```

(swap in the Tailscale MagicDNS name of the machine running Ollama; LM Studio's local server
works the same way, on its own port.) Point your agent's OpenAI-compatible base URL setting
at that address and spawn it through Pulpo like anything else:

```bash
pulpo spawn local-fix --workdir ~/repos/my-api --worktree -d -- <agent-command>
```

Pulpo turns tokens into dollars by matching the model name against a rate table — today,
that only happens for the Claude Code reader (`~/.claude` session files). A locally-served
model has no built-in rate, so without a config entry, that session's cost reports as
**withheld**, not zero. Add an explicit override so it prices at $0 instead:

```toml
[rates."qwen3-coder"]
input = 0.0
output = 0.0
```

For any agent Pulpo already reads exact usage for, this makes local-model sessions show an
honest `$0.00` in `pulpo usage`, sitting next to whatever your paid-pool sessions actually
cost — instead of a blank "cost withheld" gap.

## Common Patterns

### Add a worktree

```bash
pulpo spawn my-task --workdir ~/repos/my-api --worktree -d -- <agent-command>
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
- [Config Reference](/reference/config) — see `[rates.<model>]`
- [Nightly Code Review](/guides/nightly-code-review)
- [Parallel Agents On One Repo](/guides/parallel-agents-one-repo)
- [Worktrees](/guides/worktrees)
