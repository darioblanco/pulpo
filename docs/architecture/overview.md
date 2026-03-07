# Architecture Overview

Pulpo consists of:

- `pulpod`: daemon runtime + API + embedded UI
- `pulpo`: CLI client
- `tmux` backend abstraction for session execution
- SQLite store for lifecycle persistence

Control surfaces:

- CLI
- web UI
- REST/SSE API
- MCP mode (`pulpod mcp`)

Design intent:

- infrastructure/runtime layer, not prompt framework
- explicit session states and recovery semantics
- provider-agnostic operations surface

For deep architecture details, use:

- [SPEC.md](../../SPEC.md)
