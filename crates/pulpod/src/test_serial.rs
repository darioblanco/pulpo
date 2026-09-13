//! Serializes the handful of real-tmux integration tests still living in the unit
//! test suite (`backend::tmux`, `session::manager::real_tmux_tests`) so they never
//! run concurrently against each other under a parallel `cargo test`.
//!
//! Most of these tests already use their own throwaway tmux socket (`tmux -L
//! pulpo-test-<uuid>`), which isolates them from the developer's real tmux server
//! and from *most* cross-test interference — but a handful predate that pattern and
//! still talk to the *default* tmux socket (`backend/tmux.rs`'s
//! `tmux_run_and_capture`-based tests, `test_setup_logging_captures_session_output_to_file`,
//! `test_check_tmux_version_succeeds_if_installed`). Running several of those
//! concurrently was a documented source of flakiness (session name collisions,
//! server startup races) — see `CLAUDE.md`'s testing section. A single process-wide
//! mutex, held for a whole test's duration, is the simplest fix: it doesn't require
//! auditing every test for which socket it uses today, and it stays correct if a
//! future test is added without an isolated socket.
//!
//! The rest of the suite (thousands of `MockBackend`-based unit tests) is unaffected
//! — this lock is only acquired by tests that opt in by calling [`lock`].

use std::sync::{Mutex, MutexGuard, OnceLock};

static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Acquire the process-wide real-tmux test lock. Hold the returned guard for the
/// duration of the test (typically `let _guard = crate::test_serial::lock();` as
/// the first line). Recovers from a poisoned lock (a previous real-tmux test
/// panicked while holding it) rather than propagating the poison — one flaky test's
/// panic must not cascade into every real-tmux test after it failing to even start.
pub fn lock() -> MutexGuard<'static, ()> {
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
