# Nightly Code Review

This recipe shows how to make Pulpo run a recurring overnight review session on
infrastructure you control.

It is a good example because it combines:

- scheduled execution
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

## Direct Scheduled Command

```bash
pulpo schedule add nightly-review "0 3 * * *" \
  --workdir ~/repos/my-api \
  --secret GH_WORK --secret ANTHROPIC_KEY \
  -- claude -p "Review this repository for bugs, regressions, risky changes, and missing tests. Summarize findings clearly."
```

What this does:

1. Adds a schedule named `nightly-review`
2. Runs every day at `03:00` in the daemon's machine timezone
3. Starts a fresh Pulpo session in `~/repos/my-api`, with the given secrets injected
4. Uses your review prompt as the session command

Each schedule fire creates a fresh timestamped session such as:

```text
nightly-review-20260331-0300
```

## With a Cost Budget

Give the recurring job a cap so a runaway review can't burn unattended:

```bash
pulpo schedule add nightly-review "0 3 * * *" \
  --workdir ~/repos/my-api \
  --budget-cost 5.0 \
  -- claude -p "Review this repository for bugs, regressions, risky changes, and missing tests. Summarize findings clearly."
```

The watchdog alerts at 80% of the schedule's `budget_cost_usd` and stops the session at
100% — you find out about a runaway overnight job instead of a surprise bill.

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

### Isolate The Review In A Worktree

If you want the unattended run to work in an isolated checkout (so it cannot disturb your main working tree), add `--worktree` to the schedule:

```bash
pulpo schedule add nightly-review "0 3 * * *" \
  --workdir ~/repos/my-api \
  --worktree \
  -- claude -p "Review this repository for bugs, regressions, risky changes, and missing tests. Summarize findings clearly."
```

Each run gets a fresh git worktree on its own branch, cleaned up when the session is stopped.

### Review On Another Machine

Schedules always fire on the node that holds them — there is no remote dispatch. To put
this schedule on a different box, point the CLI at that node's `pulpod` with the global
`--node` flag; the schedule is created directly there and fires locally:

```bash
pulpo --node mac-mini schedule add nightly-review "0 3 * * *" \
  --workdir ~/repos/my-api \
  -- claude -p "Review this repository for bugs, regressions, risky changes, and missing tests. Summarize findings clearly."
```

Make sure the workdir path exists on `mac-mini`, not just on the machine you're typing from.

## Recommended Companion Features

This recipe gets stronger when combined with:

- [Secrets](/guides/secrets) for API keys and repo credentials
- [Discovery Guide](/guides/discovery) if you run more than one machine
- notifications so you know when the overnight run is `ready`, `stopped`, or `lost`

## Related Commands

```bash
pulpo schedule list
pulpo schedule pause <id>
pulpo schedule resume <id>
pulpo schedule remove <id>
```

## Summary

The value of this workflow is simple:

- you define the review once
- Pulpo runs it unattended
- the run becomes a managed session instead of a disposable shell command
- you check the result in the morning without guessing what happened
