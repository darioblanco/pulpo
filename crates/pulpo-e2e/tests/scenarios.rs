//! End-to-end scenario suite: a real `pulpod`, a real (private) tmux server, and a
//! fake harness (`fake-claude`) standing in for Claude Code. Every test here drives
//! the daemon exactly the way a user would — the `pulpo` CLI and the daemon's own
//! HTTP API — and asserts on real, observed state, never a mock. See
//! `crates/pulpo-e2e/src/lib.rs` for the harness these tests are built on,
//! `crates/pulpo-e2e/src/bin/fake-claude.rs` for the fake harness's contract, and
//! `AGENTS.md`'s testing section for the strategy this suite implements and how to
//! add a scenario.
//!
//! Run with `make e2e` (builds `pulpod`/`pulpo`/`fake-claude` first) or, once built,
//! `cargo test -p pulpo-e2e -- --test-threads=1` (serial — see the module doc in
//! `src/lib.rs` for why: each test boots its own daemon/tmux server, and running
//! several heavyweight real-process suites concurrently on a laptop is a documented
//! source of flakiness this project explicitly moved away from).

use std::time::Duration;

use pulpo_e2e::{
    Daemon, DaemonConfig, InterventionCode, SessionStatus, WebhookSink, init_git_repo,
    read_fake_argv, read_fake_env, temp_workdir, wait_for_fake_argv, wait_for_fake_codex_env,
    wait_for_fake_state,
};

const SHORT: Duration = Duration::from_secs(15);
const MEDIUM: Duration = Duration::from_secs(30);
const LONG: Duration = Duration::from_secs(90);

/// Write `<workdir>/pulpo-fake-scenario.txt` — see `fake-claude.rs`'s scenario
/// resolution order. Used by scenarios that need different behavior across a
/// resume (S3/S4/S5): the tmux command line is reused verbatim on resume, but this
/// file is read fresh by every new fake-claude process.
fn set_scenario(workdir: &std::path::Path, scenario: &str) {
    std::fs::write(workdir.join("pulpo-fake-scenario.txt"), scenario)
        .expect("write pulpo-fake-scenario.txt");
}

// ---------------------------------------------------------------------------
// S1 — spawn
// ---------------------------------------------------------------------------

#[test]
fn s1_spawn_reaches_working_with_harness_metadata() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "start,hang");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn("s1-spawn", &workdir, &[], &[&claude_str, "-p", "hello"]);
    assert!(
        output.status.success(),
        "pulpo spawn failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let session = daemon.wait_status("s1-spawn", SessionStatus::Working, SHORT);
    assert_eq!(session.harness.as_deref(), Some("claude"));

    let session = daemon.wait_for("s1-spawn", SHORT, |s| s.harness_session_id.is_some());
    assert!(session.harness_session_id.is_some());
    let session = daemon.wait_for("s1-spawn", SHORT, |s| s.harness_last_event_at.is_some());
    assert!(session.harness_last_event_at.is_some());

    // Parity with the retired scripts/e2e.sh smoke check: the usage endpoint/CLI
    // command works against a live daemon with at least one session on record.
    let usage = daemon.pulpo(&["usage"]);
    assert!(
        usage.status.success(),
        "pulpo usage failed: {}",
        String::from_utf8_lossy(&usage.stderr)
    );
}

// ---------------------------------------------------------------------------
// S2 — needs input
// ---------------------------------------------------------------------------

#[test]
fn s2_needs_input_then_input_resolves_it() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "start,prompt,needs_input,wait");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn(
        "s2-needs-input",
        &workdir,
        &[],
        &[&claude_str, "-p", "hello"],
    );
    assert!(output.status.success());

    let session = daemon.wait_status("s2-needs-input", SessionStatus::Waiting, SHORT);
    assert_eq!(
        session.status_reason.as_deref(),
        Some("needs_input:permission")
    );

    daemon.input("s2-needs-input", None);

    // The fake resolves the prompt (Working) then finishes the turn (Stop): Waiting
    // again, but with reason `idle` — no longer blocked on the human.
    let session = daemon.wait_for("s2-needs-input", SHORT, |s| {
        s.status == SessionStatus::Waiting && s.status_reason.as_deref() == Some("idle")
    });
    assert_eq!(session.status, SessionStatus::Waiting);
    assert_eq!(
        session.status_reason.as_deref(),
        Some("idle"),
        "needs_input reason should be cleared once the turn finishes: {:?}",
        session.status_reason
    );
}

// ---------------------------------------------------------------------------
// S3 — clean exit, then resume
// ---------------------------------------------------------------------------

#[test]
fn s3_clean_exit_then_resume_recreates_with_resume_flag() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    // `spend:0.25` (before `exit`) writes a real Claude-shaped transcript record —
    // see below for why the session must report that cost once it's Stopped.
    set_scenario(&workdir, "start,prompt,stop,spend:0.25,exit");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn(
        "s3-clean-exit",
        &workdir,
        &[],
        &[&claude_str, "-p", "hello"],
    );
    assert!(output.status.success());

    // The harness's own SessionEnded event fires (via the "exit" step) while the
    // fake process is still blocked on that hook's HTTP round trip — i.e. strictly
    // before the wrapped process actually exits and `wrap_command` writes the
    // `.code` marker — so `apply_harness_event` leaves the status alone rather than
    // fabricating a transition (see ADR 0009 / `harness::transition_for_event`'s doc
    // comment). Once the fake process really does exit, the wrapper writes the exit
    // markers and its own shell exits right behind it — no more lingering fallback
    // shell — so tmux tears the session down immediately and the very next lazy
    // dead-backend check (this `wait_status` poll) resolves it straight to `Done`
    // with reason `exited`. There is no `Ready` in between anymore.
    let session = daemon.wait_status("s3-clean-exit", SessionStatus::Done, SHORT);
    assert_eq!(session.status_reason.as_deref(), Some("exited"));
    assert_eq!(session.exit_code, Some(0));
    let harness_session_id = session
        .harness_session_id
        .clone()
        .expect("harness session id should be known before exit");

    // Regression: exact usage used to be refreshed only by the watchdog's idle
    // sweep, which never revisits a session once it leaves Working/Waiting — a
    // session that reached Done before the next tick kept reporting no cost at
    // all, even though its transcript (the `spend:0.25` step above) had the data
    // the whole time. The dead-backend transition into `Done` now runs the exact
    // reader itself (`watchdog::refresh_exact_usage`), so the cost should already
    // be there — poll briefly rather than asserting instantly, since it's recorded
    // asynchronously relative to this test's own polling of `status`.
    let session = daemon.wait_for("s3-clean-exit", SHORT, |s| {
        s.metadata
            .as_ref()
            .and_then(|m| m.get("session_cost_usd"))
            .and_then(|v| v.parse::<f64>().ok())
            .is_some_and(|cost| cost > 0.0)
    });
    assert_eq!(session.status, SessionStatus::Done);
    let cost: f64 = session
        .metadata
        .as_ref()
        .and_then(|m| m.get("session_cost_usd"))
        .and_then(|v| v.parse().ok())
        .expect("session_cost_usd should be recorded for a done session with a transcript");
    assert!(cost > 0.0, "expected session_cost_usd > 0, got {cost}");

    // Swap in a scenario that just stays up after starting, so the *resumed*
    // process is reliably observable as Working before it does anything else.
    set_scenario(&workdir, "start,hang");
    let before_resume_state = read_fake_state_or_panic(&workdir);
    let old_pid = before_resume_state["pid"].as_u64().expect("pid");

    daemon.resume("s3-clean-exit");

    let session = daemon.wait_status("s3-clean-exit", SessionStatus::Working, SHORT);
    assert_eq!(
        session.harness_session_id.as_deref(),
        Some(harness_session_id.as_str()),
        "resume must continue the same harness conversation, not start a new one"
    );

    // The fake writes its own state after every step it runs — confirms this is a
    // genuinely new process (`--resume <id>`, source: "resume" per Claude Code's own
    // SessionStart hook `source` field), not just pulpod's view looking right.
    let state = wait_for_fake_state(&workdir, SHORT, |s| {
        s["pid"].as_u64().is_some_and(|pid| pid != old_pid)
    });
    assert_eq!(state["resumed"], serde_json::json!(true));
    assert_eq!(state["source"], serde_json::json!("resume"));
    assert_eq!(state["session_id"], serde_json::json!(harness_session_id));
}

