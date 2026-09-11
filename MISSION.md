# Mission

Pulpo is the self-hosted meter and breaker box for background coding agents on your
own machines.

It provides:
- exact usage metering — what every agent session actually costs, across accounts and
  machines,
- budgets and a burn-rate governor that intervene before you blow a limit, not a
  post-hoc invoice,
- durable sessions with explicit lifecycle state,
- hook-driven supervision and recovery when agents are running unattended,
- and interface-agnostic control via API, CLI, and web UI.

Pulpo is infrastructure, not a prompt framework, IDE, or agent planner — and not a
fleet control plane: each node meters and governs its own sessions standalone, with no
cross-node orchestration.

Its job is to let you run any coding agent on infrastructure you control, know exactly
what it costs, check status from anywhere, and recover cleanly when things go wrong.

## Non-Goals

- Defining the "best" prompting methodology or command presets
- Replacing specialized local agent UX tools
- Competing with hosted agent products on model quality or cloud UX
- Building a monolithic all-in-one platform
