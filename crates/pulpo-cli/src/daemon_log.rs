//! Find and tail `pulpod`'s own log so the CLI can show *why* the daemon
//! failed to start, instead of just "did not start in time".
//!
//! Background: upgrading 0.0.39 -> 0.3.0, the daemon refused a pre-migration
//! database ("unsupported legacy database schema detected"), exited 1, and
//! launchd restarted it repeatedly. `ensure_daemon_running`'s health-check
//! polling in `lib.rs` just timed out and printed a generic hint — the real
//! cause was sitting in a log file the CLI never looked at. This module
//! finds that log (or the Homebrew service log, if that's where it went
//! instead) and surfaces its most recent `ERROR` lines.
//!
//! Only lines (or whole files) from within [`RECENCY_WINDOW_SECS`] of "now"
//! are surfaced: `pulpod.log` is rotated hourly, so the current file can
//! still contain an unrelated `ERROR` from much earlier in the same hour —
//! e.g. a one-time "state.db quarantined" warning from a previous, otherwise
//! successful start — which used to get shown as if it were today's failure
//! cause regardless of age.
//!
//! `pulpo-cli` doesn't depend on the `pulpod` crate (it's the daemon binary,
//! not a library this talks to in-process), so the daemon's data-dir default
//! and `[node] data_dir` override are re-derived here directly from
//! `~/.pulpo/config.toml` rather than reusing `pulpod::config`. `pulpo-cli`'s
//! own `Cli` has no `--config`/`PULPO_CONFIG` override of its own (only
//! `pulpod`'s does), so there is nothing to honor here beyond the default path
//! — but `data_dir` is still read defensively (missing file, unparseable
//! TOML, missing key, or a blank value all fall back to the default dir
//! rather than producing a nonsense path).

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// How far back from "now" an `ERROR` line (or a whole log file) may be and
/// still count as part of the *current* failed start attempt.
/// `ensure_daemon_running` polls for at most ~5s before giving up, but this
/// stays generous — clock skew, a slow `brew services start`, CI scheduling
/// jitter — rather than exact.
const RECENCY_WINDOW_SECS: i64 = 120;

/// Parse a `tracing_subscriber::fmt` line's leading RFC 3339 timestamp
/// (`2026-09-16T14:22:10.123456Z  ERROR pulpod: ...`). `None` for lines with
/// no recognizable timestamp prefix — e.g. a raw panic message written
/// straight to stderr, bypassing `tracing` entirely.
fn line_timestamp(line: &str) -> Option<DateTime<Utc>> {
    let token = line.split_whitespace().next()?;
    DateTime::parse_from_rfc3339(token)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Whether `ts` is within [`RECENCY_WINDOW_SECS`] of `now`. A `ts` at or after
/// `now` (clock skew, or a timestamp minted after `now` was captured) always
/// counts as fresh.
fn is_recent(ts: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now.signed_duration_since(ts) <= chrono::Duration::seconds(RECENCY_WINDOW_SECS)
}

/// Whether `path`'s own mtime is recent enough to possibly contain output
/// from the current start attempt — a cheap whole-file skip for a log file
/// untouched since long before now (also covers the Homebrew service log,
/// whose lines carry no `tracing` timestamp of their own to check).
fn file_is_recent(path: &Path, now: DateTime<Utc>) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|modified| is_recent(DateTime::<Utc>::from(modified), now))
}

/// Resolve the data dir `pulpod` would use for `home`, mirroring
/// `pulpod::config`'s `default_data_dir()` (`~/.pulpo`) and its
/// `[node] data_dir` override. Falls back to the default dir for any
/// unreadable/unparseable config, a missing key, or a blank value.
fn resolve_daemon_data_dir(home: &Path) -> PathBuf {
    let default_dir = home.join(".pulpo");
    let config_path = default_dir.join("config.toml");
    let Ok(contents) = std::fs::read_to_string(&config_path) else {
        return default_dir;
    };
    contents
        .parse::<toml::Value>()
        .ok()
        .and_then(|value| {
            let data_dir = value.get("node")?.get("data_dir")?.as_str()?.trim();
            (!data_dir.is_empty()).then(|| data_dir.to_owned())
        })
        .map_or(default_dir, |data_dir| {
            PathBuf::from(shellexpand::tilde(&data_dir).into_owned())
        })
}