fn read_fake_state_or_panic(workdir: &std::path::Path) -> serde_json::Value {
    pulpo_e2e::read_fake_state(workdir)
        .unwrap_or_else(|| panic!("no pulpo-fake-state.json found under {workdir:?}"))
}

// ---------------------------------------------------------------------------
// S4 — lost + resume
// ---------------------------------------------------------------------------

#[test]
fn s4_tmux_server_lost_then_resume_reactivates() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "start,hang");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn("s4-lost", &workdir, &[], &[&claude_str, "-p", "hello"]);
    assert!(output.status.success());
    let session = daemon.wait_status("s4-lost", SessionStatus::Working, SHORT);
    let harness_session_id = session
        .harness_session_id
        .clone()
        .expect("harness session id should be known");

    daemon.kill_tmux_server();

    daemon.wait_status("s4-lost", SessionStatus::Lost, MEDIUM);

    daemon.resume("s4-lost");

    let session = daemon.wait_status("s4-lost", SessionStatus::Working, SHORT);
    assert_eq!(
        session.harness_session_id.as_deref(),
        Some(harness_session_id.as_str())
    );
}

// ---------------------------------------------------------------------------
// S5 — daemon restart
// ---------------------------------------------------------------------------

#[test]
fn s5_daemon_restart_preserves_sessions_and_auto_resumes_lost_ones() {
    // The watchdog must not race the second half of this test (kill tmux, then
    // restart before the *old* daemon's own background loop notices and marks the
    // session Lost on its own) — a slow check interval keeps this deterministic
    // without racing wall-clock timing against the watchdog tick.
    let mut daemon = Daemon::start(DaemonConfig {
        check_interval_secs: 300,
        ..DaemonConfig::default()
    });
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    // Part 1: restart with tmux untouched — the session must survive unchanged.
    let (_dir_a, workdir_a) = temp_workdir();
    set_scenario(&workdir_a, "start,hang");
    let output = daemon.spawn(
        "s5-restart-alive",
        &workdir_a,
        &[],
        &[&claude_str, "-p", "hello"],
    );
    assert!(output.status.success());
    daemon.wait_status("s5-restart-alive", SessionStatus::Working, SHORT);

    daemon.restart_daemon();

    let session = daemon.wait_status("s5-restart-alive", SessionStatus::Working, SHORT);
    assert_eq!(session.name, "s5-restart-alive");

    // Part 2: kill tmux *then* restart — auto-resume-at-startup must recreate the
    // backend, named after the session (never a stale `$N` id).
    let (_dir_b, workdir_b) = temp_workdir();
    set_scenario(&workdir_b, "start,hang");
    let output = daemon.spawn(
        "s5-restart-with-loss",
        &workdir_b,
        &[],
        &[&claude_str, "-p", "hello"],
    );
    assert!(output.status.success());
    daemon.wait_status("s5-restart-with-loss", SessionStatus::Working, SHORT);

    daemon.kill_tmux_server();
    daemon.restart_daemon();

    daemon.wait_status("s5-restart-with-loss", SessionStatus::Working, SHORT);
    // `backend_session_id` itself gets upgraded from a plain name to tmux's own
    // `$N` id shortly after creation (see `AGENTS.md`'s "Session IDs" note) — the
    // durable proof of "named after the session, not a stale $N id" is the actual
    // tmux session name, which never changes after creation.
    assert!(
        daemon.tmux_has_session("s5-restart-with-loss"),
        "auto-resume at startup must name the recreated tmux session after the \
         session, not a stale $N backend id"
    );
}

// ---------------------------------------------------------------------------
// S6 — budget breaker
// ---------------------------------------------------------------------------

#[test]
fn s6_budget_breaker_stops_session_and_delivers_webhook() {
    let webhook = WebhookSink::start();
    let daemon = Daemon::start(DaemonConfig {
        webhook_url: Some(webhook.url()),
        ..DaemonConfig::default()
    });
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "start,prompt,spend:2.50,hang");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn(
        "s6-budget",
        &workdir,
        &["--budget-cost", "1"],
        &[&claude_str, "-p", "hello"],
    );
    assert!(output.status.success());

    let session = daemon.wait_status("s6-budget", SessionStatus::Done, MEDIUM);
    assert_eq!(
        session.intervention_code,
        Some(InterventionCode::BudgetExceeded)
    );
    assert_eq!(session.status_reason.as_deref(), Some("budget_exceeded"));

    let delivered = webhook.wait_for_event(MEDIUM);
    let delivered_text = delivered.to_string();
    assert!(
        delivered_text.contains("s6-budget") || delivered_text.contains("budget"),
        "expected the webhook payload to reference the session or the budget \
         intervention, got: {delivered_text}"
    );
}

// ---------------------------------------------------------------------------
// S7 — idle timeout kill (harness session) + idle-threshold-0 (generic command)
// ---------------------------------------------------------------------------

#[test]
fn s7_idle_timeout_kills_session_with_intervention() {
    let daemon = Daemon::start(DaemonConfig {
        idle_threshold_secs: 1,
        idle_timeout_secs: 2,
        idle_action: "kill",
        check_interval_secs: 1,
        ..DaemonConfig::default()
    });
    let (_dir, workdir) = temp_workdir();
    // Stays alive after Stop (an agent left sitting at its finished-turn prompt) —
    // Waiting, unchanging output, past the idle timeout.
    set_scenario(&workdir, "start,prompt,stop,hang");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn("s7-idle-kill", &workdir, &[], &[&claude_str, "-p", "hello"]);
    assert!(output.status.success());

    let session = daemon.wait_status("s7-idle-kill", SessionStatus::Done, LONG);
    assert_eq!(
        session.intervention_code,
        Some(InterventionCode::IdleTimeout)
    );
    assert_eq!(session.status_reason.as_deref(), Some("idle_timeout"));
}

