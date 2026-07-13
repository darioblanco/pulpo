use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use anyhow::{Result, anyhow, bail};

/// Validate that a session name is safe for shell interpolation and tmux usage.
/// Allows lowercase alphanumeric characters and hyphens (kebab-case).
/// Must start and end with alphanumeric. Max 128 chars.
pub fn validate_session_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("session name must not be empty");
    }
    if name.len() > 128 {
        bail!("session name must be at most 128 characters");
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        bail!("session name must contain only lowercase letters, digits, and hyphens: {name}");
    }
    if name.starts_with('-') || name.ends_with('-') {
        bail!("session name must not start or end with a hyphen: {name}");
    }
    Ok(())
}

pub fn validate_workdir(workdir: &str) -> Result<()> {
    let path = std::path::Path::new(workdir);
    if !path.exists() {
        bail!("working directory does not exist: {workdir}");
    }
    if !path.is_dir() {
        bail!("working directory is not a directory: {workdir}");
    }
    Ok(())
}

/// Error message returned wherever the retired docker runtime is requested.
pub const DOCKER_RUNTIME_REMOVED: &str = "the docker runtime was removed; sessions run in tmux";

/// Validate a session runtime. Only tmux is accepted — the docker session
/// runtime was removed. Historical sessions stored with `runtime = "docker"`
/// remain readable, but nothing new can be spawned or resumed with it.
pub fn validate_runtime(runtime: pulpo_common::session::Runtime) -> Result<()> {
    if runtime == pulpo_common::session::Runtime::Docker {
        bail!(DOCKER_RUNTIME_REMOVED);
    }
    Ok(())
}

const SHELL_COMMANDS: &[&str] = &["bash", "zsh", "sh", "fish", "nu"];

/// Check if a command is a bare shell (no agent work to wrap).
pub fn is_shell_command(command: &str) -> bool {
    let basename = command.rsplit('/').next().unwrap_or(command).trim();
    SHELL_COMMANDS.contains(&basename)
}

/// Create a git worktree for a session.
/// Worktrees are created under `<worktrees_dir>/<session-name>` (the daemon passes
/// `{data_dir}/worktrees`, which is `~/.pulpo/worktrees` by default) to avoid
/// polluting the project repository with a `.pulpo/` directory.
/// `base_ref`, when set, is the git ref to branch the worktree from.
/// Returns the worktree path on success.
#[cfg_attr(coverage, allow(dead_code))]
pub fn create_worktree(
    worktrees_dir: &Path,
    repo_dir: &str,
    session_name: &str,
    base_ref: Option<&str>,
) -> Result<String> {
    let target_dir = worktrees_dir.join(session_name);
    let target_dir_str = target_dir
        .to_str()
        .context("worktree path contains invalid UTF-8")?
        .to_owned();
    let branch_name = session_name.to_owned();

    std::fs::create_dir_all(worktrees_dir)?;

    let mut args = vec![
        "worktree".to_owned(),
        "add".to_owned(),
        "-b".to_owned(),
        branch_name.clone(),
        target_dir_str.clone(),
    ];
    if let Some(base) = base_ref {
        args.push(base.to_owned());
    }
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();

    let output = std::process::Command::new("git")
        .args(&args_ref)
        .current_dir(repo_dir)
        .output()
        .context("failed to run git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("already exists") {
            tracing::info!(branch = %branch_name, "Stale branch found, deleting and retrying");
            let _ = std::process::Command::new("git")
                .args(["branch", "-D", &branch_name])
                .current_dir(repo_dir)
                .output();
            let retry = std::process::Command::new("git")
                .args(&args_ref)
                .current_dir(repo_dir)
                .output()
                .context("failed to run git worktree add (retry)")?;
            if !retry.status.success() {
                let retry_stderr = String::from_utf8_lossy(&retry.stderr);
                bail!(
                    "git worktree add failed after branch cleanup: {}",
                    retry_stderr.trim()
                );
            }
        } else {
            bail!("git worktree add failed: {}", stderr.trim());
        }
    }

    Ok(target_dir_str)
}

