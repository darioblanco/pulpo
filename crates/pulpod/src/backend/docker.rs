use std::process::Command;

#[cfg(not(coverage))]
use anyhow::{Context, Result, bail};

/// Docker container backend for Docker runtime sessions.
#[allow(dead_code)]
pub struct DockerBackend {
    /// Docker image to use for container sessions.
    image: String,
}

#[allow(dead_code)]
impl DockerBackend {
    pub fn new(image: &str) -> Self {
        Self {
            image: image.to_owned(),
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

#[allow(dead_code)]
fn build_run_command(
    image: &str,
    container_name: &str,
    working_dir: &str,
    command: &str,
) -> Command {
    let mut cmd = Command::new("docker");
    cmd.args([
        "run",
        "-d",
        "--name",
        container_name,
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

        let mut cmd = Command::new("docker");
        cmd.args([
            "run",
            "-d",
            "--name",
            container_name,
            "--label",
            "pulpo.managed=true",
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
    fn test_build_run_command() {
        let cmd = build_run_command("my-image:latest", "pulpo-test", "/tmp/repo", "echo hello");
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
        let backend = DockerBackend::new("test-image:latest");
        assert_eq!(backend.image, "test-image:latest");
    }
}
