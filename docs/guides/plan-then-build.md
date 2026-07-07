# Plan Then Build

Planning and implementation have different economics: a frontier model is worth the cost
to think through a hard problem once, but paying frontier-model rates to type out the
resulting diff is waste — a subscription-tier agent (or a different vendor entirely)
implements a written plan just as well.

`pulpo handoff` connects the two steps into one command: it spawns a new session in the
exact same place the first one left off — same working directory, same git worktree if
it had one — without a manual `cd`/branch dance.

## The Flow

Spawn a planning session with a frontier model, budgeted, in its own worktree:

```bash
pulpo spawn plan-auth -w --workdir ~/repos/my-api --budget-cost 5 \
  -- claude --model opus -p "Plan the auth refactor. Write PLAN.md."
```

Wait for the ready/idle alert (web push, webhook, or `pulpo list`), then hand off to a
second agent — a different model, a subscription plan, even a different vendor entirely:

```bash
pulpo handoff plan-auth -- codex "implement PLAN.md"
```

The new session runs in the exact same directory as `plan-auth`, including its git
worktree if it had one. Nothing is copied or re-checked-out; the second agent picks up
the first agent's working tree as-is.

## What Pulpo Does And Doesn't Do

Pulpo never opens, parses, or interprets `PLAN.md` — or any other file the first agent
wrote. It guarantees exactly one thing: the next command starts where the last one left
off. Any agent pair works — two instances of the same agent, a frontier model paired
with a small local model, or two completely different vendors. Pulpo doesn't care what's
in the plan or what reads it.

Both sessions are metered independently (`pulpo usage`), so the planning session's
frontier-model cost and the build session's implementation cost show up separately — you
can see exactly what the "thinking" step cost versus the "typing" step.

## Naming And Cleanup

If you don't name the handoff session, Pulpo auto-generates one: `plan-auth-2`,
`plan-auth-3`, and so on, skipping any name already in use.

If the source session used a worktree, the handoff session **adopts** it — no second
worktree or branch is created. The shared worktree is only reclaimed once *every*
session referencing it has stopped, so purging `plan-auth` right after handoff never
deletes work the build session still needs:

```bash
pulpo stop plan-auth --purge         # worktree survives — implement-auth still needs it
pulpo stop implement-auth --purge    # now nothing references it
pulpo cleanup                        # reclaims it (also a safe no-op if already gone)
```

See [Worktrees](/guides/worktrees) for the full isolation model.

## Full Example

```bash
pulpo spawn plan-auth -w --workdir ~/repos/my-api --budget-cost 5 \
  -- claude --model opus -p "Plan the auth refactor. Write PLAN.md."

# ...plan-auth goes idle/ready...

pulpo handoff plan-auth -- codex "implement PLAN.md"

# ...the build session finishes...

pulpo usage   # cost for both the planning and the build session, shown separately
```

Codex reports exact tokens and subscription quota rather than a dollar cost — see
[CLI Reference](/reference/cli#handoff) for the full flag list, and
[Why Pulpo](/getting-started/why-pulpo) for the metering model this builds on.