/// Remove a git worktree, prune stale entries, and delete the associated branch.
pub fn cleanup_worktree(worktree_path: &str, repo_dir: &str) {
    if std::path::Path::new(worktree_path).exists() {
        match std::fs::remove_dir_all(worktree_path) {
            Ok(()) => tracing::info!(path = %worktree_path, "Worktree directory removed"),
            Err(e) => {
                tracing::warn!(path = %worktree_path, error = %e, "Failed to remove worktree directory");
            }
        }
    } else {
        tracing::info!(path = %worktree_path, "Worktree path does not exist, skipping cleanup");
    }

    let _ = std::process::Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo_dir)
        .output();

    if let Some(branch_name) = std::path::Path::new(worktree_path)
        .file_name()
        .and_then(|n| n.to_str())
    {
        match std::process::Command::new("git")
            .args(["branch", "-D", branch_name])
            .current_dir(repo_dir)
            .output()
        {
            Ok(output) if output.status.success() => {
                tracing::info!(branch = %branch_name, "Worktree branch deleted");
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::debug!(
                    branch = %branch_name,
                    stderr = %stderr.trim(),
                    "Branch deletion skipped (may not exist)"
                );
            }
            Err(e) => {
                tracing::warn!(
                    branch = %branch_name,
                    error = %e,
                    "Failed to run git branch -D"
                );
            }
        }
    }
}

/// Path to a session's captured-output log file: `{data_dir}/logs/{id}.log`.
pub fn session_log_path(data_dir: &str, id: &str) -> PathBuf {
    Path::new(data_dir).join("logs").join(format!("{id}.log"))
}

/// Remove a session's captured-output log file if it exists.
/// Returns `true` when a file was actually removed.
pub fn remove_session_log(data_dir: &str, id: &str) -> bool {
    let path = session_log_path(data_dir, id);
    path.exists() && std::fs::remove_file(&path).is_ok()
}

/// Directory holding per-session worktrees (`{data_dir}/worktrees`).
pub fn worktrees_dir(data_dir: &str) -> PathBuf {
    Path::new(data_dir).join("worktrees")
}

/// Find orphaned worktree directories: immediate subdirectories of `worktrees_dir`
/// whose absolute path is not referenced by any live session. These belong to
/// sessions that were already deleted from the database but whose directory leaked.
/// Safe by construction — a directory still referenced by any session is never returned.
pub fn find_orphan_worktree_dirs(
    worktrees_dir: &Path,
    referenced: &HashSet<String>,
) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(worktrees_dir) else {
        return Vec::new();
    };
    let mut orphans = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !referenced.contains(&path.to_string_lossy().into_owned()) {
            orphans.push(path);
        }
    }
    orphans
}

/// Find orphaned per-session log files: `{uuid}.log` files in `logs_dir` whose UUID
/// is not a known session id. The rolling daemon log (`pulpod.log.*`) is never
/// returned — only files whose stem parses as a UUID are considered session logs.
pub fn find_orphan_session_logs(logs_dir: &Path, known_ids: &HashSet<String>) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(logs_dir) else {
        return Vec::new();
    };
    let mut orphans = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".log") else {
            continue;
        };
        // Only treat `{uuid}.log` as a session log; skip `pulpod.log` and rotations.
        if uuid::Uuid::parse_str(stem).is_err() {
            continue;
        }
        if !known_ids.contains(stem) {
            orphans.push(path);
        }
    }
    orphans
}

/// Directory holding per-session exit markers (`{data_dir}/exit`).
pub fn exit_dir(data_dir: &str) -> PathBuf {
    Path::new(data_dir).join("exit")
}

/// Path to a session's exit-code marker: `{data_dir}/exit/{id}.code`.
pub fn exit_code_marker_path(data_dir: &str, id: &str) -> PathBuf {
    exit_dir(data_dir).join(format!("{id}.code"))
}

/// Path to a session's clean-exit marker: `{data_dir}/exit/{id}.clean`.
pub fn exit_clean_marker_path(data_dir: &str, id: &str) -> PathBuf {
    exit_dir(data_dir).join(format!("{id}.clean"))
}

