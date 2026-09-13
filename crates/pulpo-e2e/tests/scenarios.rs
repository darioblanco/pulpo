//! End-to-end scenario suite: a real `pulpod`, a real (private) tmux server, and a
//! fake harness (`fake-claude`) standing in for Claude Code. Every test here drives
//! the daemon exactly the way a user would — the `pulpo` CLI and the daemon's own
//! HTTP API — and asserts on real, observed state, never a mock. See
//! `crates/pulpo-e2e/src/lib.rs` for the harness these tests are built on,
//! `crates/pulpo-e2e/src/bin/fake-claude.rs` for the fake harness's contract, and
//! `CLAUDE.md`'s testing section for the strategy this suite implements and how to
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
    read_fake_env, temp_workdir, wait_for_fake_state,
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
fn s1_spawn_reaches_active_with_harness_metadata() {
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

    let session = daemon.wait_status("s1-spawn", SessionStatus::Active, SHORT);
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

    let session = daemon.wait_status("s2-needs-input", SessionStatus::Idle, SHORT);
    assert_eq!(
        session
            .metadata
            .as_ref()
            .and_then(|m| m.get("needs_input"))
            .map(String::as_str),
        Some("permission")
    );

    daemon.input("s2-needs-input", None);

    // The fake resolves the prompt (Working) then finishes the turn (Stop): Idle,
    // but no longer blocked on the human.
    let session = daemon.wait_for("s2-needs-input", SHORT, |s| {
        s.status == SessionStatus::Idle
            && s.metadata
                .as_ref()
                .is_none_or(|m| !m.contains_key("needs_input"))
    });
    assert_eq!(session.status, SessionStatus::Idle);
    assert!(
        session
            .metadata
            .as_ref()
            .is_none_or(|m| !m.contains_key("needs_input")),
        "needs_input should be cleared: {:?}",
        session.metadata
    );
}

// ---------------------------------------------------------------------------
// S3 — clean exit, then resume
// ---------------------------------------------------------------------------

#[test]
fn s3_clean_exit_then_resume_recreates_with_resume_flag() {
    let daemon = Daemon::start(DaemonConfig::default());
    let (_dir, workdir) = temp_workdir();
    set_scenario(&workdir, "start,prompt,stop,exit");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn(
        "s3-clean-exit",
        &workdir,
        &[],
        &[&claude_str, "-p", "hello"],
    );
    assert!(output.status.success());

    // The harness's own SessionEnded event (fired by the "exit" step, before the
    // fake process actually exits) resolves the session to Ready first — the
    // backend (fallback shell) is still alive, so this doesn't necessarily know an
    // exit code straight away (`apply_harness_event` tries the `.code` marker
    // best-effort on the same hook, but the wrapper may not have written it yet).
    // The watchdog's own marker sweep (`watchdog::idle::check_idle_sessions`
    // revisiting `Ready` sessions with no `exit_code` yet) is the durable path, so
    // poll for it here rather than asserting it's already set the instant the
    // session reaches Ready.
    let session = daemon.wait_status("s3-clean-exit", SessionStatus::Ready, SHORT);
    let harness_session_id = session
        .harness_session_id
        .clone()
        .expect("harness session id should be known before exit");
    let session = daemon.wait_for("s3-clean-exit", SHORT, |s| s.exit_code == Some(0));
    assert_eq!(session.status, SessionStatus::Ready);
    assert_eq!(session.exit_code, Some(0));

    // The user exits the lingering fallback shell: the backend dies and the
    // session resolves the rest of the way to Stopped, keeping the exit code
    // already recorded while it was Ready.
    daemon.input("s3-clean-exit", Some("exit"));
    let session = daemon.wait_status("s3-clean-exit", SessionStatus::Stopped, SHORT);
    assert_eq!(session.exit_code, Some(0));

    // Swap in a scenario that just stays up after starting, so the *resumed*
    // process is reliably observable as Active before it does anything else.
    set_scenario(&workdir, "start,hang");
    let before_resume_state = read_fake_state_or_panic(&workdir);
    let old_pid = before_resume_state["pid"].as_u64().expect("pid");

    daemon.resume("s3-clean-exit");

    let session = daemon.wait_status("s3-clean-exit", SessionStatus::Active, SHORT);
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
    let session = daemon.wait_status("s4-lost", SessionStatus::Active, SHORT);
    let harness_session_id = session
        .harness_session_id
        .clone()
        .expect("harness session id should be known");

    daemon.kill_tmux_server();

    daemon.wait_status("s4-lost", SessionStatus::Lost, MEDIUM);

    daemon.resume("s4-lost");

    let session = daemon.wait_status("s4-lost", SessionStatus::Active, SHORT);
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
    daemon.wait_status("s5-restart-alive", SessionStatus::Active, SHORT);

    daemon.restart_daemon();

    let session = daemon.wait_status("s5-restart-alive", SessionStatus::Active, SHORT);
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
    daemon.wait_status("s5-restart-with-loss", SessionStatus::Active, SHORT);

    daemon.kill_tmux_server();
    daemon.restart_daemon();

    daemon.wait_status("s5-restart-with-loss", SessionStatus::Active, SHORT);
    // `backend_session_id` itself gets upgraded from a plain name to tmux's own
    // `$N` id shortly after creation (see `CLAUDE.md`'s "Session IDs" note) — the
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

    let session = daemon.wait_status("s6-budget", SessionStatus::Stopped, MEDIUM);
    assert_eq!(
        session.intervention_code,
        Some(InterventionCode::BudgetExceeded)
    );

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
    // Idle, unchanging output, past the idle timeout.
    set_scenario(&workdir, "start,prompt,stop,hang");
    let claude = daemon.fake_claude_bin();
    let claude_str = claude.to_string_lossy().into_owned();

    let output = daemon.spawn("s7-idle-kill", &workdir, &[], &[&claude_str, "-p", "hello"]);
    assert!(output.status.success());

    let session = daemon.wait_status("s7-idle-kill", SessionStatus::Stopped, LONG);
    assert_eq!(
        session.intervention_code,
        Some(InterventionCode::IdleTimeout)
    );
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
        SessionStatus::Active,
        "idle_threshold_secs=0 must disable the time-based Active->Idle transition"
    );

    // It still ends normally once the command exits.
    daemon.wait_status("s7-idle-threshold-0", SessionStatus::Ready, SHORT);
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
    daemon.wait_status("s9-wt-one", SessionStatus::Stopped, SHORT);
    assert!(
        std::path::Path::new(&wt1).exists(),
        "stopping a session (without --purge) must not remove its worktree"
    );

    daemon.stop("s9-wt-two", false);
    daemon.wait_status("s9-wt-two", SessionStatus::Stopped, SHORT);

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
    daemon.wait_status("s10-port", SessionStatus::Active, SHORT);

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
fn s11_generic_command_has_no_harness_and_ends_stopped() {
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
    let session = daemon.wait_status("s11-generic", SessionStatus::Active, SHORT);
    assert_eq!(session.harness.as_deref(), Some("generic"));
    assert_eq!(session.harness_session_id, None);

    let session = daemon.wait_status("s11-generic", SessionStatus::Ready, SHORT);
    assert_eq!(session.exit_code, Some(0));

    daemon.input("s11-generic", Some("exit"));
    daemon.wait_status("s11-generic", SessionStatus::Stopped, SHORT);
}
