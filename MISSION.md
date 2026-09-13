# Mission

Pulpo runs any coding agent — Claude Code, Codex, pi, or any other terminal command — as a
durable session on a machine you own.

It provides:
- state learned from the agent's own hooks (Claude Code, Codex, pi), not scraped terminal
  output,
- exact usage metering — token counts read from the agent's own session files, costed and
  rolled up per repo,
- a spend cap per session or schedule that alerts at 80% and stops at 100%, not a post-hoc
  invoice,
- durable sessions with explicit lifecycle state that survive a reboot and resume the same
  conversation,
- cron-based schedules and per-session git worktrees, so unattended and parallel work don't
  collide,
- one webhook per lifecycle/intervention/budget event, delivered to infrastructure you
  already run,
- and interface-agnostic control via API, CLI, and web UI.

Pulpo is infrastructure, not a prompt framework, IDE, or agent planner — and not a fleet
control plane: each node is single-node and sovereign, governing its own sessions
standalone, with no cross-node orchestration.

Its job is to let you run any coding agent on infrastructure you control, know exactly
what it costs, cap it before it costs too much, and pick the same session back up.

## Non-Goals

- Defining the "best" prompting methodology or command presets
- Replacing specialized local agent UX tools
- Competing with hosted agent products on model quality or cloud UX
- Building a monolithic all-in-one platform or a multi-node control plane