/// Read a session's recorded exit code from its `.code` marker, if present and parseable.
pub fn read_exit_code_marker(data_dir: &str, id: &str) -> Option<i32> {
    std::fs::read_to_string(exit_code_marker_path(data_dir, id))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// True when either exit marker exists — evidence the wrapped shell ran to
/// completion (an intentional end), as opposed to the tmux process disappearing
/// mid-run (crash / `kill-session` / `kill-server` / reboot).
pub fn has_exit_marker(data_dir: &str, id: &str) -> bool {
    exit_code_marker_path(data_dir, id).exists() || exit_clean_marker_path(data_dir, id).exists()
}

/// Remove both exit marker files for a session (best-effort). Returns true if at
/// least one was actually removed.
pub fn remove_exit_markers(data_dir: &str, id: &str) -> bool {
    let a = {
        let p = exit_code_marker_path(data_dir, id);
        p.exists() && std::fs::remove_file(&p).is_ok()
    };
    let b = {
        let p = exit_clean_marker_path(data_dir, id);
        p.exists() && std::fs::remove_file(&p).is_ok()
    };
    a || b
}

/// Find orphaned exit-marker files: `{uuid}.code`/`{uuid}.clean` in `exit_dir` whose
/// UUID is not a known session id. Mirrors `find_orphan_session_logs`.
pub fn find_orphan_exit_markers(exit_dir: &Path, known_ids: &HashSet<String>) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(exit_dir) else {
        return Vec::new();
    };
    let mut orphans = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = name
            .strip_suffix(".code")
            .or_else(|| name.strip_suffix(".clean"))
        else {
            continue;
        };
        if uuid::Uuid::parse_str(stem).is_err() {
            continue;
        }
        if !known_ids.contains(stem) {
            orphans.push(path);
        }
    }
    orphans
}

/// Write secrets to a sourced-and-deleted file under the pulpo data dir.
pub fn write_secrets_file(
    session_id: &uuid::Uuid,
    secrets: &HashMap<String, String>,
    data_dir: &str,
) -> Result<Option<String>> {
    use std::fmt::Write;
    use std::io::Write as IoWrite;

    if secrets.is_empty() {
        return Ok(None);
    }

    let mut content = String::new();
    for (key, value) in secrets {
        let escaped_value = value.replace('\'', "'\\''");
        let _ = writeln!(content, "export {key}='{escaped_value}'");
    }

    let secrets_dir = format!("{data_dir}/secrets");
    std::fs::create_dir_all(&secrets_dir)
        .map_err(|e| anyhow!("failed to create secrets directory {secrets_dir}: {e}"))?;

    let path = format!("{secrets_dir}/secrets-{session_id}.sh");

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| anyhow!("failed to create secrets file {path}: {e}"))?;
        file.write_all(content.as_bytes())
            .map_err(|e| anyhow!("failed to write secrets file {path}: {e}"))?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(&path, &content)
            .map_err(|e| anyhow!("failed to write secrets file {path}: {e}"))?;
    }

    Ok(Some(path))
}

#[cfg(test)]
#[allow(dead_code)]
pub fn wrap_command_for_test(
    command: &str,
    session_id: &uuid::Uuid,
    session_name: &str,
    secrets_file: Option<&str>,
    data_dir: &str,
) -> String {
    wrap_command(
        command,
        session_id,
        session_name,
        secrets_file,
        None,
        data_dir,
    )
}

