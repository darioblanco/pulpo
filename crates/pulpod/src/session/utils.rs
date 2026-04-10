use std::collections::HashMap;

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

const SHELL_COMMANDS: &[&str] = &["bash", "zsh", "sh", "fish", "nu"];

/// Check if a command is a bare shell (no agent work to wrap).
pub fn is_shell_command(command: &str) -> bool {
    let basename = command.rsplit('/').next().unwrap_or(command).trim();
    SHELL_COMMANDS.contains(&basename)
}

/// Create a git worktree for a session.
/// Worktrees are created under `~/.pulpo/worktrees/<session-name>` to avoid
/// polluting the project repository with a `.pulpo/` directory.
/// Returns the worktree path on success.
#[cfg_attr(coverage, allow(dead_code))]
pub fn create_worktree(
    repo_dir: &str,
    session_name: &str,
    worktree_base: Option<&str>,
) -> Result<String> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let wt_base_dir = home.join(".pulpo").join("worktrees");
    let worktree_dir = wt_base_dir.join(session_name);
    let worktree_dir_str = worktree_dir
        .to_str()
        .context("worktree path contains invalid UTF-8")?
        .to_owned();
    let branch_name = session_name.to_owned();

    std::fs::create_dir_all(&wt_base_dir)?;

    let mut args = vec![
        "worktree".to_owned(),
        "add".to_owned(),
        "-b".to_owned(),
        branch_name.clone(),
        worktree_dir_str.clone(),
    ];
    if let Some(base) = worktree_base {
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

    Ok(worktree_dir_str)
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
) -> String {
    wrap_command(command, session_id, session_name, secrets_file, None)
}

/// Wrap an agent command with env vars, exit marker, and fallback shell.
pub fn wrap_command(
    command: &str,
    session_id: &uuid::Uuid,
    session_name: &str,
    secrets_file: Option<&str>,
    term_program: Option<&str>,
) -> String {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_owned());
    let secrets_source =
        secrets_file.map_or_else(String::new, |path| format!(". {path} && rm -f {path}; "));
    let safe_name = session_name.replace('\'', "'\\''");
    let term_program_export = term_program.map_or_else(String::new, |tp| {
        let safe_tp = tp.replace('\'', "'\\''");
        format!("export TERM_PROGRAM='{safe_tp}'; ")
    });

    let env = format!(
        "{secrets_source}export PULPO_SESSION_ID={session_id}; export PULPO_SESSION_NAME={safe_name}; {term_program_export}export BROWSER=true; \
         open() {{ case \"$1\" in http://*|https://*) return 0;; *) command open \"$@\";; esac; }}; "
    );

    if is_shell_command(command) {
        let escaped = command.replace('\'', "'\\''");
        return format!("{shell} -l -c '{env}exec {escaped}'");
    }
    let escaped = command.replace('\'', "'\\''");
    format!(
        "{shell} -l -c '{env}{escaped}; echo '\\''[pulpo] Agent exited (session: {safe_name}). Run: pulpo resume {safe_name}'\\''; exec {shell} -l'"
    )
}
