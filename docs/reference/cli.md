# CLI Reference

Top-level commands:

```text
pulpo spawn [OPTIONS] [PROMPT...]
pulpo list
pulpo logs <NAME> [--follow]
pulpo attach <NAME>
pulpo input <NAME> [TEXT]
pulpo kill <NAME>
pulpo delete <NAME>
pulpo resume <NAME>
pulpo interventions <NAME>
pulpo culture [--session] [--kind] [--repo] [--context] [--get] [--delete] [--push]
pulpo nodes
pulpo schedule <install|list|pause|resume|remove>
pulpo ui
```

## Spawn options

| Flag | Description | Providers |
|------|-------------|-----------|
| `--workdir <PATH>` | Working directory (defaults to current directory) | All |
| `--name <NAME>` | Session name (auto-generated if omitted) | All |
| `--provider <NAME>` | Agent provider (claude, codex, gemini, opencode) | All |
| `--auto` | Run in autonomous mode (fire-and-forget) | All |
| `--ink <NAME>` | Ink preset from config | All |
| `--unrestricted` | Disable all safety guardrails | Claude, Gemini |
| `--model <MODEL>` | Model override (e.g. opus, sonnet) | Claude, Codex, Gemini |
| `--worktree` | Use git worktree isolation (see below) | Claude |
| `--system-prompt <TEXT>` | System prompt to append | Claude |
| `--allowed-tools <TOOLS>` | Explicit allowed tools (comma-separated) | Claude |
| `--max-turns <N>` | Maximum agent turns before stopping | Claude |
| `--max-budget <USD>` | Maximum budget in USD before stopping | Claude |
| `--output-format <FMT>` | Output format (e.g. json, stream-json) | Claude, Gemini, OpenCode |

## Worktree isolation

The `--worktree` flag enables git worktree isolation (Claude only). Each session gets its own git worktree — an isolated copy of the repo on a separate branch — so the agent's changes don't interfere with your working tree or other sessions.

This is essential when running multiple agents on the same repository concurrently:

```bash
# Two agents working on the same repo in parallel, each in their own branch
pulpo spawn --worktree --workdir ~/myproject "add caching layer"
pulpo spawn --worktree --workdir ~/myproject "refactor auth module"

# When agents finish, review and merge the branches
git branch                    # see worktree branches
git merge <session-name>      # merge when ready
```

Without `--worktree`, all agents edit the same files simultaneously. For a single session, `--worktree` is optional — omit it if you want the agent to work directly in your tree.

While `--worktree` is Claude-only, other providers can work in a worktree that Claude created by pointing `--workdir` at it. Claude creates worktrees at `<repo>/.claude/worktrees/<session-name>`:

```bash
# Claude creates an isolated worktree
pulpo spawn --worktree --workdir ~/myproject "scaffold the API"
# Once the worktree exists, point other providers at it
pulpo spawn --provider codex --workdir ~/myproject/.claude/worktrees/<session-name> "write tests"
```

For exact options, run:

```bash
pulpo --help
pulpo <command> --help
```