#[test]
fn s7_idle_threshold_zero_disables_time_based_idle_for_generic_command() {
    let daemon = Daemon::start(DaemonConfig {
        idle_threshold_secs: 1,
        check_interval_secs: 1,
        ..DaemonConfig::default()
    });
    let (_dir, workdir) = temp_workdir();

    let output = daemon.spawn(
        "s7-idle-threshold-0",
        &workdir,
        &["--idle-threshold", "0"],
        &["sleep", "8"],
    );
    assert!(output.status.success());

    // Give the global 1s threshold several chances to (wrongly) fire before
    // asserting it never does for this session.
    std::thread::sleep(Duration::from_secs(5));
    let session = daemon
        .session("s7-idle-threshold-0")
        .expect("session should exist");
    assert_eq!(
        session.status,
        SessionStatus::Working,
        "idle_threshold_secs=0 must disable the time-based Working->Waiting transition"
    );

    // It still ends normally once the command exits — straight to Done, no
    // intermediate Ready (there's no harness here at all, just the ordinary
    // exit-marker/dead-backend classification).
    let session = daemon.wait_status("s7-idle-threshold-0", SessionStatus::Done, SHORT);
    assert_eq!(session.status_reason.as_deref(), Some("exited"));
}

// ---------------------------------------------------------------------------
// S8 — schedule fires
// ---------------------------------------------------------------------------

#[test]
fn s8_schedule_fires_and_runs_a_session() {
    let daemon = Daemon::start(DaemonConfig {
        scheduler_tick_secs: 2,
        ..DaemonConfig::default()
    });
    let (_dir, workdir) = temp_workdir();

    // A cron due every minute fires at most 60s after being added — tick_secs=2
    // keeps the *check* frequent; the wait below bounds worst case at that minute.
    let output = daemon.schedule_add("s8-every-minute", "* * * * *", &workdir, &["sleep", "5"]);
    assert!(
        output.status.success(),
        "pulpo schedule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        let sessions = daemon.sessions_all();
        if sessions
            .iter()
            .any(|s| s.name.starts_with("s8-every-minute-"))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "schedule never fired a session within 90s"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}

// ---------------------------------------------------------------------------
// S9 — worktrees
// ---------------------------------------------------------------------------

#[test]
fn s9_worktrees_distinct_survive_stop_and_removed_by_cleanup() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_repo_dir, repo_path) = temp_workdir();
    init_git_repo(&repo_path);

    let out1 = daemon.spawn("s9-wt-one", &repo_path, &["--worktree"], &["sleep", "300"]);
    assert!(out1.status.success());
    let out2 = daemon.spawn("s9-wt-two", &repo_path, &["--worktree"], &["sleep", "300"]);
    assert!(out2.status.success());

    let s1 = daemon.wait_for("s9-wt-one", SHORT, |s| s.worktree_path.is_some());
    let s2 = daemon.wait_for("s9-wt-two", SHORT, |s| s.worktree_path.is_some());
    let wt1 = s1.worktree_path.expect("worktree path");
    let wt2 = s2.worktree_path.expect("worktree path");
    assert_ne!(wt1, wt2, "each session should get its own worktree");
    assert!(std::path::Path::new(&wt1).exists());
    assert!(std::path::Path::new(&wt2).exists());

    daemon.stop("s9-wt-one", false);
    let s1_stopped = daemon.wait_status("s9-wt-one", SessionStatus::Done, SHORT);
    assert_eq!(s1_stopped.status_reason.as_deref(), Some("stopped"));
    assert!(
        std::path::Path::new(&wt1).exists(),
        "stopping a session (without --purge) must not remove its worktree"
    );

    // `pulpo rm` purges a single done session outright, worktree included —
    // without touching the still-alive `s9-wt-two` sharing nothing but the origin
    // repo.
    let rm_out = daemon.remove("s9-wt-one");
    assert!(
        rm_out.status.success(),
        "pulpo rm failed: {}",
        String::from_utf8_lossy(&rm_out.stderr)
    );
    assert!(
        daemon.session("s9-wt-one").is_none(),
        "pulpo rm should remove the session record"
    );
    assert!(
        !std::path::Path::new(&wt1).exists(),
        "pulpo rm should remove worktree 1"
    );
    assert!(
        std::path::Path::new(&wt2).exists(),
        "pulpo rm on session 1 must not touch session 2's worktree"
    );

    daemon.stop("s9-wt-two", false);
    daemon.wait_status("s9-wt-two", SessionStatus::Done, SHORT);

    let cleanup = daemon.cleanup();
    assert!(cleanup.status.success());

    assert!(
        !std::path::Path::new(&wt1).exists(),
        "cleanup should remove worktree 1"
    );
    assert!(
        !std::path::Path::new(&wt2).exists(),
        "cleanup should remove worktree 2"
    );
}

// ---------------------------------------------------------------------------
// S10 — hooks reach a non-default port
// ---------------------------------------------------------------------------

