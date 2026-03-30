# Mission

Pulpo is a self-hosted control plane for background coding agents on your own
machines.

It provides:
- durable sessions with explicit lifecycle state,
- supervision and recovery when agents are running unattended,
- policy and safety guardrails for local and remote execution,
- and interface-agnostic control via API, CLI, and web UI.

Pulpo is infrastructure, not a prompt framework, IDE, or agent planner.

Its job is to let you run any coding agent on infrastructure you control, check
status from anywhere, and recover cleanly when things go wrong.

## Non-Goals

- Defining the "best" inks or prompting methodology
- Replacing specialized local agent UX tools
- Competing with hosted agent products on model quality or cloud UX
- Building a monolithic all-in-one platform
