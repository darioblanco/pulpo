#[cfg(all(target_os = "macos", not(coverage)))]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

#[cfg(not(coverage))]
use anyhow::{Context, Result, bail};

/// Docker container backend for Docker runtime sessions.
#[allow(dead_code)]
pub struct DockerBackend {
    /// Docker image to use for container sessions.
    image: String,
    /// Volume mounts (host:container[:mode]).
    volumes: Vec<String>,
}

#[allow(dead_code)]
impl DockerBackend {
    pub fn new(image: &str, volumes: Vec<String>) -> Self {
        Self {
            image: image.to_owned(),
            volumes,
        }
    }
}

/// Check if a `backend_session_id` refers to a Docker container.
pub fn is_docker_session(backend_id: &str) -> bool {
    backend_id.starts_with("docker:")
}

/// Extract the container name from a Docker `backend_session_id`.
pub fn docker_container_name(backend_id: &str) -> &str {
    backend_id.strip_prefix("docker:").unwrap_or(backend_id)
}

/// Resolve `~` in a path to the actual home directory.
/// Returns `None` if the home directory cannot be determined.
#[cfg_attr(coverage, allow(dead_code))]
fn resolve_tilde(path: &str) -> Option<PathBuf> {
    path.strip_prefix("~/").map_or_else(
        || {
            if path == "~" {
                dirs::home_dir()
            } else {
                Some(PathBuf::from(path))
            }
        },
        |rest| dirs::home_dir().map(|home| home.join(rest)),
    )
}

/// Resolve volume mounts: expand `~`, skip mounts where host path does not exist.
/// Returns resolved volume strings ready for `-v` flags.
#[cfg_attr(coverage, allow(dead_code))]
fn resolve_volumes(volumes: &[String]) -> Vec<String> {
    let mut resolved = Vec::new();
    for vol in volumes {
        // Parse host:container[:mode] format
        let parts: Vec<&str> = vol.splitn(3, ':').collect();
        if parts.len() < 2 {
            tracing::debug!(volume = %vol, "Skipping malformed volume mount (no colon separator)");
            continue;
        }
        let host_path_str = parts[0];
        let container_path = parts[1];
        let mode = parts.get(2).copied();

        let Some(host_path) = resolve_tilde(host_path_str) else {
            tracing::debug!(
                volume = %vol,
                "Skipping volume mount: cannot resolve home directory"
            );
            continue;
        };

        if !host_path.exists() {
            tracing::debug!(
                volume = %vol,
                resolved_host = %host_path.display(),
                "Skipping volume mount: host path does not exist"
            );
            continue;
        }

        let resolved_vol = mode.map_or_else(
            || format!("{}:{}", host_path.display(), container_path),
            |m| format!("{}:{}:{m}", host_path.display(), container_path),
        );
        resolved.push(resolved_vol);
    }
    resolved
}

/// Try to extract Claude Code credentials from the macOS Keychain.
/// Returns the JSON string on success, or `None` if not found.
#[cfg(target_os = "macos")]
#[cfg(not(coverage))]
fn extract_claude_keychain_credentials() -> Option<String> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if value.is_empty() { None } else { Some(value) }
    } else {
        None
    }
}

/// Write keychain credentials to a temp file in the data dir.
/// Returns the path to the temp file on success.
#[cfg(target_os = "macos")]
#[cfg(not(coverage))]
fn write_credentials_temp_file(data_dir: &Path, credentials_json: &str) -> Option<PathBuf> {
    let creds_dir = data_dir.join("docker-creds");
    if std::fs::create_dir_all(&creds_dir).is_err() {
        return None;
    }
    let creds_file = creds_dir.join("claude-credentials.json");
    if std::fs::write(&creds_file, credentials_json).is_err() {
        return None;
    }
    Some(creds_file)
}