#[test]
fn s10_pulpo_url_points_at_the_configured_port() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "start,hang");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn("s10-port", &workdir, &[], &[&claude_str, "-p", "hello"]);
    assert!(output.status.success());
    daemon.wait_status("s10-port", SessionStatus::Working, SHORT);

    // Every scenario in this suite implicitly proves PULPO_URL works (S1 already
    // fails otherwise — the daemon is never on its default port 7433 here) — this
    // test makes the assertion explicit, reading the fake's own env dump.
    let deadline = std::time::Instant::now() + SHORT;
    loop {
        let env = read_fake_env(&workdir);
        if let Some(url) = env.get("PULPO_URL") {
            assert_eq!(url, &format!("http://127.0.0.1:{}", daemon.port));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pulpo-fake-env.txt never appeared under {workdir:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------
// S11 — generic command (no harness)
// ---------------------------------------------------------------------------

#[test]
fn s11_generic_command_has_no_harness_and_ends_done() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();

    let output = daemon.spawn("s11-generic", &workdir, &[], &["sleep", "3"]);
    assert!(output.status.success());

    // `GenericAdapter` always matches (last, in the registry) so every command
    // gets a harness id — "generic" specifically means "no real adapter, no
    // rewrite, no events," which is what "no harness" means operationally: no
    // lifecycle-hook rewrite happened, and detection stays on scrollback
    // heuristics/exit markers the whole way, exactly as it did before harness
    // adapters existed.
    let session = daemon.wait_status("s11-generic", SessionStatus::Working, SHORT);
    assert_eq!(session.harness.as_deref(), Some("generic"));
    assert_eq!(session.harness_session_id, None);

    // No harness events at all for a generic command — once `sleep 3` exits,
    // `wrap_command` writes the exit markers and its own shell exits right behind
    // it (no more lingering fallback shell), so tmux tears the session down and the
    // very next dead-backend check resolves straight to `Done` — no intermediate
    // `Ready`, and no need to type `exit` into a shell that no longer exists.
    let session = daemon.wait_status("s11-generic", SessionStatus::Done, SHORT);
    assert_eq!(session.status_reason.as_deref(), Some("exited"));
    assert_eq!(session.exit_code, Some(0));

    // `sleep 3` exiting on its own wrote a real `.code` exit marker via
    // `wrap_command` — confirm it's actually there before proving `pulpo rm`
    // cleans it up below.
    let session_id = session.id.to_string();
    let marker_path = daemon
        .data_dir
        .join("exit")
        .join(format!("{session_id}.code"));
    assert!(
        marker_path.exists(),
        "expected exit marker at {marker_path:?}"
    );

    // `pulpo rm` on a Done session: purges the row and its exit markers (see
    // the same purge helper `pulpo stop --purge`/`pulpo cleanup` use).
    let rm_out = daemon.remove("s11-generic");
    assert!(
        rm_out.status.success(),
        "pulpo rm failed: {}",
        String::from_utf8_lossy(&rm_out.stderr)
    );
    assert!(
        daemon.session("s11-generic").is_none(),
        "pulpo rm should remove the session record"
    );
    assert!(
        !marker_path.exists(),
        "pulpo rm should remove the exit marker"
    );
}

// ---------------------------------------------------------------------------
// S12 — quoted spawn arguments survive to the harness (v0.3.0 quoting bug)
// ---------------------------------------------------------------------------

/// Regression test for the v0.3.0 bug reported on the owner's machine:
/// `pulpo spawn smoke --workdir ~/x -d -- claude -p "Reply with exactly the word:
/// pong" --model haiku` stored (and ran) the command as
/// `claude -p Reply with exactly the word: pong --model haiku` — the CLI joined the
/// trailing `command: Vec<String>` with a plain `command.join(" ")`, so every shell
/// downstream (the harness adapter's `shell_words::split`, and ultimately the shell
/// `pulpod`'s `wrap_command` runs the session under) split and reinterpreted the
/// prompt into stray positional arguments instead of treating it as Claude Code's `-p`
/// value. `pulpo-cli` now joins with `shell_words::join`, which quotes each argument
/// so it round-trips exactly.
///
/// This proves the fix end to end through every real hop: the `pulpo` CLI, the
/// daemon's Claude adapter (`prepare_spawn`'s split/rewrite/join), `wrap_command`'s
/// own shell-escaping, and the real shell that finally execs the harness — by reading
/// back `fake-claude`'s own recorded `argv` (`pulpo-fake-argv.json`), not just
/// `pulpod`'s view of the stored command string.
#[test]
fn s12_spawn_quoted_prompt_reaches_harness_intact() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "start,hang");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn(
        "s12-quoted-spawn",
        &workdir,
        &[],
        &[
            &claude_str,
            "-p",
            "Reply with exactly the word: pong",
            "--model",
            "haiku",
        ],
    );
    assert!(
        output.status.success(),
        "pulpo spawn failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    daemon.wait_status("s12-quoted-spawn", SessionStatus::Working, SHORT);
    let argv = wait_for_fake_argv(&workdir, SHORT);

    // The daemon's Claude adapter splices `--session-id <uuid> --settings <path>`
    // right after argv0, so `-p`/`--model` are not necessarily at fixed indices —
    // find them by value instead of asserting on the whole argv shape.
    let p_index = argv
        .iter()
        .position(|a| a == "-p")
        .unwrap_or_else(|| panic!("-p flag missing from recorded argv: {argv:?}"));
    assert_eq!(
        argv.get(p_index + 1).map(String::as_str),
        Some("Reply with exactly the word: pong"),
        "the prompt must survive as ONE argument, not split by a shell along the way; \
         full argv: {argv:?}"
    );

    let model_index = argv
        .iter()
        .position(|a| a == "--model")
        .unwrap_or_else(|| panic!("--model flag missing from recorded argv: {argv:?}"));
    assert_eq!(
        argv.get(model_index + 1).map(String::as_str),
        Some("haiku"),
        "full argv: {argv:?}"
    );
}

// ---------------------------------------------------------------------------
// S13 — an unusable database recovers instead of crash-looping
// ---------------------------------------------------------------------------

/// Assert the data dir has a quarantined `state.db.unusable-*` file, then
/// prove the fresh database left in its place is actually usable through the
/// real CLI.
fn assert_recovered_and_usable(daemon: &Daemon) {
    let quarantined = std::fs::read_dir(&daemon.data_dir)
        .expect("read data dir")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains("state.db.unusable-")
        });
    assert!(
        quarantined,
        "expected a quarantined state.db.unusable-* file in {:?}",
        daemon.data_dir
    );

    let output = daemon.pulpo(&["ls", "--all"]);
    assert!(
        output.status.success(),
        "pulpo ls --all failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Build a real SQLite file at `path` with a legitimate `_sqlx_migrations`
/// table (the same schema sqlx itself creates) carrying one row for a
/// migration version (9999) far ahead of anything this binary's embedded
/// migrator knows — simulating a database a *newer* `pulpod` already
/// migrated, now opened by an *older* binary after a downgrade. Shells out to
/// the system `sqlite3` (also relied on being present the way `tmux` is for
/// this whole suite — see `AGENTS.md`'s "Running it" note).
fn seed_downgraded_database(path: &std::path::Path) {
    let sql = "\
        CREATE TABLE _sqlx_migrations (\n\
            version BIGINT PRIMARY KEY,\n\
            description TEXT NOT NULL,\n\
            installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,\n\
            success BOOLEAN NOT NULL,\n\
            checksum BLOB NOT NULL,\n\
            execution_time BIGINT NOT NULL\n\
        );\n\
        INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)\n\
        VALUES (9999, 'from the future', 1, x'00', 0);\n";
    let status = std::process::Command::new("sqlite3")
        .arg(path)
        .arg(sql)
        .status()
        .expect("run sqlite3 to seed a downgraded database");
    assert!(status.success(), "sqlite3 seed command failed");
}

/// Regression test for the owner's 0.0.39 -> 0.3.0 upgrade incident: the
/// daemon refused a pre-migration database ("unsupported legacy database
/// schema detected"), exited 1, and launchd restarted it 14 times — the CLI
/// only ever reported "pulpod did not start in time", with the real cause
/// sitting in a log file nobody looked at. `pulpod` now quarantines an
/// unusable database (renaming it to `state.db.unusable-<timestamp>`) and
/// starts fresh instead of exiting. This case: an outright garbage/corrupt
/// `state.db` (not a SQLite file at all).
#[test]
fn s13_garbage_database_recovers_and_starts_fresh() {
    let daemon = Daemon::start_with_seed(DaemonConfig::default(), |data_dir| {
        std::fs::write(data_dir.join("state.db"), b"not a sqlite database")
            .expect("seed garbage state.db");
    });

    // `Daemon::start_with_seed` already waits for the daemon to become
    // healthy (panicking with its log tailed in if it never does) — reaching
    // here already proves the daemon didn't crash-loop on the garbage file.
    assert_recovered_and_usable(&daemon);
}

/// Same recovery, for the other failure mode called out in `AGENTS.md`: a
/// downgrade, where `MIGRATOR.run()` fails with sqlx's `VersionMissing`
/// rather than the legacy-schema check.
#[test]
fn s13_downgraded_database_recovers_and_starts_fresh() {
    let daemon = Daemon::start_with_seed(DaemonConfig::default(), |data_dir| {
        seed_downgraded_database(&data_dir.join("state.db"));
    });

    assert_recovered_and_usable(&daemon);
}

// ---------------------------------------------------------------------------
// S14 — Codex: spawn, permission prompt, exit, resume, usage
// ---------------------------------------------------------------------------

/// Assert `pulpo usage --scan --json` reports at least one token counted for `agent`
/// (`"claude"`/`"codex"`/`"pi"`) — proves a fake harness's own history file is
/// discovered by the real usage reader, not just that the CLI call succeeds.
fn assert_usage_scan_counts_agent(daemon: &Daemon, agent: &str) {
    let output = daemon.pulpo(&["usage", "--scan", "--json"]);
    assert!(
        output.status.success(),
        "pulpo usage --scan --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let scan: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("usage --scan --json output should be JSON");
    let tokens = scan["by_agent"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["label"] == agent))
        .and_then(|row| row["total_tokens"].as_u64())
        .unwrap_or(0);
    assert!(
        tokens > 0,
        "expected {agent} usage to be counted by the scan, got: {scan}"
    );
}

#[test]
fn s14_codex_spawn_needs_input_exit_then_resume_and_usage_counted() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "start,prompt,needs_input,wait,exit");
    let codex = daemon.fake_codex_bin();
    let codex_str = codex.to_string_lossy().into_owned();

    let output = daemon.spawn(
        "s14-codex",
        &workdir,
        &[],
        &[&codex_str, "-m", "gpt-5-codex", "fix the bug"],
    );
    assert!(
        output.status.success(),
        "pulpo spawn failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let session = daemon.wait_status("s14-codex", SessionStatus::Working, SHORT);
    assert_eq!(session.harness.as_deref(), Some("codex"));

    // Codex has no flag to preset its own thread id at launch — the FIRST hook
    // (SessionStart) is what teaches pulpo the harness_session_id.
    let session = daemon.wait_for("s14-codex", SHORT, |s| s.harness_session_id.is_some());
    let harness_session_id = session.harness_session_id.clone().unwrap();

    // The adapter's isolated CODEX_HOME setup: fake-codex checks auth.json/the real-home
    // symlinks itself and reports the results — proves the seeding actually ran, not
    // just that the isolated directory exists.
    let env_checks = wait_for_fake_codex_env(&workdir, SHORT);
    assert!(
        env_checks["auth_json_exists"].is_boolean(),
        "fake-codex should always report whether auth.json exists: {env_checks}"
    );
    let symlinks = env_checks["symlinks"]
        .as_object()
        .expect("symlinks should be an object");
    for name in [
        "AGENTS.md",
        "skills",
        "rules",
        "plugins",
        "prompts",
        "memories",
    ] {
        assert!(
            symlinks.contains_key(name),
            "expected a check for symlinked entry {name:?}: {env_checks}"
        );
    }
    let original_codex_home = env_checks["codex_home"]
        .as_str()
        .expect("codex_home should be a string")
        .to_owned();

    let session = daemon.wait_status("s14-codex", SessionStatus::Waiting, SHORT);
    assert_eq!(
        session.status_reason.as_deref(),
        Some("needs_input:permission")
    );

    daemon.input("s14-codex", None);

    // The fake resolves the prompt (Working) then finishes the turn (Stop) before the
    // next scenario step ("exit") fires SessionEnd and the process exits cleanly.
    // Same as S3: SessionEnded fires before the process actually exits, so the
    // status stays put until the wrapper's markers land and tmux tears the session
    // down on its own — straight to Done, no intermediate Ready.
    let session = daemon.wait_status("s14-codex", SessionStatus::Done, SHORT);
    assert_eq!(session.status_reason.as_deref(), Some("exited"));
    assert_eq!(session.exit_code, Some(0));

    // Swap in a scenario that just stays up after starting (the tmux command line is
    // reused verbatim on resume, but this file is read fresh by every new
    // fake-codex process — see fake-claude's S3/S4/S5 precedent), so the *resumed*
    // process is reliably observable as Working without also replaying
    // needs_input/wait and getting stuck blocked on stdin again.
    set_scenario(&workdir, "start,hang");
    let before_resume_state = read_fake_state_or_panic(&workdir);
    let old_pid = before_resume_state["pid"].as_u64().expect("pid");

    daemon.resume("s14-codex");

    let session = daemon.wait_status("s14-codex", SessionStatus::Working, SHORT);
    assert_eq!(
        session.harness_session_id.as_deref(),
        Some(harness_session_id.as_str()),
        "resume must continue the same Codex thread, not start a new one"
    );

    wait_for_fake_state(&workdir, SHORT, |s| {
        s["pid"].as_u64().is_some_and(|pid| pid != old_pid)
    });

    let argv = read_fake_argv(&workdir).expect("resumed fake-codex should have recorded argv");
    assert!(
        argv.iter().any(|a| a == "resume"),
        "expected a resume subcommand in argv: {argv:?}"
    );
    assert!(
        argv.iter().any(|a| a == harness_session_id.as_str()),
        "expected the exact harness session id in argv: {argv:?}"
    );
    assert!(
        argv.iter().any(|a| a == "--dangerously-bypass-hook-trust"),
        "expected the hook-trust bypass flag on resume: {argv:?}"
    );

    // Same isolated CODEX_HOME as the original spawn — resume must reuse it, not
    // redirect to a fresh (empty) one.
    let env = read_fake_env(&workdir);
    assert_eq!(
        env.get("CODEX_HOME").map(String::as_str),
        Some(original_codex_home.as_str())
    );

    assert_usage_scan_counts_agent(&daemon, "codex");
}