/// Wrap a command with env vars, exit markers, and (for agent commands) a fallback shell.
///
/// Two exit markers are written under `{data_dir}/exit/`, read fresh from disk by the
/// daemon whenever it next checks (race-free even across a daemon restart):
///   - `{id}.code`  — the wrapped agent command's exit code (`$?`), agent path only.
///   - `{id}.clean` — written by the main/fallback shell as the very last thing it
///     does before exiting normally, on **both** paths.
///
/// Presence of either marker is what lets the daemon tell "the session ended on its
/// own" (`Stopped`) apart from "tmux disappeared out from under a live session"
/// (`Lost`) — see `SessionManager::check_and_mark_stale`.
///
/// `exec` is deliberately NOT used on either path (unlike the historical wrapper):
/// exec'ing would replace the wrapper shell's process image, so nothing could run
/// after the wrapped command / fallback shell exits. Running it as a plain (non-exec'd)
/// command lets the wrapper shell regain control and write the `.clean` marker right
/// before it terminates.
pub fn wrap_command(
    command: &str,
    session_id: &uuid::Uuid,
    session_name: &str,
    secrets_file: Option<&str>,
    term_program: Option<&str>,
    data_dir: &str,
) -> String {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_owned());
    let secrets_source =
        secrets_file.map_or_else(String::new, |path| format!(". {path} && rm -f {path}; "));
    let safe_name = session_name.replace('\'', "'\\''");
    let term_program_export = term_program.map_or_else(String::new, |tp| {
        let safe_tp = tp.replace('\'', "'\\''");
        format!("export TERM_PROGRAM='{safe_tp}'; ")
    });

    // Quote the exit-marker directory defensively — data_dir may contain spaces —
    // using the same `'\''` shell-escape idiom already relied on for safe_name/safe_tp.
    let exit_dir_str = exit_dir(data_dir).to_string_lossy().into_owned();
    let safe_exit_dir = exit_dir_str.replace('\'', "'\\''");
    let code_path = format!("{safe_exit_dir}/{session_id}.code");
    let clean_path = format!("{safe_exit_dir}/{session_id}.clean");

    let env = format!(
        "mkdir -p '\\''{safe_exit_dir}'\\''; {secrets_source}export PULPO_SESSION_ID={session_id}; export PULPO_SESSION_NAME={safe_name}; {term_program_export}export BROWSER=true; \
         open() {{ case \"$1\" in http://*|https://*) return 0;; *) command open \"$@\";; esac; }}; "
    );

    if is_shell_command(command) {
        let escaped = command.replace('\'', "'\\''");
        return format!("{shell} -l -c '{env}{escaped}; : > '\\''{clean_path}'\\'''");
    }
    let escaped = command.replace('\'', "'\\''");
    format!(
        "{shell} -l -c '{env}{escaped}; ec=$?; echo \"$ec\" > '\\''{code_path}'\\''; echo '\\''[pulpo] Agent exited (session: {safe_name}). Run: pulpo resume {safe_name}'\\''; {shell} -l; : > '\\''{clean_path}'\\'''"
    )
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;

    #[test]
    fn test_session_log_path_format() {
        let p = session_log_path("/data", "abc");
        assert_eq!(p, Path::new("/data/logs/abc.log"));
    }

    #[test]
    fn test_worktrees_dir_format() {
        assert_eq!(worktrees_dir("/data"), Path::new("/data/worktrees"));
    }

    #[test]
    fn test_remove_session_log_removes_existing_and_reports_false_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().to_str().unwrap();
        let id = "11111111-1111-1111-1111-111111111111";
        // Absent → false.
        assert!(!remove_session_log(data_dir, id));
        // Create then remove → true, file gone.
        let path = session_log_path(data_dir, id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"output").unwrap();
        assert!(remove_session_log(data_dir, id));
        assert!(!path.exists());
    }

    #[test]
    fn test_find_orphan_worktree_dirs_returns_unreferenced_only() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        let kept = base.join("kept");
        let orphan = base.join("orphan");
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::create_dir_all(&orphan).unwrap();
        // A stray file (not a dir) must be ignored.
        std::fs::write(base.join("stray.txt"), b"x").unwrap();

        let mut referenced = HashSet::new();
        referenced.insert(kept.to_string_lossy().into_owned());

        let orphans = find_orphan_worktree_dirs(base, &referenced);
        assert_eq!(orphans, vec![orphan]);
    }

    #[test]
    fn test_find_orphan_worktree_dirs_missing_base_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");
        assert!(find_orphan_worktree_dirs(&missing, &HashSet::new()).is_empty());
    }

    #[test]
    fn test_find_orphan_session_logs_skips_known_and_non_uuid() {
        let tmp = tempfile::tempdir().unwrap();
        let logs = tmp.path();
        let known = "22222222-2222-2222-2222-222222222222";
        let orphan = "33333333-3333-3333-3333-333333333333";
        std::fs::write(logs.join(format!("{known}.log")), b"a").unwrap();
        std::fs::write(logs.join(format!("{orphan}.log")), b"b").unwrap();
        // Rolling daemon log + a rotation + a non-uuid file: all must be ignored.
        std::fs::write(logs.join("pulpod.log"), b"c").unwrap();
        std::fs::write(logs.join("pulpod.log.2026-06-07-12"), b"d").unwrap();
        std::fs::write(logs.join("notes.txt"), b"e").unwrap();

        let mut known_ids = HashSet::new();
        known_ids.insert(known.to_owned());

        let orphans = find_orphan_session_logs(logs, &known_ids);
        assert_eq!(orphans.len(), 1);
        assert_eq!(
            orphans[0].file_name().unwrap(),
            format!("{orphan}.log").as_str()
        );
    }

    #[test]
    fn test_find_orphan_session_logs_missing_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");
        assert!(find_orphan_session_logs(&missing, &HashSet::new()).is_empty());
    }

    #[test]
    fn test_exit_dir_format() {
        assert_eq!(exit_dir("/data"), Path::new("/data/exit"));
    }

    #[test]
    fn test_exit_code_marker_path_format() {
        let p = exit_code_marker_path("/data", "abc");
        assert_eq!(p, Path::new("/data/exit/abc.code"));
    }

    #[test]
    fn test_exit_clean_marker_path_format() {
        let p = exit_clean_marker_path("/data", "abc");
        assert_eq!(p, Path::new("/data/exit/abc.clean"));
    }

    #[test]
    fn test_read_exit_code_marker_absent_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().to_str().unwrap();
        assert!(read_exit_code_marker(data_dir, "no-such-id").is_none());
    }

    #[test]
    fn test_read_exit_code_marker_parses_value() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().to_str().unwrap();
        let id = "11111111-1111-1111-1111-111111111111";
        let path = exit_code_marker_path(data_dir, id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "0\n").unwrap();
        assert_eq!(read_exit_code_marker(data_dir, id), Some(0));
    }

    #[test]
    fn test_read_exit_code_marker_nonzero_value() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().to_str().unwrap();
        let id = "11111111-1111-1111-1111-111111111112";
        let path = exit_code_marker_path(data_dir, id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "130\n").unwrap();
        assert_eq!(read_exit_code_marker(data_dir, id), Some(130));
    }

    #[test]
    fn test_read_exit_code_marker_unparseable_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().to_str().unwrap();
        let id = "11111111-1111-1111-1111-111111111113";
        let path = exit_code_marker_path(data_dir, id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not-a-number").unwrap();
        assert!(read_exit_code_marker(data_dir, id).is_none());
    }

    #[test]
    fn test_has_exit_marker_absent_is_false() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().to_str().unwrap();
        assert!(!has_exit_marker(data_dir, "no-such-id"));
    }

    #[test]
    fn test_has_exit_marker_true_with_only_clean_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().to_str().unwrap();
        let id = "22222222-2222-2222-2222-222222222221";
        let path = exit_clean_marker_path(data_dir, id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        assert!(has_exit_marker(data_dir, id));
    }

    #[test]
    fn test_has_exit_marker_true_with_only_code_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().to_str().unwrap();
        let id = "22222222-2222-2222-2222-222222222222";
        let path = exit_code_marker_path(data_dir, id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "0").unwrap();
        assert!(has_exit_marker(data_dir, id));
    }

    #[test]
    fn test_remove_exit_markers_removes_both_and_reports_false_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().to_str().unwrap();
        let id = "33333333-3333-3333-3333-333333333331";
        // Absent → false.
        assert!(!remove_exit_markers(data_dir, id));

        let code_path = exit_code_marker_path(data_dir, id);
        let clean_path = exit_clean_marker_path(data_dir, id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();
        std::fs::write(&clean_path, "").unwrap();

        assert!(remove_exit_markers(data_dir, id));
        assert!(!code_path.exists());
        assert!(!clean_path.exists());
        // Idempotent — a second call finds nothing left to remove.
        assert!(!remove_exit_markers(data_dir, id));
    }

    #[test]
    fn test_find_orphan_exit_markers_skips_known_and_non_uuid() {
        let tmp = tempfile::tempdir().unwrap();
        let exit = tmp.path();
        let known = "44444444-4444-4444-4444-444444444441";
        let orphan = "44444444-4444-4444-4444-444444444442";
        std::fs::write(exit.join(format!("{known}.code")), b"0").unwrap();
        std::fs::write(exit.join(format!("{orphan}.code")), b"0").unwrap();
        std::fs::write(exit.join(format!("{orphan}.clean")), b"").unwrap();
        std::fs::write(exit.join("notes.txt"), b"e").unwrap();

        let mut known_ids = HashSet::new();
        known_ids.insert(known.to_owned());

        let mut orphans = find_orphan_exit_markers(exit, &known_ids);
        orphans.sort();
        assert_eq!(orphans.len(), 2);
        assert!(
            orphans
                .iter()
                .any(|p| p.file_name().unwrap() == format!("{orphan}.code").as_str())
        );
        assert!(
            orphans
                .iter()
                .any(|p| p.file_name().unwrap() == format!("{orphan}.clean").as_str())
        );
    }

    #[test]
    fn test_find_orphan_exit_markers_missing_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");
        assert!(find_orphan_exit_markers(&missing, &HashSet::new()).is_empty());
    }
}

