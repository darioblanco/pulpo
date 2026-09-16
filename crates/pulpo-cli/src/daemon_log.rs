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
//! `pulpo-cli` doesn't depend on the `pulpod` crate (it's the daemon binary,
//! not a library this talks to in-process), so the daemon's data-dir default
//! and `[node] data_dir` override are re-derived here directly from
//! `~/.pulpo/config.toml` rather than reusing `pulpod::config`.

use std::path::{Path, PathBuf};

/// Resolve the data dir `pulpod` would use for `home`, mirroring
/// `pulpod::config`'s `default_data_dir()` (`~/.pulpo`) and its
/// `[node] data_dir` override.
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
            value
                .get("node")?
                .get("data_dir")?
                .as_str()
                .map(str::to_owned)
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
/// oldest first. Empty when the file doesn't exist, can't be read, or has no
/// `ERROR` lines.
fn tail_error_lines_from(path: &Path, max_lines: usize) -> Vec<String> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut errors: Vec<&str> = contents
        .lines()
        .filter(|line| line.contains("ERROR"))
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
    for path in candidate_log_paths(&home) {
        let lines = tail_error_lines_from(&path, max_lines);
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
            tail_error_lines_from(&path, 2),
            vec!["ERROR b".to_string(), "ERROR c".to_string()]
        );
    }

    #[test]
    fn test_tail_error_lines_from_fewer_errors_than_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pulpod.log");
        std::fs::write(&path, "INFO one\nERROR only\n").unwrap();
        assert_eq!(
            tail_error_lines_from(&path, 3),
            vec!["ERROR only".to_string()]
        );
    }

    #[test]
    fn test_tail_error_lines_from_missing_file() {
        assert!(tail_error_lines_from(Path::new("/nonexistent/pulpod.log"), 3).is_empty());
    }

    #[test]
    fn test_tail_error_lines_from_no_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pulpod.log");
        std::fs::write(&path, "INFO all fine\n").unwrap();
        assert!(tail_error_lines_from(&path, 3).is_empty());
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
        let lines = candidates
            .iter()
            .find_map(|path| {
                let lines = tail_error_lines_from(path, 3);
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