// ---------------------------------------------------------------------------
// S15 — pi: spawn, permission prompt, exit, resume (idempotent --session-id), usage
// ---------------------------------------------------------------------------

#[test]
fn s15_pi_spawn_needs_input_exit_then_resume_is_idempotent_and_usage_counted() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "start,prompt,needs_input,wait,exit");
    let pi = daemon.fake_pi_bin();
    let pi_str = pi.to_string_lossy().into_owned();

    let output = daemon.spawn("s15-pi", &workdir, &[], &[&pi_str, "-p", "fix the bug"]);
    assert!(
        output.status.success(),
        "pulpo spawn failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let session = daemon.wait_status("s15-pi", SessionStatus::Working, SHORT);
    assert_eq!(session.harness.as_deref(), Some("pi"));
    // Unlike Codex, pi's --session-id is preset up front (like Claude's) — known
    // immediately, no hook needed.
    let harness_session_id = session
        .harness_session_id
        .clone()
        .expect("pi harness session id should be known up front");

    let session = daemon.wait_status("s15-pi", SessionStatus::Waiting, SHORT);
    assert_eq!(
        session.status_reason.as_deref(),
        Some("needs_input:permission")
    );

    daemon.input("s15-pi", None);

    // Same as S3/S14: SessionEnded fires before the process actually exits, so the
    // status stays put until the wrapper's markers land and tmux tears the session
    // down on its own — straight to Done, no intermediate Ready.
    let session = daemon.wait_status("s15-pi", SessionStatus::Done, SHORT);
    assert_eq!(session.status_reason.as_deref(), Some("exited"));
    assert_eq!(session.exit_code, Some(0));

    // Swap in a scenario that just stays up after starting (read fresh by every new
    // fake-pi process — see fake-claude's S3/S4/S5 precedent), so the resumed
    // process is reliably observable as Working without also replaying
    // needs_input/wait and getting stuck blocked on stdin again.
    set_scenario(&workdir, "start,hang");
    let before_resume_state = read_fake_state_or_panic(&workdir);
    let old_pid = before_resume_state["pid"].as_u64().expect("pid");

    daemon.resume("s15-pi");

    let session = daemon.wait_status("s15-pi", SessionStatus::Working, SHORT);
    assert_eq!(
        session.harness_session_id.as_deref(),
        Some(harness_session_id.as_str())
    );

    let state = wait_for_fake_state(&workdir, SHORT, |s| {
        s["pid"].as_u64().is_some_and(|pid| pid != old_pid)
    });
    // pi's --session-id is idempotent create-or-open: the resumed process must
    // reopen the SAME on-disk session file the original spawn created, not mint a
    // new one — `resumed: true` here means "found the file already on disk".
    assert_eq!(state["resumed"], serde_json::json!(true));
    assert_eq!(state["session_id"], serde_json::json!(harness_session_id));

    let argv = read_fake_argv(&workdir).expect("resumed fake-pi should have recorded argv");
    // Resume command shape identical to spawn: `pi --session-id <id> -e <path> ...`.
    let idx = argv
        .iter()
        .position(|a| a == "--session-id")
        .unwrap_or_else(|| panic!("--session-id missing from resumed argv: {argv:?}"));
    assert_eq!(
        argv.get(idx + 1).map(String::as_str),
        Some(harness_session_id.as_str())
    );
    assert!(
        argv.iter().any(|a| a == "-e"),
        "hooks must still be wired on resume: {argv:?}"
    );

    assert_usage_scan_counts_agent(&daemon, "pi");
}

