# Use Cases

Pulpo's value shows up differently depending on what's actually bothering you. These are the
concrete jobs it does today, roughly in the order you'll run into them.

## 1. "How Much Are My Agents Actually Costing Me?"

You run Claude Code, Codex, or pi across a few repos and maybe more than one account, and the
only cost picture you have is a vendor's `/usage` page — one account, one vendor, checked after
the fact.

```bash
pulpo usage --scan
```

This reads each agent's own on-disk history (`~/.claude`, `~/.codex`, `~/.pi`) and reports spend
and tokens by agent, model, and repo, unified across vendors — no daemon, nothing routed through
Pulpo first. Git worktrees and subdirectories roll up to their origin repo, so "this repo" means
the whole thing, not one checkout.

Best docs to read next:

- [Quickstart](/getting-started/quickstart)
- [Why Pulpo](/getting-started/why-pulpo)
- [CLI Reference](/reference/cli)

## 2. The Daily Driver: An Agent That Survives Your Laptop Lid

You want an agent to keep working after wifi drops, an SSH session dies, or you just close the
lid — then check back in from wherever you are.

```bash
pulpo spawn fix --workdir ~/repos/api -- claude -p "Fix the failing auth tests"
# Ctrl-b d to detach — the session keeps running
pulpo attach fix
```

The session runs in `tmux` on a machine you leave on, independent of your terminal or laptop's
power state. Reattach over SSH from a laptop, or check status from a phone through the web UI
(installable as a PWA).

Best docs to read next:

- [Control Your Agents From Anywhere](/guides/remote-control)
- [Session Lifecycle](/operations/session-lifecycle)
- [Quickstart](/getting-started/quickstart)

## 3. Parallel Agents On One Repo, Without Collisions

You want two or three agents working the same repo at once — one on the frontend, one on the
backend — without them clobbering each other's working tree.

```bash
pulpo spawn frontend --workdir ~/repo --worktree -- claude -p "redesign the sidebar"
pulpo spawn backend  --workdir ~/repo --worktree -- codex "optimize the query path"
```

Each session gets its own git worktree and branch. This isn't orchestration — the agents don't
coordinate or hand off work to each other — it's isolation plus metering: every session is still
a durable, cost-tracked object you can inspect independently.

Best docs to read next:

- [Parallel Agents On One Repo](/guides/parallel-agents-one-repo)
- [Worktrees Guide](/guides/worktrees)

## 4. Unattended, Scheduled Jobs That Can't Run Away

You want a nightly review, a weekly dependency scan, or a recurring migration rehearsal — run
unattended, with a ceiling so a run with nobody watching can't quietly burn a chunk of your
weekly quota by morning.

Give the recurring job a budget directly on the schedule:

```bash
pulpo schedule add nightly-review "0 3 * * *" --workdir ~/repo --budget-cost 5.0 \
  -- claude -p "Review this repository for bugs, regressions, and missing tests."
```

The watchdog alerts at 80% of the schedule's `budget_cost_usd` and stops the session at
100% — you find a `stopped` session with a clear reason in the morning, not a surprise on
the invoice.

Best docs to read next:

- [Nightly Code Review](/guides/nightly-code-review)
- [Configuration Guide](/guides/configuration)

## 5. The 2 A.M. Runaway

An agent gets stuck in a retry loop, or a job with no budget set starts burning tokens fast, and
nobody is watching.

A flat cost cap (case 4) still catches sessions that have one. For the ones that don't, or where
the danger is the *rate* rather than the total, the burn-velocity governor watches that instead:

```toml
[watchdog]
burn_ceiling_usd_per_hour = 20.0
burn_action = "alert"   # "stop" to opt into auto-kill
```

Point a webhook at the alert so it reaches you, not just the dashboard:

```toml
[[webhooks]]
name = "phone"
url = "https://example.com/hooks/pulpo"
events = ["usage_alert.*", "intervention.*"]
min_severity = "warn"
```

Alerting is on by default; auto-stop is opt-in. Combined with `--budget-cost` on the spawn
itself, this is the closest thing to a breaker an unattended agent gets.

Best docs to read next:

- [Configuration Guide](/guides/configuration)
- [Config Reference](/reference/config)

## 6. Sovereign Infrastructure: Private Repos, Private Data

Your repos, internal APIs, or credentials aren't a fit for a vendor-managed cloud sandbox — the
agent has to run on your network, and so does your usage data.

```toml
[node]
name = "mac-mini"
bind = "tailscale"
```

Pulpo runs as a single binary on a box you own, reachable only over your tailnet. Usage and cost
data are read from local files and never leave that machine unless you point `[[webhooks]]`
somewhere yourself. Secrets are stored per-node, injected into the session's own process
environment, and never returned by the API.

Best docs to read next:

- [Private Infrastructure With Tailscale And Secrets](/guides/private-infra-with-tailscale)
- [Secrets](/guides/secrets)
- [Discovery Guide](/guides/discovery)

## Quick Decision Table

| If you need... | Start here |
| --- | --- |
| An instant answer to "what are my agents costing me" | [Quickstart](/getting-started/quickstart) |
| An agent that survives a closed laptop lid | [Control Your Agents From Anywhere](/guides/remote-control) |
| Multiple agents on one repo without collisions | [Parallel Agents On One Repo](/guides/parallel-agents-one-repo) |
| A recurring job that can't overspend | [Nightly Code Review](/guides/nightly-code-review) |
| Alerts before a runaway session gets expensive | [Configuration Guide](/guides/configuration) |
| Agents near private repos or internal APIs | [Private Infrastructure With Tailscale And Secrets](/guides/private-infra-with-tailscale) |
| An objective comparison with alternatives | [Alternatives And Comparisons](/getting-started/alternatives) |
