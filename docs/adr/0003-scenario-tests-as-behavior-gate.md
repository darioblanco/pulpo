# 0003. Scenario tests as the behavior gate; unit tests for logic; 98% coverage as a decay guard

- **Status:** Accepted
- **Date:** 2026-09-13 (PR [#114](https://github.com/darioblanco/pulpo/pull/114); PR
  [#107](https://github.com/darioblanco/pulpo/pull/107) closed as rejected)

## Context

2,300+ unit tests at 98% line coverage still missed four real user-facing bugs (the
idle threshold was never read, hooks only reached the default port, interventions
deleted a *shared* worktree, a `Ready` session bounced back on resume). Every one of
these was a gap *between* units (config → watchdog, harness → session manager,
intervention → worktree cleanup) — exactly the kind of gap a `MockBackend`-based "flow"
test asserts past, because the mock never disagrees with the code driving it.

Separately, and in the same week, PR #107 proposed lowering the enforced coverage gate
in `make coverage-rust` from 98% to 90% (matching the web gate), arguing that the last
eight points were costing agents and reviewers real time keeping the number up on
plumbing code, not on behavior that mattered — coverage at 98.5% at the time, so
nothing would have changed immediately, but the marginal cost of maintaining the last
few points was already visible.

## Decision

We will keep two different kinds of test for two different jobs, and reject lowering
the coverage floor:

- **Unit tests** — pure logic: parsing, cron math, rate tables, command-string
  rewriting, state-transition functions, request validation. TDD still applies: write
  the test first, watch it fail, implement, refactor, keep it green.
- **Scenario tests** (`crates/pulpo-e2e/`, PR #114) — every *documented user-facing
  behavior* (session lifecycle, harness adapters, the watchdog, schedules, worktrees),
  proven against a **real `pulpod` daemon and a real (private) tmux server**, driven
  through the real `pulpo` CLI and HTTP API, with only the *agent* faked
  (`fake-claude`, a small binary imitating Claude Code's CLI/hook surface, scripted by
  `FAKE_AGENT_SCENARIO`). No mock backend, no mock store, no mock harness adapter — a
  scenario test can't pass just because a mock agreed with the code under test.
  Mock-backend flow tests for *new* user-facing session/watchdog/harness/schedule work
  are no longer accepted as a substitute; existing `MockBackend` unit tests for pure
  branch coverage (a handler's error paths, a store query's edge cases) are unaffected.
- **Coverage stays at 98%** (`make coverage-rust`), and PR #107's proposal to lower it
  to 90% was closed without merging. Coverage is reframed explicitly as a **decay
  guard**, not the primary correctness signal: it still catches an untested branch, but
  it never catches a wrong *interaction* between two correctly-tested units — that's
  what the scenario suite is for. Keeping the number high still has a job to do; it
  just isn't the job people were implicitly asking it to do.

## Consequences

- `crates/pulpo-e2e` is dev-only: excluded from the coverage run (`--exclude
  pulpo-e2e`) and from release builds — it needs pre-built binaries and a real tmux
  server, and instrumenting it would need both. It has its own CI job (`e2e`), run
  manually before a PR that touches session/watchdog/harness/schedule behavior, and
  takes roughly a minute (S8 waits for a real cron-minute boundary), which is why it is
  not part of `make ci`/the pre-commit hook.
- A PR adding or changing session/watchdog/harness/schedule behavior now needs a new or
  extended scenario, not just unit tests, before it can be considered proven correct.
- The coverage bar for new code is unchanged from before this decision: every new
  function, branch, and error path still gets a test — this ADR did not relax that,
  it declined to relax the aggregate floor either.
- Two suites to run instead of one: `make test`/`make ci` (fast, every commit) plus
  `make e2e` (slower, manual/pre-PR, its own CI job) — a slightly higher process cost
  in exchange for tests that can't be satisfied by a mock agreeing with itself.