// ---------------------------------------------------------------------------
// S16 — resume fallback when a harness id was never learned
// ---------------------------------------------------------------------------

/// Codex never presets its own thread id at spawn (unlike Claude/pi) — the
/// `SessionStart` hook is the *only* way pulpo learns it. A scenario that skips the
/// `start` step (hooks never fire it) leaves `harness_session_id` unknown even
/// though the session ran and exited cleanly — exactly the "legacy row / hook never
/// fired" case `resolve_resume_command`'s fallback exists for. Resume must still
/// target this exact session's own isolated `CODEX_HOME` via `resume --last` (the
/// harness's own "most recent conversation here" flag) rather than silently starting
/// a brand new Codex thread.
#[test]
fn s16_codex_resume_without_harness_session_id_falls_back_to_resume_last() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "stop,exit");
    let codex = daemon.fake_codex_bin();
    let codex_str = codex.to_string_lossy().into_owned();

    let output = daemon.spawn(
        "s16-codex-fallback",
        &workdir,
        &[],
        &[&codex_str, "-m", "gpt-5-codex", "fix the bug"],
    );
    assert!(
        output.status.success(),
        "pulpo spawn failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Don't wait on the transient `Working` status here: the "stop,exit" scenario
    // (no "start"/"wait" step) can run to completion in well under the ~150ms poll
    // interval on a loaded CI runner, so `Working` may never be observed at all
    // before the session moves on to `Done` — a genuine intermittent race, not a
    // platform difference (this exact pattern flaked on Linux CI). Wait directly
    // for the stable, terminal state this scenario actually settles into instead.
    //
    // Same as S3/S14/S15: the `SessionEnded` hook fires before the fake process
    // actually exits, so `apply_harness_event` leaves the status alone; once the
    // process really does exit, the wrapper's markers land and its shell exits
    // right behind it (no more lingering fallback shell), so the very next
    // dead-backend check resolves straight to `Done` — no separate `Ready`/stop
    // step needed.
    let session = daemon.wait_for("s16-codex-fallback", SHORT, |s| s.exit_code == Some(0));
    assert_eq!(session.harness.as_deref(), Some("codex"));
    assert_eq!(session.status, SessionStatus::Done);
    assert_eq!(session.status_reason.as_deref(), Some("exited"));
    assert!(
        session.harness_session_id.is_none(),
        "still no id learned after a clean exit with hooks disabled"
    );

    // Swap in a scenario that just stays up after starting (read fresh by every new
    // fake-codex process), so the resumed process is reliably observable as Working
    // rather than racing straight through "stop,exit" again before a poll catches it.
    set_scenario(&workdir, "start,hang");
    let before_resume_state = read_fake_state_or_panic(&workdir);
    let old_pid = before_resume_state["pid"].as_u64().expect("pid");

    daemon.resume("s16-codex-fallback");

    daemon.wait_status("s16-codex-fallback", SessionStatus::Working, SHORT);
    wait_for_fake_state(&workdir, SHORT, |s| {
        s["pid"].as_u64().is_some_and(|pid| pid != old_pid)
    });

    let argv = read_fake_argv(&workdir).expect("resumed fake-codex should have recorded argv");
    assert!(
        argv.iter().any(|a| a == "resume"),
        "expected a resume subcommand in argv: {argv:?}"
    );
    assert!(
        argv.iter().any(|a| a == "--last"),
        "no id was ever learned — resume must fall back to --last, not a fresh spawn: {argv:?}"
    );
    assert!(
        argv.iter().any(|a| a == "--dangerously-bypass-hook-trust"),
        "hooks must still be re-injected on the fallback resume: {argv:?}"
    );
    // The original prompt positional must not be replayed as a new turn.
    assert!(
        !argv.iter().any(|a| a == "fix the bug"),
        "the original prompt must be stripped on resume, not resubmitted: {argv:?}"
    );
}

/// Write `pulpo-fake-scenario.txt` in `repo_path` and commit it — used (instead of
/// plain [`set_scenario`]) when the file needs to already be present the moment a
/// *worktree* is created from this repo (`git worktree add` only checks out tracked
/// content, so an untracked scenario file written into `repo_path` would never reach
/// a fresh worktree — and writing it into the worktree directory afterward would
/// race the fake harness process reading it once at startup).
fn commit_fake_scenario(repo_path: &std::path::Path, scenario: &str) {
    set_scenario(repo_path, scenario);
    let run = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed in {repo_path:?}");
    };
    run(&["add", "pulpo-fake-scenario.txt"]);
    run(&["commit", "-q", "-m", "add fake scenario"]);
}