/// Real-`git` integration tests for the worktree lifecycle. Gated `not(coverage)` (like
/// the real-tmux tests): they run in the CI `Test` job, which has git, and are excluded
/// from the coverage build, which has no real repos.
#[cfg(all(test, not(coverage)))]
mod git_integration_tests {
    use super::*;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git should run")
    }

    fn init_repo(repo: &Path) {
        std::fs::create_dir_all(repo).unwrap();
        git(repo, &["init", "-q"]);
        git(repo, &["config", "user.email", "qa@pulpo.test"]);
        git(repo, &["config", "user.name", "pulpo-qa"]);
        std::fs::write(repo.join("README.md"), "seed").unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-q", "-m", "init"]);
    }

    fn contains(out: &std::process::Output, needle: &str) -> bool {
        String::from_utf8_lossy(&out.stdout).contains(needle)
    }

    #[test]
    fn test_worktree_create_then_cleanup_lifecycle() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        let wt_base = tmp.path().join("worktrees");
        let repo_str = repo.to_str().unwrap();

        // Create: directory exists, branch exists, registered as a worktree.
        let wt_path = create_worktree(&wt_base, repo_str, "fix-auth", None).unwrap();
        assert!(Path::new(&wt_path).exists(), "worktree dir should exist");
        assert!(
            contains(&git(&repo, &["branch", "--list", "fix-auth"]), "fix-auth"),
            "branch should be created"
        );
        assert!(
            contains(&git(&repo, &["worktree", "list"]), "fix-auth"),
            "worktree should be registered"
        );

        // Cleanup: directory removed, branch deleted, worktree pruned.
        cleanup_worktree(&wt_path, repo_str);
        assert!(
            !Path::new(&wt_path).exists(),
            "worktree dir should be removed"
        );
        assert!(
            !contains(&git(&repo, &["branch", "--list", "fix-auth"]), "fix-auth"),
            "branch should be deleted"
        );
        assert!(
            !contains(&git(&repo, &["worktree", "list"]), "fix-auth"),
            "worktree should be pruned"
        );
    }

    #[test]
    fn test_create_worktree_recovers_from_stale_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        let wt_base = tmp.path().join("worktrees");
        let repo_str = repo.to_str().unwrap();

        // Pre-create a branch with the session's name (no worktree) → "already exists".
        git(&repo, &["branch", "reuse-me"]);
        // create_worktree should delete the stale branch and retry successfully.
        let wt_path = create_worktree(&wt_base, repo_str, "reuse-me", None).unwrap();
        assert!(
            Path::new(&wt_path).exists(),
            "worktree created after stale-branch recovery"
        );

        cleanup_worktree(&wt_path, repo_str);
    }
}