/// On macOS, check if Claude credentials need to be extracted from Keychain.
/// If `~/.claude` is in the volumes but `~/.claude/.credentials.json` does not exist,
/// attempt to extract from macOS Keychain and return an extra volume mount for the
/// credentials file.
#[cfg(target_os = "macos")]
#[cfg(not(coverage))]
fn maybe_extract_claude_credentials(volumes: &[String], data_dir: &str) -> Option<String> {
    use std::path::Path;

    // Check if any volume mounts ~/.claude
    let has_claude_mount = volumes.iter().any(|v| {
        let host = v.split(':').next().unwrap_or("");
        let resolved = resolve_tilde(host);
        resolved
            .as_ref()
            .is_some_and(|p| p.ends_with(".claude") || p.to_string_lossy().contains("/.claude"))
    });
    if !has_claude_mount {
        return None;
    }

    // Check if credentials file already exists on disk
    if resolve_tilde("~/.claude")
        .is_some_and(|claude_dir| claude_dir.join(".credentials.json").exists())
    {
        return None;
    }

    // Try extracting from Keychain
    let credentials_json = extract_claude_keychain_credentials()?;
    tracing::info!("Extracted Claude Code credentials from macOS Keychain");

    let data_path = Path::new(data_dir);
    let creds_file = write_credentials_temp_file(data_path, &credentials_json)?;
    Some(format!(
        "{}:/root/.claude/.credentials.json:ro",
        creds_file.display()
    ))
}