/// #128 follow-up, S16 extension: Codex's fallback resume must keep working even
/// when the session's worktree has been removed — unlike Claude/pi's `--continue`
/// (refused in that case, see `session::manager::resolve_resume_command`'s unit
/// tests), `codex resume --last` is keyed by this session's own isolated
/// `CODEX_HOME`, not by the directory it's run from, so `resolve_resume_command`
/// must not refuse it just because `effective_resume_workdir` fell back to the
/// original repo path.
///
/// Reaches `Done` directly (no more lingering fallback shell to separately stop —
/// ADR 0009), and never waits on the transient `Working` status after the initial
/// spawn — see the plain S16 test above for the full writeup of an intermittent
/// CI-only (Linux) race this scenario shape exposed: with a "stop,exit"-only fake
/// scenario (no "start"/"wait" step), the whole run can finish in well under this
/// suite's ~150ms poll interval, so a `wait_status(..., Working, _)` right after
/// spawn can time out having *never* observed `Working` at all — this is not
/// deterministic (a rerun of the same commit passed) and not specific to
/// worktrees (the plain S16 test flaked the exact same way on a later run).
#[test]
fn s16_codex_resume_still_works_when_worktree_removed() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_repo_dir, repo_path) = temp_workdir();
    init_git_repo(&repo_path);
    // Committed (not just written) so a fresh `git worktree add` checkout already
    // has it in place before the fake harness process ever starts — see
    // `commit_fake_scenario`.
    commit_fake_scenario(&repo_path, "stop,exit");

    let codex = daemon.fake_codex_bin();
    let codex_str = codex.to_string_lossy().into_owned();
    let output = daemon.spawn(
        "s16-codex-worktree-removed",
        &repo_path,
        &["--worktree"],
        &[&codex_str, "-m", "gpt-5-codex", "fix the bug"],
    );
    assert!(
        output.status.success(),
        "pulpo spawn failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let session = daemon.wait_for("s16-codex-worktree-removed", SHORT, |s| {
        s.worktree_path.is_some()
    });
    assert_eq!(session.harness.as_deref(), Some("codex"));
    let worktree_path = std::path::PathBuf::from(session.worktree_path.expect("worktree path"));
    assert!(worktree_path.exists(), "expected the worktree to exist");

    // Don't wait on the transient `Working` status here — see the plain S16 test's
    // comment above: "stop,exit" (no "start"/"wait" step) can complete in well
    // under the test's ~150ms poll interval on a loaded CI runner, so `Working` may
    // never actually be observed. Wait directly for the stable, terminal state:
    // once the exit code lands, the wrapper's markers are already there too, and
    // (no more lingering fallback shell — ADR 0009) the very next dead-backend
    // check resolves straight to `Done`, so no separate `pulpo stop` is needed.
    let resolved = daemon.wait_for("s16-codex-worktree-removed", SHORT, |s| {
        s.exit_code == Some(0)
    });
    assert_eq!(resolved.status, SessionStatus::Done);
    assert_eq!(resolved.status_reason.as_deref(), Some("exited"));
    assert!(
        resolved.harness_session_id.is_none(),
        "SessionStart never fires in this scenario — no id should be learned"
    );

    // Simulate the worktree being removed (branch merged and cleaned up, disk
    // wiped, ...) — `effective_resume_workdir` will now fall back to `repo_path`.
    std::fs::remove_dir_all(&worktree_path).expect("remove worktree");
    assert!(!worktree_path.exists());

    // `repo_path` never had a fake-claude/-codex process run in it before (the
    // initial run's cwd was the worktree), so there is no stale state file to race
    // against here, unlike S16's plain (non-worktree) case above.
    set_scenario(&repo_path, "start,hang");

    daemon.resume("s16-codex-worktree-removed");

    daemon.wait_status("s16-codex-worktree-removed", SessionStatus::Working, SHORT);
    wait_for_fake_state(&repo_path, SHORT, |s| s["pid"].as_u64().is_some());

    let argv = read_fake_argv(&repo_path).expect("resumed fake-codex should have recorded argv");
    assert!(
        argv.iter().any(|a| a == "resume"),
        "expected a resume subcommand in argv: {argv:?}"
    );
    assert!(
        argv.iter().any(|a| a == "--last"),
        "no id was ever learned — resume must fall back to --last: {argv:?}"
    );
    assert!(
        argv.iter().any(|a| a == "--dangerously-bypass-hook-trust"),
        "hooks must still be re-injected on the fallback resume: {argv:?}"
    );
    assert!(
        !argv.iter().any(|a| a == "fix the bug"),
        "the original prompt must be stripped on resume, not resubmitted: {argv:?}"
    );
}

// ---------------------------------------------------------------------------
// S19 — single-instance lock: a second pulpod on the same data dir refuses to start
// ---------------------------------------------------------------------------

