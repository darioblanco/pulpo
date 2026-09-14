# 0007. Rust stays for now; a Go rewrite is deferred until after a real dogfood test

- **Status:** Accepted
- **Date:** 2026-09-13 (owner's call)

## Context

Whether `pulpod`/`pulpo` should eventually move from Rust to Go has come up as a
recurring background question — Go's simpler concurrency model, faster iterative
compiles, and larger hiring pool are real considerations for a project whose surface
area (daemon, CLI, watchdog, harness adapters) is still growing. Against that: Rust is
already delivering concretely for this project specifically — `forbid(unsafe_code)`
workspace-wide, a single embedded-web-UI binary via `rust-embed`, a 98%-covered
codebase (ADR [0003](0003-scenario-tests-as-behavior-gate.md)), and a harness-adapter
model (ADR [0001](0001-hook-driven-agent-state.md)) that leans on Rust's trait system
for the `HarnessAdapter`/`Backend` abstractions. A rewrite of that surface is not a
small decision to make speculatively.

## Decision

We will **stay on Rust** for now, and **defer** any decision to rewrite in Go until
after a real dogfood test: running the current Rust implementation as the owner's
actual daily driver for a meaningful stretch, and documenting any concrete friction
(compile times, contributor onboarding cost, cross-compilation pain, or anything else)
from that real use — not from speculation about what a rewrite might fix.

## Consequences

- No rewrite work is scheduled. Current investment (harness adapters, the scenario
  suite, coverage tooling, the `rust-embed` single-binary distribution model) stays
  fully intact and continues to receive normal maintenance.
- If the dogfood test does surface real friction, a future ADR should supersede this
  one with a concrete migration plan and rationale grounded in what actually went
  wrong — not reopen the question from scratch.
- This ADR exists so the "should we rewrite in Go" question has a recorded answer and
  doesn't get re-litigated informally every time it comes up before there's new
  evidence.
