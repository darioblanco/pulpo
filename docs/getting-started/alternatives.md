# Alternatives And Comparisons

This page is category-based, not a fake head-to-head. Different tools solve different layers of
"agents cost money and need somewhere to run" — the question is which layer you actually need.

Descriptions below are based on each project's own docs/site as of 2026-07-04, re-checked while
writing this page. Where a claim from an earlier version of this page couldn't be re-verified, it
was dropped rather than repeated.

## Comparison Principles

The fair questions to ask are:

1. Does it read usage after the fact, or run the session and enforce a limit while it happens?
2. Where does the runtime live — a vendor's cloud, your Mac, or a machine you administer?
3. Is it built around one agent vendor, or does it run whatever CLI you point it at?
4. What happens when the work is unattended and something goes wrong at 2 a.m.?

## Category 1: Cost Readers (ccusage And Vendor `/usage` Pages)

**Examples:** [ccusage](https://ccusage.com/), vendor account-usage dashboards.

### What This Category Is Best At

ccusage reads local usage logs and turns them into cost/token reports. It has grown well beyond
Claude Code: its own docs list Claude Code, Codex, OpenCode, Gemini CLI, GitHub Copilot CLI, and
close to a dozen other CLIs, all read from local files, offline-capable, free. If you just want a
report on the machine you're sitting at, it's an excellent, purpose-built tool for exactly that.

### Where Pulpo Differs

Pulpo's `--scan` is the same trick — read-only, zero setup, no data leaves the machine — but for
fewer agents today (Claude Code, Codex, and pi, with exact tokens and cost where the agent
exposes them). What Pulpo adds on top:

- **It also runs the sessions.** ccusage only reads what already happened; Pulpo can spawn, meter,
  and enforce a budget on the same session, so a cap actually stops something.
- **Cross-machine.** ccusage is explicitly local/single-machine, with no aggregation across
  systems. Pulpo's per-repo rollups and its signed-webhook event backbone are how you get one
  number across every machine you run, without a hosted aggregator in between.
- **Lifecycle, not just a report.** ccusage has no concept of a session, a budget, or an alert —
  Pulpo's watchdog turns "you overspent" into "the session stopped at 100%."

Use ccusage when you want a cost report on this machine, right now, and have no interest in
running or supervising anything. Use Pulpo when you also want the runtime.

## Category 2: Native Multi-Agent UX Tools

**Examples:** [Conductor](https://www.conductor.build/) (YC-backed Mac app, $22M Series A in
2026; runs Claude Code, Codex, and Cursor sessions in isolated git worktrees with a review/merge
UI), [Claude Code's built-in Remote Control](https://code.claude.com/docs/en/remote-control)
(bridges a local Claude Code session to claude.ai/code or the mobile app for push notifications
and remote input, outbound-only, no inbound ports), OpenAI's Codex desktop app (a command center
for running several Codex agents in parallel, with built-in worktrees), and terminal-native
session managers.

### What This Category Is Best At

The nicest available way to run and watch a handful of agents interactively on your own Mac —
native UI, no daemon to configure, tight integration with one vendor's or one OS's workflow,
worktree creation built in.

### Where Pulpo Differs

Pulpo doesn't compete on interactive UX; these tools are better at that specific job. The
difference is what each is *for*:

- **Command- and model-agnostic.** Remote Control only bridges Claude Code; Conductor covers
  three agents but stays a Mac app. Pulpo runs whatever terminal command you give it.
- **Self-hostable headless**, on Linux, on a spare box with no display — not tied to a local Mac
  install.
- **Cost metering and budget enforcement live in the daemon**, not the terminal app, so a session
  survives a closed laptop lid because the runtime was never tied to the app that launched it.
- **No vendor relay.** Remote Control's mobile bridge routes through Anthropic's servers; Pulpo's
  web UI talks to your own daemon over your own tailnet.

Use one of these when your problem really is "I want a nicer way to run a few agents
interactively on my Mac, right now." That's a real, well-served problem — Pulpo isn't trying to
win it.

## Category 3: The Agent CLIs Themselves

Claude Code, Codex, pi, Gemini CLI, Aider, Goose, OpenCode, and any other terminal coding agent
are not something Pulpo competes with — they're the layer Pulpo runs on top of.

`pulpo spawn` takes any command after `--`. The agent does the actual coding work; Pulpo wraps
that invocation with a durable lifecycle, exact usage metering (Claude Code, Codex, and pi so
far), budgets, and remote reachability. Switching agents, or running several at once, doesn't
change how you use Pulpo — see [Agent Examples](/guides/agent-examples).

## Category 4: Raw Infrastructure (tmux, cron, SSH, Docker Scripts)

### What This Category Is Best At

Maximum flexibility, no product opinion. If you want to hand-roll exactly the lifecycle behavior
you need, these primitives will let you build it.

### Where Pulpo Differs

This is what Pulpo formalizes, using the same primitives underneath:

- explicit session states (`active`, `idle`, `ready`, `lost`, `stopped`) instead of a tmux pane
  you have to remember the name of
- `cron`-driven scheduling wired to the same session objects, not a standalone script
- exact usage metering and budgets on every run, not something you'd script yourself against
  each agent's log format
- SSH access formalized into `pulpo attach` plus a web UI, reachable the same way from any node

If you're already comfortable maintaining that yourself, you may not need Pulpo. Most people find
the maintenance cost catches up once there's more than one agent or one machine involved.

## Where The Runtime Lives: Hosted Clouds Are A Different Question

Vendor-hosted background agents — [GitHub Copilot's coding
agent](https://github.com/features/copilot) (spins up a GitHub Actions VM, opens a PR) and
[Cursor's background agents](https://docs.cursor.com/en/background-agents) (cloud VMs you spin up
and merge from) — are a different trade entirely: zero infrastructure to run, in exchange for the
runtime, your repo access, and your usage data all living on the vendor's cloud. Pulpo's whole
premise runs the other way — runtime, credentials, and cost data stay on hardware you administer.
If a fully-hosted PR bot is what you want, the rest of this page doesn't apply to you.

## Quick Decision Guide

Use a cost reader (ccusage) when you want a report on this machine, right now, and don't need to
run or supervise anything.

Use a native multi-agent UX tool (Conductor, Remote Control, or similar) when you work on one Mac,
want the nicest interactive experience, and don't need budgets, remote access from other
machines, or a headless server.

Use a hosted agent cloud when you want zero infrastructure and are fine with the runtime and your
repo living on a vendor's servers.

Use raw tmux/cron/SSH when you want to assemble exactly what you need yourself and are willing to
maintain it.

Use Pulpo when you want to know what every agent, on every machine, is costing you; you want a
budget that actually stops a session; and you want the runtime to stay on infrastructure you
administer.

## Sources

- ccusage: <https://ccusage.com/> · <https://github.com/ryoppippi/ccusage>
- Conductor: <https://www.conductor.build/> · <https://docs.conductor.build/>
- Claude Code Remote Control: <https://code.claude.com/docs/en/remote-control>
- OpenAI Codex app: <https://openai.com/index/introducing-the-codex-app/>
- GitHub Copilot coding agent: <https://github.com/features/copilot>
- Cursor background agents: <https://docs.cursor.com/en/background-agents>

Checked 2026-07-04.