/// #126 follow-up: `pulpod` acquires an exclusive advisory lock on
/// `{data_dir}/pulpod.lock` before ever touching `state.db` (well before the port
/// bind too — see `lib.rs::build_app`), so a second `pulpod` accidentally started
/// against the same data directory refuses to start instead of racing the first one
/// to open (and, on an unusable-database error, quarantine) the same database out
/// from under it.
#[test]
fn s19_second_daemon_on_same_data_dir_refuses_to_start() {
    let daemon = Daemon::start(DaemonConfig::default());
    let db_path = daemon.data_dir.join("state.db");
    assert!(
        db_path.exists(),
        "expected state.db to exist while the daemon runs"
    );

    let output = daemon.try_start_second_instance(MEDIUM);
    assert!(
        !output.status.success(),
        "a second pulpod on the same data dir must exit non-zero; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // The first daemon's database must be untouched: still at the canonical path,
    // and no quarantine file appeared alongside it.
    assert!(
        db_path.exists(),
        "state.db must still be at the canonical path"
    );
    let quarantined = std::fs::read_dir(&daemon.data_dir)
        .expect("read data dir")
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().contains("unusable"));
    assert!(
        !quarantined,
        "must not quarantine a healthy, in-use database"
    );

    // The first daemon is still completely healthy and serving requests.
    let output = daemon.pulpo(&["ls", "--all"]);
    assert!(
        output.status.success(),
        "pulpo ls --all failed after a blocked second instance: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// S17 — attach on a done session errors with the resume hint; resume recreates
// with the harness's own resume command
// ---------------------------------------------------------------------------

/// ADR 0009: once a session is `Done`, there's no live backend left to attach to at
/// all (no more lingering fallback shell to reconnect to, unlike the old `Ready`
/// state) — `pulpo attach` must refuse with a hint pointing at `pulpo resume`/`pulpo
/// logs` instead of trying (and failing more confusingly) to attach tmux to a
/// session that no longer exists. `pulpo resume` on that same session must still
/// work, recreating the backend with the harness's own resume command
/// (`claude --resume <id>`) — the same mechanism S3 proves in depth, checked here
/// specifically in combination with the attach error to prove the *whole*
/// done-session story (can't attach, can resume) rather than either half alone.
#[test]
fn s17_attach_on_done_errors_with_resume_hint_and_resume_recreates_with_harness_command() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "start,prompt,stop,exit");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn(
        "s17-attach-done",
        &workdir,
        &[],
        &[&claude_str, "-p", "hello"],
    );
    assert!(output.status.success());

    // Same shape as S3: SessionEnded fires before the fake process actually exits,
    // so the session only reaches Done once the wrapper's markers land and tmux
    // tears itself down — no manual "exit" step, no intermediate Ready.
    let session = daemon.wait_status("s17-attach-done", SessionStatus::Done, SHORT);
    assert_eq!(session.status_reason.as_deref(), Some("exited"));
    assert_eq!(session.exit_code, Some(0));
    let harness_session_id = session
        .harness_session_id
        .clone()
        .expect("harness session id should be known before exit");

    let attach_output = daemon.pulpo(&["attach", "s17-attach-done"]);
    assert!(
        !attach_output.status.success(),
        "pulpo attach on a done session should fail"
    );
    let attach_stderr = String::from_utf8_lossy(&attach_output.stderr);
    assert!(
        attach_stderr.contains("done"),
        "expected the error to name the done status: {attach_stderr}"
    );
    assert!(
        attach_stderr.contains("pulpo resume"),
        "expected a resume hint: {attach_stderr}"
    );
    assert!(
        attach_stderr.contains("pulpo logs"),
        "expected a logs hint: {attach_stderr}"
    );

    // Swap in a scenario that just stays up after starting, so the *resumed*
    // process is reliably observable as Working before it does anything else.
    set_scenario(&workdir, "start,hang");
    let before_resume_state = read_fake_state_or_panic(&workdir);
    let old_pid = before_resume_state["pid"].as_u64().expect("pid");

    daemon.resume("s17-attach-done");

    let session = daemon.wait_status("s17-attach-done", SessionStatus::Working, SHORT);
    assert_eq!(
        session.harness_session_id.as_deref(),
        Some(harness_session_id.as_str()),
        "resume must continue the same harness conversation, not start a new one"
    );

    // Confirm the daemon actually rewrote the spawn into the harness's own resume
    // command (`claude --resume <id> ...`), not a fresh `-p` invocation.
    let state = wait_for_fake_state(&workdir, SHORT, |s| {
        s["pid"].as_u64().is_some_and(|pid| pid != old_pid)
    });
    assert_eq!(state["resumed"], serde_json::json!(true));
    assert_eq!(state["source"], serde_json::json!("resume"));
    assert_eq!(state["session_id"], serde_json::json!(harness_session_id));
    let argv = read_fake_argv(&workdir).expect("resumed fake-claude should have recorded argv");
    assert!(
        argv.iter().any(|a| a == "--resume"),
        "expected the harness's own --resume flag in argv: {argv:?}"
    );
    assert!(
        argv.iter().any(|a| a == harness_session_id.as_str()),
        "expected the exact harness session id in argv: {argv:?}"
    );
}

// ---------------------------------------------------------------------------
// S18 — watchdog resolves a dead backend without anyone polling, and the
// session's final output survives the pane closing
// ---------------------------------------------------------------------------

#[test]
fn s18_watchdog_resolves_dead_backend_eagerly_and_preserves_final_output() {
    // Regression test for a Fable review finding on PR #129: `check_and_mark_stale`
    // only ran lazily off `get_session`/`list_sessions` — the watchdog's own tick
    // never called it, so a session whose backend died went undetected (stuck
    // `working` forever, no `lifecycle.done` webhook) unless something happened to
    // poll it. Every other scenario in this suite calls `daemon.wait_status`/
    // `wait_for`, which polls `GET /sessions/{name}` every 150ms — exactly the
    // kind of polling that was masking this bug. This test asserts the daemon's
    // own dead-backend resolution *without ever polling the session endpoint*:
    // it waits only on the webhook sink for the `lifecycle.done` delivery, and
    // only fetches the session afterward, to confirm the DB state matches.
    let webhook = WebhookSink::start();
    let daemon = Daemon::start(DaemonConfig {
        webhook_url: Some(webhook.url()),
        // The daemon's default `check_interval_secs` in this harness is already 1s
        // (see `DaemonConfig::default`), so the watchdog's own eager check kicks in
        // within a couple of ticks — no override needed here.
        ..DaemonConfig::default()
    });
    let (_dir, workdir) = temp_workdir();

    // A generic (harness-less) command: a brief pause (giving `pulpod`'s own
    // `create_session` → `setup_logging` pipe-pane attach a moment to happen
    // before anything is printed — pipe-pane only captures output produced
    // *after* it attaches, not retroactively, see `backend::tmux`'s own
    // `test_pipe_pane_captures_output`), then prints a distinctive final line
    // and exits immediately with a non-zero code. `wrap_command` writes the
    // `.code` exit marker and its own wrapper shell exits right behind it —
    // there is no more lingering fallback shell (ADR 0009), so tmux tears the
    // pane down in the same instant. Any live tmux capture attempt made after
    // that will find nothing; only the pipe-pane log (now on by default —
    // `capture_session_output`) still has "s18-final-output-marker".
    let output = daemon.spawn(
        "s18-dead-backend",
        &workdir,
        &[],
        &["sh", "-c", "sleep 1; echo s18-final-output-marker; exit 3"],
    );
    assert!(output.status.success());

    // No `daemon.session(...)`/`wait_status`/`wait_for` calls above this line —
    // the webhook delivery below is the only thing this test waits on to learn
    // the session finished.
    let delivered = webhook.wait_for_matching(MEDIUM, |event| {
        event["type"] == "lifecycle"
            && event["subtype"] == "done"
            && event["session"]["name"] == "s18-dead-backend"
    });
    assert_eq!(
        delivered["session"]["exit_code"],
        serde_json::json!(3),
        "expected the real exit code on the lifecycle.done event, got: {delivered:#?}"
    );
    assert_eq!(delivered["session"]["status"], serde_json::json!("done"));

    // Only now, after the event already proved the daemon resolved this on its
    // own, fetch the session and its logs to confirm the rest of the DB state
    // and that the final output survived the pane closing.
    let session = daemon
        .session("s18-dead-backend")
        .expect("session should exist");
    assert_eq!(session.status, SessionStatus::Done);
    assert_eq!(session.status_reason.as_deref(), Some("exited"));
    assert_eq!(session.exit_code, Some(3));

    let logs_output = daemon.pulpo(&["logs", "s18-dead-backend"]);
    assert!(logs_output.status.success());
    let logs = String::from_utf8_lossy(&logs_output.stdout);
    assert!(
        logs.contains("s18-final-output-marker"),
        "expected the session's final printed line to survive via the pipe-pane \
         log fallback, got: {logs}"
    );
}