/// Find the newest `pulpod.log*` file directly under `logs_dir` — matches how
/// `pulpod::init_tracing`'s hourly-rotated file appender names its files
/// (`pulpod.log.<date>-<hour>`).
fn newest_log_in(logs_dir: &Path) -> Option<PathBuf> {
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(logs_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("pulpod.log")
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    entries.into_iter().map(|(_, path)| path).next()
}

/// Up to `max_lines` of the most recent lines containing `ERROR` in `path`,
/// oldest first, restricted to those within [`RECENCY_WINDOW_SECS`] of `now`
/// (by the line's own timestamp when present, otherwise by the file's mtime).
/// Empty when the file doesn't exist, can't be read, is too old, or has no
/// recent `ERROR` lines.
fn tail_error_lines_from(path: &Path, max_lines: usize, now: DateTime<Utc>) -> Vec<String> {
    if !file_is_recent(path, now) {
        return Vec::new();
    }
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut errors: Vec<&str> = contents
        .lines()
        .filter(|line| line.contains("ERROR"))
        .filter(|line| line_timestamp(line).is_none_or(|ts| is_recent(ts, now)))
        .collect();
    let start = errors.len().saturating_sub(max_lines);
    errors
        .split_off(start)
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Candidate log files to check, most-authoritative first: the daemon's own
/// log dir under its data dir, then (macOS only) `brew_log` — the Homebrew
/// service log a `brew services`-managed `pulpod` may write stdout/stderr to.
fn candidate_log_paths_with_brew_log(home: &Path, brew_log: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = newest_log_in(&resolve_daemon_data_dir(home).join("logs")) {
        candidates.push(path);
    }
    if brew_log.is_file() {
        candidates.push(brew_log.to_path_buf());
    }
    candidates
}

/// [`candidate_log_paths_with_brew_log`] against the real, fixed Homebrew
/// service log path.
fn candidate_log_paths(home: &Path) -> Vec<PathBuf> {
    candidate_log_paths_with_brew_log(home, Path::new("/opt/homebrew/var/log/pulpo.log"))
}

/// Up to `max_lines` of the most recent `ERROR` lines from whichever
/// candidate daemon log has any (see [`candidate_log_paths`]). Empty when
/// nothing is found or `$HOME` can't be resolved — the caller should still
/// print its own generic hint in that case.
#[must_use]
pub fn tail_daemon_error_lines(max_lines: usize) -> Vec<String> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let now = Utc::now();
    for path in candidate_log_paths(&home) {
        let lines = tail_error_lines_from(&path, max_lines, now);
        if !lines.is_empty() {
            return lines;
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tail_error_lines_from_picks_last_n_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pulpod.log");
        std::fs::write(&path, "INFO one\nERROR a\nINFO two\nERROR b\nERROR c\n").unwrap();
        assert_eq!(
            tail_error_lines_from(&path, 2, Utc::now()),
            vec!["ERROR b".to_string(), "ERROR c".to_string()]
        );
    }

    #[test]
    fn test_tail_error_lines_from_fewer_errors_than_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pulpod.log");
        std::fs::write(&path, "INFO one\nERROR only\n").unwrap();
        assert_eq!(
            tail_error_lines_from(&path, 3, Utc::now()),
            vec!["ERROR only".to_string()]
        );
    }

    #[test]
    fn test_tail_error_lines_from_missing_file() {
        assert!(
            tail_error_lines_from(Path::new("/nonexistent/pulpod.log"), 3, Utc::now()).is_empty()
        );
    }

    #[test]
    fn test_tail_error_lines_from_no_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pulpod.log");
        std::fs::write(&path, "INFO all fine\n").unwrap();
        assert!(tail_error_lines_from(&path, 3, Utc::now()).is_empty());
    }

    #[test]
    fn test_tail_error_lines_from_ignores_stale_file() {
        // A log file untouched for far longer than the recency window can only
        // contain errors from an earlier, unrelated run.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pulpod.log");
        std::fs::write(&path, "ERROR ancient failure\n").unwrap();
        let far_future = Utc::now() + chrono::Duration::seconds(RECENCY_WINDOW_SECS + 3600);
        assert!(tail_error_lines_from(&path, 3, far_future).is_empty());
    }

    #[test]
    fn test_tail_error_lines_from_filters_stale_lines_by_own_timestamp() {
        // Same (fresh) file, but one ERROR line's own timestamp is old (e.g. a
        // one-time warning from earlier in this hourly-rotated file) and one is
        // fresh — only the fresh one should surface.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pulpod.log");
        let now = Utc::now();
        let old_ts = (now - chrono::Duration::seconds(RECENCY_WINDOW_SECS + 60)).to_rfc3339();
        let fresh_ts = now.to_rfc3339();
        std::fs::write(
            &path,
            format!("{old_ts}  ERROR stale quarantine warning\n{fresh_ts}  ERROR real failure\n"),
        )
        .unwrap();
        let lines = tail_error_lines_from(&path, 3, now);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("real failure"), "got: {lines:?}");
    }

    #[test]
    fn test_tail_error_lines_from_keeps_untimestamped_lines_in_a_fresh_file() {
        // A raw panic line has no `tracing` timestamp prefix — still surfaced
        // as long as the file itself was touched recently.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pulpo.log");
        std::fs::write(&path, "thread 'main' panicked: ERROR boom\n").unwrap();
        let lines = tail_error_lines_from(&path, 3, Utc::now());
        assert_eq!(
            lines,
            vec!["thread 'main' panicked: ERROR boom".to_string()]
        );
    }

    #[test]
    fn test_line_timestamp_parses_tracing_format() {
        let ts = line_timestamp("2026-09-16T14:22:10.123456Z  ERROR pulpod: boom");
        assert!(ts.is_some());
    }

    #[test]
    fn test_line_timestamp_none_without_prefix() {
        assert!(line_timestamp("ERROR boom, no timestamp here").is_none());
    }

    #[test]
    fn test_is_recent_true_for_future_timestamp() {
        let now = Utc::now();
        let future = now + chrono::Duration::seconds(30);
        assert!(is_recent(future, now));
    }

    #[test]
    fn test_is_recent_false_for_old_timestamp() {
        let now = Utc::now();
        let old = now - chrono::Duration::seconds(RECENCY_WINDOW_SECS + 1);
        assert!(!is_recent(old, now));
    }

    #[test]
    fn test_newest_log_in_picks_most_recently_modified() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("pulpod.log.2026-01-01-00");
        let new = dir.path().join("pulpod.log.2026-01-01-01");
        std::fs::write(&old, "old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&new, "new").unwrap();
        assert_eq!(newest_log_in(dir.path()), Some(new));
    }

    #[test]
    fn test_newest_log_in_ignores_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("other.txt"), "x").unwrap();
        assert_eq!(newest_log_in(dir.path()), None);
    }

    #[test]
    fn test_newest_log_in_missing_dir() {
        assert_eq!(newest_log_in(Path::new("/nonexistent/logs")), None);
    }

    #[test]
    fn test_resolve_daemon_data_dir_default_without_config() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_daemon_data_dir(home.path()),
            home.path().join(".pulpo")
        );
    }

    #[test]
    fn test_resolve_daemon_data_dir_default_when_config_unparseable() {
        let home = tempfile::tempdir().unwrap();
        let pulpo_dir = home.path().join(".pulpo");
        std::fs::create_dir_all(&pulpo_dir).unwrap();
        std::fs::write(pulpo_dir.join("config.toml"), "not valid toml {{{").unwrap();
        assert_eq!(resolve_daemon_data_dir(home.path()), pulpo_dir);
    }

    #[test]
    fn test_resolve_daemon_data_dir_default_when_no_data_dir_key() {
        let home = tempfile::tempdir().unwrap();
        let pulpo_dir = home.path().join(".pulpo");
        std::fs::create_dir_all(&pulpo_dir).unwrap();
        std::fs::write(pulpo_dir.join("config.toml"), "[node]\nname = \"x\"\n").unwrap();
        assert_eq!(resolve_daemon_data_dir(home.path()), pulpo_dir);
    }

    #[test]
    fn test_resolve_daemon_data_dir_default_when_blank_data_dir() {
        let home = tempfile::tempdir().unwrap();
        let pulpo_dir = home.path().join(".pulpo");
        std::fs::create_dir_all(&pulpo_dir).unwrap();
        std::fs::write(
            pulpo_dir.join("config.toml"),
            "[node]\ndata_dir = \"   \"\n",
        )
        .unwrap();
        assert_eq!(resolve_daemon_data_dir(home.path()), pulpo_dir);
    }

    #[test]
    fn test_resolve_daemon_data_dir_from_config_override() {
        let home = tempfile::tempdir().unwrap();
        let pulpo_dir = home.path().join(".pulpo");
        std::fs::create_dir_all(&pulpo_dir).unwrap();
        let custom_data_dir = home.path().join("custom-data");
        std::fs::write(
            pulpo_dir.join("config.toml"),
            format!("[node]\ndata_dir = \"{}\"\n", custom_data_dir.display()),
        )
        .unwrap();
        assert_eq!(resolve_daemon_data_dir(home.path()), custom_data_dir);
    }

    #[test]
    fn test_candidate_log_paths_with_brew_log_includes_both_when_present() {
        let home = tempfile::tempdir().unwrap();
        let logs_dir = home.path().join(".pulpo").join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::write(logs_dir.join("pulpod.log.2026-01-01-00"), "ERROR boom").unwrap();
        let brew_dir = tempfile::tempdir().unwrap();
        let brew_log = brew_dir.path().join("pulpo.log");
        std::fs::write(&brew_log, "ERROR brew boom").unwrap();

        let candidates = candidate_log_paths_with_brew_log(home.path(), &brew_log);
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn test_candidate_log_paths_with_brew_log_omits_missing_brew_log() {
        let home = tempfile::tempdir().unwrap();
        let logs_dir = home.path().join(".pulpo").join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::write(logs_dir.join("pulpod.log.2026-01-01-00"), "ERROR boom").unwrap();

        let candidates =
            candidate_log_paths_with_brew_log(home.path(), Path::new("/nonexistent/pulpo.log"));
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn test_candidate_log_paths_with_brew_log_empty_when_neither_present() {
        let home = tempfile::tempdir().unwrap();
        let candidates =
            candidate_log_paths_with_brew_log(home.path(), Path::new("/nonexistent/pulpo.log"));
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_candidate_log_paths_uses_real_brew_log_path() {
        let home = tempfile::tempdir().unwrap();
        // `candidate_log_paths` is documented as exactly
        // `candidate_log_paths_with_brew_log` pinned to the real, fixed Homebrew
        // service log path (`/opt/homebrew/var/log/pulpo.log`) — assert that
        // delegation directly, rather than a `len() <= 2` bound every possible
        // result (0, 1, or 2 candidates) trivially satisfies regardless of whether
        // the real entrypoint is wired to the right path at all. Both calls read
        // the same fixed path at effectively the same instant, so this stays exact
        // regardless of whether the test machine actually has that Homebrew log.
        let candidates = candidate_log_paths(home.path());
        let expected = candidate_log_paths_with_brew_log(
            home.path(),
            Path::new("/opt/homebrew/var/log/pulpo.log"),
        );
        assert_eq!(candidates, expected);
    }

    #[test]
    fn test_tail_daemon_error_lines_finds_daemon_log() {
        let home = tempfile::tempdir().unwrap();
        // `tail_daemon_error_lines` resolves `$HOME` itself via `dirs::home_dir()`,
        // which we can't override from a test without a process-wide env
        // mutation racy with other tests — so exercise the rest of the pipeline
        // (candidate resolution + tailing) through the parameterized helpers
        // instead, and separately smoke-test the public entrypoint below.
        let logs_dir = resolve_daemon_data_dir(home.path()).join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::write(logs_dir.join("pulpod.log.2026-01-01-00"), "ERROR boom\n").unwrap();
        let candidates = candidate_log_paths(home.path());
        let now = Utc::now();
        let lines = candidates
            .iter()
            .find_map(|path| {
                let lines = tail_error_lines_from(path, 3, now);
                (!lines.is_empty()).then_some(lines)
            })
            .unwrap_or_default();
        assert_eq!(lines, vec!["ERROR boom".to_string()]);
    }

    #[test]
    fn test_tail_daemon_error_lines_smoke() {
        // Exercises the real public entrypoint against whatever `$HOME` happens
        // to resolve to in CI/dev — it must never panic regardless of what's on
        // disk there.
        let _ = tail_daemon_error_lines(3);
    }
}