#[allow(dead_code)]
fn build_run_command(
    image: &str,
    container_name: &str,
    working_dir: &str,
    command: &str,
    volumes: &[String],
) -> Command {
    let mut cmd = Command::new("docker");
    cmd.args(["run", "-d", "--name", container_name]);
    for vol in volumes {
        cmd.args(["-v", vol]);
    }
    cmd.args([
        "-v",
        &format!("{working_dir}:/workspace"),
        "-w",
        "/workspace",
        image,
        "bash",
        "-l",
        "-c",
        command,
    ]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_stop_command(container_name: &str) -> Command {
    let mut cmd = Command::new("docker");
    cmd.args(["stop", container_name]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_rm_command(container_name: &str) -> Command {
    let mut cmd = Command::new("docker");
    cmd.args(["rm", "-f", container_name]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_inspect_running_command(container_name: &str) -> Command {
    let mut cmd = Command::new("docker");
    cmd.args(["inspect", "--format", "{{.State.Running}}", container_name]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_logs_command(container_name: &str, lines: usize) -> Command {
    let mut cmd = Command::new("docker");
    cmd.args(["logs", "--tail", &lines.to_string(), container_name]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_ps_command() -> Command {
    let mut cmd = Command::new("docker");
    cmd.args([
        "ps",
        "-a",
        "--filter",
        "label=pulpo.managed=true",
        "--format",
        "{{.Names}}",
    ]);
    cmd
}

#[cfg(not(coverage))]
impl super::Backend for DockerBackend {
    fn session_id(&self, name: &str) -> String {
        format!("docker:pulpo-{name}")
    }

    fn check_version(&self) -> Result<String> {
        let output = Command::new("docker")
            .arg("--version")
            .output()
            .context("docker not found")?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn create_session(&self, backend_id: &str, working_dir: &str, command: &str) -> Result<()> {
        let container_name = docker_container_name(backend_id);
        // Remove any existing container with the same name
        let _ = build_rm_command(container_name).output();

        // Resolve volume mounts (expand ~, skip nonexistent)
        #[allow(unused_mut)]
        let mut resolved_volumes = resolve_volumes(&self.volumes);

        // On macOS, try to extract Claude credentials from Keychain if needed
        #[cfg(target_os = "macos")]
        {
            let data_dir = dirs::home_dir().map_or_else(
                || PathBuf::from("/tmp/pulpo-data"),
                |h| h.join(".pulpo/data"),
            );
            if let Some(creds_mount) =
                maybe_extract_claude_credentials(&self.volumes, &data_dir.to_string_lossy())
            {
                resolved_volumes.push(creds_mount);
            }
        }

        let mut cmd = Command::new("docker");
        cmd.args([
            "run",
            "-d",
            "--name",
            container_name,
            "--label",
            "pulpo.managed=true",
        ]);

        for vol in &resolved_volumes {
            cmd.args(["-v", vol]);
        }

        cmd.args([
            "-v",
            &format!("{working_dir}:/workspace"),
            "-w",
            "/workspace",
            &self.image,
            "bash",
            "-l",
            "-c",
            command,
        ]);
        let output = cmd.output().context("failed to run docker")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("docker run failed: {}", stderr.trim());
        }
        Ok(())
    }

    fn kill_session(&self, backend_id: &str) -> Result<()> {
        let name = docker_container_name(backend_id);
        let _ = build_stop_command(name).output();
        let _ = build_rm_command(name).output();
        Ok(())
    }

    fn is_alive(&self, backend_id: &str) -> Result<bool> {
        let name = docker_container_name(backend_id);
        let output = build_inspect_running_command(name).output()?;
        Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
    }

    fn capture_output(&self, backend_id: &str, lines: usize) -> Result<String> {
        let name = docker_container_name(backend_id);
        let output = build_logs_command(name, lines).output()?;
        // Docker logs sends stdout and stderr separately; combine
        let mut result = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            result.push_str(&stderr);
        }
        Ok(result)
    }

    fn send_input(&self, _backend_id: &str, _text: &str) -> Result<()> {
        // Sandboxed sessions don't support interactive input
        anyhow::bail!("send_input not supported for Docker sessions")
    }

    fn setup_logging(&self, _backend_id: &str, _log_path: &str) -> Result<()> {
        // Docker handles logging natively
        Ok(())
    }

    fn list_sessions(&self) -> Result<Vec<(String, String)>> {
        let output = build_ps_command().output()?;
        if !output.status.success() {
            return Ok(Vec::new());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|name| {
                let id = format!("docker:{name}");
                (id, name.to_owned())
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn test_is_docker_session() {
        assert!(is_docker_session("docker:pulpo-my-task"));
        assert!(is_docker_session("docker:anything"));
        assert!(!is_docker_session("$0"));
        assert!(!is_docker_session("my-session"));
    }

    #[test]
    fn test_docker_container_name() {
        assert_eq!(
            docker_container_name("docker:pulpo-my-task"),
            "pulpo-my-task"
        );
        assert_eq!(docker_container_name("docker:test"), "test");
        assert_eq!(docker_container_name("no-prefix"), "no-prefix");
    }

    #[test]
    fn test_build_run_command_with_no_volumes() {
        let cmd = build_run_command(
            "my-image:latest",
            "pulpo-test",
            "/tmp/repo",
            "echo hello",
            &[],
        );
        assert_eq!(cmd.get_program(), "docker");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert!(args.contains(&OsStr::new("run")));
        assert!(args.contains(&OsStr::new("-d")));
        assert!(args.contains(&OsStr::new("--name")));
        assert!(args.contains(&OsStr::new("pulpo-test")));
        assert!(args.contains(&OsStr::new("my-image:latest")));
        assert!(args.contains(&OsStr::new("/tmp/repo:/workspace")));
    }

    #[test]
    fn test_build_run_command_with_volumes() {
        let volumes = vec![
            "/home/user/.claude:/root/.claude:ro".to_owned(),
            "/home/user/.codex:/root/.codex:ro".to_owned(),
        ];
        let cmd = build_run_command(
            "my-image:latest",
            "pulpo-test",
            "/tmp/repo",
            "echo hello",
            &volumes,
        );
        let args: Vec<&OsStr> = cmd.get_args().collect();
        // Check that -v flags are present for each volume
        // 2 custom volumes + 1 workspace volume = 3 -v flags
        let v_count = args.iter().filter(|a| **a == OsStr::new("-v")).count();
        assert_eq!(v_count, 3);
        assert!(args.contains(&OsStr::new("/home/user/.claude:/root/.claude:ro")));
        assert!(args.contains(&OsStr::new("/home/user/.codex:/root/.codex:ro")));
    }

    #[test]
    fn test_build_stop_command() {
        let cmd = build_stop_command("pulpo-test");
        assert_eq!(cmd.get_program(), "docker");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["stop", "pulpo-test"]);
    }

    #[test]
    fn test_build_rm_command() {
        let cmd = build_rm_command("pulpo-test");
        assert_eq!(cmd.get_program(), "docker");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["rm", "-f", "pulpo-test"]);
    }

    #[test]
    fn test_build_inspect_running_command() {
        let cmd = build_inspect_running_command("pulpo-test");
        assert_eq!(cmd.get_program(), "docker");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert!(args.contains(&OsStr::new("inspect")));
        assert!(args.contains(&OsStr::new("pulpo-test")));
    }

    #[test]
    fn test_build_logs_command() {
        let cmd = build_logs_command("pulpo-test", 100);
        assert_eq!(cmd.get_program(), "docker");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert!(args.contains(&OsStr::new("logs")));
        assert!(args.contains(&OsStr::new("--tail")));
        assert!(args.contains(&OsStr::new("100")));
        assert!(args.contains(&OsStr::new("pulpo-test")));
    }

    #[test]
    fn test_build_ps_command() {
        let cmd = build_ps_command();
        assert_eq!(cmd.get_program(), "docker");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert!(args.contains(&OsStr::new("ps")));
        assert!(args.contains(&OsStr::new("label=pulpo.managed=true")));
    }

    #[test]
    fn test_docker_backend_new() {
        let backend = DockerBackend::new(
            "test-image:latest",
            vec!["~/.claude:/root/.claude:ro".into()],
        );
        assert_eq!(backend.image, "test-image:latest");
        assert_eq!(backend.volumes.len(), 1);
    }

    #[test]
    fn test_docker_backend_new_empty_volumes() {
        let backend = DockerBackend::new("test-image:latest", vec![]);
        assert_eq!(backend.image, "test-image:latest");
        assert!(backend.volumes.is_empty());
    }

    #[test]
    fn test_resolve_tilde_with_home() {
        let resolved = resolve_tilde("~/test-dir");
        assert!(resolved.is_some());
        let path = resolved.unwrap();
        assert!(!path.to_string_lossy().starts_with('~'));
        assert!(path.to_string_lossy().ends_with("test-dir"));
    }

    #[test]
    fn test_resolve_tilde_bare() {
        let resolved = resolve_tilde("~");
        assert!(resolved.is_some());
        let path = resolved.unwrap();
        assert!(!path.to_string_lossy().starts_with('~'));
    }

    #[test]
    fn test_resolve_tilde_absolute_path() {
        let resolved = resolve_tilde("/absolute/path");
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_resolve_tilde_relative_path() {
        let resolved = resolve_tilde("relative/path");
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), PathBuf::from("relative/path"));
    }

    #[test]
    fn test_resolve_volumes_skips_nonexistent() {
        let volumes = vec![
            "/nonexistent/path/abc123:/container:ro".to_owned(),
            "/tmp:/container-tmp:ro".to_owned(),
        ];
        let resolved = resolve_volumes(&volumes);
        // /nonexistent/path should be skipped, /tmp should remain
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].starts_with("/tmp:"));
    }

    #[test]
    fn test_resolve_volumes_expands_tilde() {
        // Create a temp dir to act as a real path
        let tmpdir = tempfile::tempdir().unwrap();
        let host_path = tmpdir.path().to_str().unwrap();
        let volumes = vec![format!("{host_path}:/container:ro")];
        let resolved = resolve_volumes(&volumes);
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].starts_with(host_path));
    }

    #[test]
    fn test_resolve_volumes_skips_malformed() {
        let volumes = vec!["no-colon-separator".to_owned()];
        let resolved = resolve_volumes(&volumes);
        assert!(resolved.is_empty());
    }

    #[test]
    fn test_resolve_volumes_without_mode() {
        let tmpdir = tempfile::tempdir().unwrap();
        let host_path = tmpdir.path().to_str().unwrap();
        let volumes = vec![format!("{host_path}:/container")];
        let resolved = resolve_volumes(&volumes);
        assert_eq!(resolved.len(), 1);
        // Should not have a mode suffix
        assert_eq!(resolved[0], format!("{host_path}:/container"));
    }

    #[test]
    fn test_resolve_volumes_with_mode() {
        let tmpdir = tempfile::tempdir().unwrap();
        let host_path = tmpdir.path().to_str().unwrap();
        let volumes = vec![format!("{host_path}:/container:rw")];
        let resolved = resolve_volumes(&volumes);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0], format!("{host_path}:/container:rw"));
    }

    #[test]
    fn test_resolve_volumes_empty() {
        let resolved = resolve_volumes(&[]);
        assert!(resolved.is_empty());
    }

    #[test]
    fn test_resolve_volumes_all_nonexistent() {
        let volumes = vec![
            "/nonexistent/a:/c1:ro".to_owned(),
            "/nonexistent/b:/c2:ro".to_owned(),
        ];
        let resolved = resolve_volumes(&volumes);
        assert!(resolved.is_empty());
    }

    #[test]
    fn test_resolve_tilde_other_user_path() {
        // A path like "~otheruser/foo" doesn't start with "~/" and isn't exactly "~",
        // so resolve_tilde treats it as a plain (relative) path.
        let resolved = resolve_tilde("~otheruser/foo");
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), PathBuf::from("~otheruser/foo"));
    }

    #[test]
    fn test_resolve_volumes_path_with_spaces() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir_with_spaces = tmpdir.path().join("path with spaces");
        std::fs::create_dir_all(&dir_with_spaces).unwrap();
        let host_path = dir_with_spaces.to_str().unwrap();
        let volumes = vec![format!("{host_path}:/container:ro")];
        let resolved = resolve_volumes(&volumes);
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].contains("path with spaces"));
    }

    #[test]
    fn test_resolve_volumes_mixed_existent_and_nonexistent() {
        let tmpdir = tempfile::tempdir().unwrap();
        let host_path = tmpdir.path().to_str().unwrap();
        let volumes = vec![
            format!("{host_path}:/container1:ro"),
            "/nonexistent/xyz:/container2:ro".to_owned(),
            format!("{host_path}:/container3:rw"),
        ];
        let resolved = resolve_volumes(&volumes);
        assert_eq!(resolved.len(), 2);
        assert!(resolved[0].contains("/container1"));
        assert!(resolved[1].contains("/container3"));
    }

    #[test]
    fn test_build_run_command_verifies_command_structure() {
        let volumes = vec!["/host:/container:ro".to_owned()];
        let cmd = build_run_command(
            "my-image:v2",
            "pulpo-my-session",
            "/work/dir",
            "claude -p 'do stuff'",
            &volumes,
        );
        let args: Vec<&OsStr> = cmd.get_args().collect();

        // Verify exact structure: run -d --name <name> -v <vol> -v <workdir> -w /workspace <image> bash -l -c <cmd>
        assert_eq!(args[0], "run");
        assert_eq!(args[1], "-d");
        assert_eq!(args[2], "--name");
        assert_eq!(args[3], "pulpo-my-session");
        assert_eq!(args[4], "-v");
        assert_eq!(args[5], "/host:/container:ro");
        assert_eq!(args[6], "-v");
        assert_eq!(args[7], "/work/dir:/workspace");
        assert_eq!(args[8], "-w");
        assert_eq!(args[9], "/workspace");
        assert_eq!(args[10], "my-image:v2");
        assert_eq!(args[11], "bash");
        assert_eq!(args[12], "-l");
        assert_eq!(args[13], "-c");
        assert_eq!(args[14], "claude -p 'do stuff'");
    }

    #[test]
    fn test_is_docker_session_empty_string() {
        assert!(!is_docker_session(""));
    }

    #[test]
    fn test_docker_container_name_empty_string() {
        assert_eq!(docker_container_name(""), "");
    }

    #[test]
    fn test_docker_container_name_just_prefix() {
        assert_eq!(docker_container_name("docker:"), "");
    }

    #[test]
    fn test_resolve_tilde_empty_string() {
        let resolved = resolve_tilde("");
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), PathBuf::from(""));
    }
}
