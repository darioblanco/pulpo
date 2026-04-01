# Nightly Code Review

This recipe shows how to make Pulpo run a recurring overnight review session on
infrastructure you control.

It is a good example because it combines:

- scheduled execution
- reusable inks
- durable session state
- morning-after inspection through CLI, UI, or notifications

## What This Means

"Nightly code review" does not mean Pulpo itself performs a special GitHub
review action.

It means Pulpo starts a scheduled agent session with a review-oriented command,
tracks that session like any other, and lets you inspect the result the next
morning.

Typical outcomes:

- a summary in session output
- a generated branch or PR if the agent chooses to create one
- a completed `ready` session you can inspect from the dashboard or CLI

## When This Recipe Fits

Use this when you want:

- overnight code review or audit passes
- recurring review on a stable repo
- an unattended background workflow you can check in the morning

This recipe is especially useful for the "operator running recurring agent work"
ICP described in [Use Cases](/getting-started/use-cases).

## Option 1: Direct Scheduled Command

Start with the simplest form:

```bash
pulpo schedule add nightly-review "0 3 * * *" \
  --workdir ~/repos/my-api \
  -- claude -p "Review this repository for bugs, regressions, risky changes, and missing tests. Summarize findings clearly."
```

What this does:

1. Adds a schedule named `nightly-review`
2. Runs every day at `03:00` in the daemon's machine timezone
3. Starts a fresh Pulpo session in `~/repos/my-api`
4. Uses your review prompt as the session command

Each schedule fire creates a fresh timestamped session such as:

```text
nightly-review-20260331-0300
```

## Option 2: Ink-Based Nightly Review

This is the better long-term version because it keeps the reusable review logic
in one place.

Add an ink to `~/.pulpo/config.toml`:

```toml
[inks.nightly-review]
description = "Nightly review focused on bugs, regressions, and missing tests"
command = "claude -p 'Review this repository for bugs, regressions, risky changes, and missing tests. Summarize findings clearly.'"
runtime = "docker"
secrets = ["GH_WORK", "ANTHROPIC_KEY"]
```

Then schedule the ink:

```bash
pulpo schedule add nightly-review "0 3 * * *" \
  --workdir ~/repos/my-api \
  --ink nightly-review
```

Why this version is better:

- the schedule stays short
- you can refine the review command in one place
- runtime and secrets travel with the review blueprint
- the same ink can be reused across multiple repos or schedules

## Morning Check-In

The next morning, inspect what happened:

```bash
pulpo schedule list
pulpo ls
pulpo logs nightly-review-20260331-0300
```

In the web UI, you can:

- open the sessions view
- filter for `ready`, `idle`, or `lost`
- inspect output, branch badges, PR badges, and error indicators

## Variations

### Run In tmux

If you do not need container isolation, omit the ink runtime or set:

```toml
runtime = "tmux"
```

### Run In Docker

If the task needs stronger isolation or unrestricted agent permissions, use:

```toml
runtime = "docker"
```

This is often the better choice for unattended review or audit tasks.

### Review On Another Machine

Run the schedule against a specific node:

```bash
pulpo schedule add nightly-review "0 3 * * *" \
  --node mac-mini \
  --workdir ~/repos/my-api \
  --ink nightly-review
```

Or let Pulpo choose automatically:

Only use a remote-targeted schedule when the request goes to the master and the target node has the repo at the same path, or your operational setup guarantees the right path exists there.

## Recommended Companion Features

This recipe gets stronger when combined with:

- [Secrets](/guides/secrets) for API keys and repo credentials
- [Discovery Guide](/guides/discovery) for multi-node fleets
- notifications so you know when the overnight run is `ready`, `stopped`, or `lost`

## Related Commands

```bash
pulpo schedule list
pulpo schedule pause <id>
pulpo schedule resume <id>
pulpo schedule remove <id>
pulpo ink list
pulpo ink get nightly-review
```

## Summary

The value of this workflow is simple:

- you define the review once
- Pulpo runs it unattended
- the run becomes a managed session instead of a disposable shell command
- you check the result in the morning without guessing what happened
