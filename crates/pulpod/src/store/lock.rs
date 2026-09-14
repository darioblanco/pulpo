//! Single-instance guard: an exclusive advisory lock on `{data_dir}/pulpod.lock`.
//!
//! `state.db` is opened (and, on an unusable-database error, possibly quarantined —
//! see [`super::open_and_migrate`]) well before `pulpod` binds its HTTP port
//! (`main.rs` binds the listener only after `build_app` returns). Without a guard, a
//! second `pulpod` accidentally started against the same data directory during that
//! window could quarantine (rename away) a database the first, already-running
//! instance still holds open and considers healthy. [`try_acquire`] must be called
//! before [`super::open_and_migrate`] — see `build_app` in `lib.rs`.
//!
//! Backed by `flock(2)` (via `rustix`), which the OS releases automatically when the
//! holding process exits for any reason (clean shutdown, crash, `SIGKILL`) — unlike a
//! bare PID file, there is no stale-lock state to detect or clean up on the next
//! start.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustix::fs::{FlockOperation, flock};
use rustix::io::Errno;

/// Holds the single-instance lock for as long as this value is alive — store it
/// somewhere that lives for the daemon's whole run (see `ShutdownHandle` in
/// `lib.rs`). Dropping it early releases the lock while `pulpod` keeps running.
pub struct SingleInstanceLock {
    // Never read; kept only so the OS releases the flock when this (and thus its
    // file descriptor) is dropped, or the process exits.
    _file: File,
}

/// The result of [`try_acquire`].
pub enum LockOutcome {
    /// The lock was free and is now held by this process.
    Acquired(SingleInstanceLock),
    /// Another process already holds the lock. `holder_pid` is a best-effort read
    /// of the PID that process recorded in the lock file when it acquired it —
    /// `None` if the file was empty/unreadable/unparseable (a rare race, or a lock
    /// file from a version of pulpod that predates recording a PID at all). This is
    /// purely informational: the caller must refuse to start either way.
    HeldByAnother { holder_pid: Option<u32> },
}

fn lock_path(data_dir: &str) -> PathBuf {
    Path::new(data_dir).join("pulpod.lock")
}

/// Try to acquire the single-instance lock at `{data_dir}/pulpod.lock`. Must be
/// called before opening `state.db`.
///
/// `Err` only for a failure unrelated to contention — e.g. the data directory
/// doesn't exist or isn't writable. Contention itself (`LockOutcome::HeldByAnother`)
/// is not an error: it's the expected, correctly-detected "someone else is already
/// running" case.
pub fn try_acquire(data_dir: &str) -> Result<LockOutcome> {
    let lock_path = lock_path(data_dir);
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false) // preserve any existing PID so a losing acquirer can read it
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("failed to open lock file {}", lock_path.display()))?;

    match flock(&file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => {
            // Best-effort: record our PID so a later contending process can report
            // who's holding the lock. Never fatal on its own — holding the flock is
            // what actually matters.
            let _ = write_own_pid(&mut file);
            Ok(LockOutcome::Acquired(SingleInstanceLock { _file: file }))
        }
        Err(errno) => match classify_flock_error(errno) {
            None => Ok(LockOutcome::HeldByAnother {
                holder_pid: read_holder_pid(&mut file),
            }),
            Some(io_err) => {
                Err(io_err).with_context(|| format!("failed to lock {}", lock_path.display()))
            }
        },
    }
}

/// Classify a `flock` failure: `None` means ordinary contention (another process
/// already holds the lock — the expected, non-error "someone else is running"
/// case); `Some(err)` means something else went wrong (permissions, an
/// unsupported filesystem, ...) that the caller should surface as a real error.
///
/// `flock(2)` with `LOCK_NB` reports contention as `EWOULDBLOCK` (POSIX); `EAGAIN`
/// is checked too since not every platform/libc pairing is guaranteed to keep them
/// distinct constants even though their values coincide on Linux and macOS.
fn classify_flock_error(errno: Errno) -> Option<std::io::Error> {
    if errno == Errno::WOULDBLOCK || errno == Errno::AGAIN {
        None
    } else {
        Some(std::io::Error::from(errno))
    }
}

fn write_own_pid(file: &mut File) -> std::io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    write!(file, "{}", std::process::id())?;
    file.flush()
}

fn read_holder_pid(file: &mut File) -> Option<u32> {
    let mut contents = String::new();
    file.seek(SeekFrom::Start(0)).ok()?;
    file.read_to_string(&mut contents).ok()?;
    contents.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_acquire_succeeds_on_a_free_lock() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path().to_str().unwrap();

        match try_acquire(dir).unwrap() {
            LockOutcome::Acquired(_lock) => {}
            LockOutcome::HeldByAnother { .. } => panic!("expected the lock to be free"),
        }
        assert!(lock_path(dir).exists());
    }

    #[test]
    fn test_try_acquire_reports_contention_while_held() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path().to_str().unwrap();

        let first = try_acquire(dir).unwrap();
        let LockOutcome::Acquired(_held) = first else {
            panic!("expected the first acquire to succeed");
        };

        match try_acquire(dir).unwrap() {
            LockOutcome::HeldByAnother { holder_pid } => {
                assert_eq!(holder_pid, Some(std::process::id()));
            }
            LockOutcome::Acquired(_) => {
                panic!("a second acquire must not succeed while the first is held")
            }
        }
        // `_held` (and thus the lock) is dropped at the end of this scope.
    }

    #[test]
    fn test_try_acquire_succeeds_again_after_the_lock_is_dropped() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path().to_str().unwrap();

        {
            let LockOutcome::Acquired(_held) = try_acquire(dir).unwrap() else {
                panic!("expected the first acquire to succeed");
            };
        }
        // The guard above was dropped, releasing the OS-level flock.

        match try_acquire(dir).unwrap() {
            LockOutcome::Acquired(_) => {}
            LockOutcome::HeldByAnother { .. } => {
                panic!("expected the lock to be free again after the holder was dropped")
            }
        }
    }

    #[test]
    fn test_try_acquire_errs_when_data_dir_missing() {
        let tmpdir = tempfile::tempdir().unwrap();
        let missing = tmpdir.path().join("does-not-exist");
        let result = try_acquire(missing.to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_read_holder_pid_none_for_empty_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("empty.lock");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert_eq!(read_holder_pid(&mut file), None);
    }

    #[test]
    fn test_read_holder_pid_none_for_garbage_content() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("garbage.lock");
        std::fs::write(&path, b"not-a-pid").unwrap();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert_eq!(read_holder_pid(&mut file), None);
    }

    #[test]
    fn test_classify_flock_error_wouldblock_is_contention() {
        assert!(classify_flock_error(Errno::WOULDBLOCK).is_none());
    }

    #[test]
    fn test_classify_flock_error_again_is_contention() {
        assert!(classify_flock_error(Errno::AGAIN).is_none());
    }

    #[test]
    fn test_classify_flock_error_other_errno_is_a_real_error() {
        let err = classify_flock_error(Errno::PERM).expect("expected a real error, not contention");
        assert_eq!(err.kind(), std::io::Error::from(Errno::PERM).kind());
    }
}
