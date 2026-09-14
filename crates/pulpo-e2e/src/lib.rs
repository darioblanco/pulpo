//! Scenario test harness for pulpo's end-to-end suite.
//!
//! Boots a real `pulpod` (built from the workspace, not mocked), pointed at an
//! isolated `HOME`/data dir/config/tmux server, and drives it exactly the way a user
//! would: through the real `pulpo` CLI and the daemon's own HTTP API. No component
//! under test is faked — only the *agent* is (`fake-claude`, `src/bin/fake-claude.rs`)
//! so a scenario can deterministically script Claude Code's lifecycle hooks without
//! needing a live API key.
//!
//! See `tests/scenarios.rs` for the scenarios themselves, and
//! `docs/architecture/harness-adapters.md` / `docs/operations/session-lifecycle.md`
//! for the behavior they exercise. `CLAUDE.md`'s testing section documents the
//! strategy this crate implements and how to add a new scenario.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use pulpo_common::session::{InterventionCode, Session, SessionStatus};

/// Locate the workspace's cargo target directory: `$CARGO_TARGET_DIR` if set (the
/// shared build cache this project's tooling and CI use), else `<repo_root>/target`.
///
/// `pulpo-e2e` is a separate workspace member from `pulpod`/`pulpo-cli`, so
/// `env!("CARGO_BIN_EXE_...")` — which only resolves binaries built for the *same*
/// package as the running test binary — can't find `pulpod`/`pulpo`/`fake-claude`.
/// This walks up from `CARGO_MANIFEST_DIR` instead. Binaries must already be built
/// (`make e2e` does this before running the suite; a bare `cargo test -p pulpo-e2e`
/// does not).
#[must_use]
pub fn target_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(dir);
    }
    // crates/pulpo-e2e/Cargo.toml -> crates/ -> <repo root> -> target/
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("pulpo-e2e should live at <repo root>/crates/pulpo-e2e")
        .join("target")
}

fn debug_bin(name: &str) -> PathBuf {
    let path = target_dir().join("debug").join(name);
    assert!(
        path.is_file(),
        "{path:?} not found — build pulpod/pulpo/fake-claude first \
         (`make e2e` does this; a bare `cargo test -p pulpo-e2e` does not)"
    );
    path
}

/// Reserve a free TCP port by binding to `127.0.0.1:0` and reading back the
/// OS-assigned port, then dropping the listener. A small window exists where
/// something else could grab the port before `pulpod` binds it, but this is
/// standard practice for test harnesses and each scenario gets its own daemon, so
/// collisions between scenarios in this suite are not a real risk.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
        .local_addr()
        .expect("read local addr")
        .port()
}

/// Resolve a path to its canonical (symlink-free, absolute) form, falling back to
/// the original path if canonicalization fails (e.g. it doesn't exist yet).
///
/// Scenarios that check a fake harness's own `cwd` against the `--workdir` they
/// passed at spawn time (budget/usage scenarios especially — see
/// `pulpod::usage::claude::sanitize_workdir`) need the *exact* string both sides
/// agree on. `tempfile::tempdir()` can return a path through a symlink (`/tmp` ->
/// `/private/tmp` on macOS); canonicalizing once up front, before handing the path
/// to `--workdir`, avoids a mismatch between the literal string pulpod stores and
/// the real path `std::env::current_dir()` reports once tmux `cd`s into it.
#[must_use]
pub fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Create a fresh temp dir and return its canonical path alongside the guard that
/// keeps it alive. Convenience for the common "give me a real workdir" case.
#[must_use]
pub fn temp_workdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp workdir");
    let path = canonical(dir.path());
    (dir, path)
}

/// Initialize a throwaway git repo at `path` with one commit — for worktree
/// scenarios. Mirrors `session::manager::real_tmux_tests::init_repo`.
pub fn init_git_repo(path: &Path) {
    std::fs::create_dir_all(path).expect("create repo dir");
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(path)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed in {path:?}");
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "e2e@pulpo.test"]);
    run(&["config", "user.name", "pulpo-e2e"]);
    std::fs::write(path.join("README.md"), "seed\n").expect("write seed file");
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);
}

/// Read `<workdir>/pulpo-fake-env.txt` (written by `fake-claude` at startup) into a
/// map. Empty map when the file doesn't exist yet.
#[must_use]
pub fn read_fake_env(workdir: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(workdir.join("pulpo-fake-env.txt"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// Read `<workdir>/pulpo-fake-argv.json` (written by `fake-claude` at startup, right
/// alongside the env dump) as the fake harness's own `std::env::args()` — `argv[0]`
/// included. `None` when the file doesn't exist yet (the process hasn't started, or
/// hasn't gotten past its own startup code).
///
/// Proves quoting survived every shell hop between `pulpo spawn/handoff/schedule add
/// -- claude ...` and the harness process actually exec'd: the CLI's
/// `shell_words::join`, the harness adapter's `shell_words::split`/rewrite/
/// `shell_words::join`, and `pulpod`'s own `wrap_command` embedding — see S12.
#[must_use]
pub fn read_fake_argv(workdir: &Path) -> Option<Vec<String>> {
    let content = std::fs::read_to_string(workdir.join("pulpo-fake-argv.json")).ok()?;
    serde_json::from_str(&content).ok()
}

/// Poll `read_fake_argv` until it appears, or panic after `timeout`.
pub fn wait_for_fake_argv(workdir: &Path, timeout: Duration) -> Vec<String> {
    let start = Instant::now();
    loop {
        if let Some(argv) = read_fake_argv(workdir) {
            return argv;
        }
        if start.elapsed() > timeout {
            panic!("timed out after {timeout:?} waiting for pulpo-fake-argv.json at {workdir:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Read `<workdir>/pulpo-fake-state.json` (written by `fake-claude` after every
/// scenario step) as a JSON value. `None` when the file doesn't exist yet.
#[must_use]
pub fn read_fake_state(workdir: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(workdir.join("pulpo-fake-state.json")).ok()?;
    serde_json::from_str(&content).ok()
}

/// Read `<workdir>/pulpo-fake-codex-env.json` (written by `fake-codex` at startup) —
/// its own check of whether `auth.json` and the symlinked real-home entries the
/// Codex adapter seeds into the isolated `CODEX_HOME` (`AGENTS.md`, `skills`, ...)
/// actually exist/resolve. `None` when the file doesn't exist yet.
#[must_use]
pub fn read_fake_codex_env(workdir: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(workdir.join("pulpo-fake-codex-env.json")).ok()?;
    serde_json::from_str(&content).ok()
}

/// Poll `read_fake_codex_env` until it appears, or panic after `timeout`.
pub fn wait_for_fake_codex_env(workdir: &Path, timeout: Duration) -> serde_json::Value {
    let start = Instant::now();
    loop {
        if let Some(value) = read_fake_codex_env(workdir) {
            return value;
        }
        if start.elapsed() > timeout {
            panic!(
                "timed out after {timeout:?} waiting for pulpo-fake-codex-env.json at {workdir:?}"
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Poll `read_fake_state` until `pred` accepts it, or panic after `timeout`.
pub fn wait_for_fake_state(
    workdir: &Path,
    timeout: Duration,
    mut pred: impl FnMut(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let start = Instant::now();
    let mut last = None;
    loop {
        if let Some(state) = read_fake_state(workdir) {
            if pred(&state) {
                return state;
            }
            last = Some(state);
        }
        if start.elapsed() > timeout {
            panic!(
                "timed out after {timeout:?} waiting for fake-claude state at {workdir:?}; \
                 last observed: {last:#?}"
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Configuration knobs a scenario can override before starting the daemon.
///
/// Defaults match `pulpod::config`'s own production defaults except for the two
/// tick intervals (`check_interval_secs`, `scheduler_tick_secs`), which default
/// much lower here — production defaults (10s / 60s) would make every scenario
/// wait that long for the watchdog/scheduler to even look.
pub struct DaemonConfig {
    pub idle_threshold_secs: u64,
    pub idle_timeout_secs: u64,
    pub idle_action: &'static str,
    pub check_interval_secs: u64,
    pub scheduler_tick_secs: u64,
    pub webhook_url: Option<String>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            idle_threshold_secs: 60,
            idle_timeout_secs: 600,
            idle_action: "alert",
            check_interval_secs: 1,
            scheduler_tick_secs: 2,
            webhook_url: None,
        }
    }
}

/// A running, isolated `pulpod` + private tmux server, driven through the real
/// `pulpo` CLI and HTTP API. Dropping this kills the daemon and the tmux server and
/// removes every temp file it created.
pub struct Daemon {
    child: Option<Child>,
    pub url: String,
    pub base_url: String,
    pub port: u16,
    pub data_dir: PathBuf,
    home_dir: PathBuf,
    tmux_tmp: PathBuf,
    fakebin_dir: PathBuf,
    path_env: std::ffi::OsString,
    config_path: PathBuf,
    log_path: PathBuf,
    pulpod_bin: PathBuf,
    pulpo_bin: PathBuf,
    client: reqwest::blocking::Client,
    // Kept alive only so its `Drop` removes the whole tree on teardown — every
    // path above lives under this directory.
    _root: tempfile::TempDir,
}

impl Daemon {
    /// Start a new isolated daemon. Panics (with the daemon's own log tailed in)
    /// if it never becomes healthy.
    #[must_use]
    pub fn start(cfg: DaemonConfig) -> Self {
        Self::start_with_seed(cfg, |_data_dir| {})
    }

    /// Like [`Daemon::start`], but calls `seed_data_dir` with the (empty,
    /// already-created) data dir path *before* `pulpod` is spawned — for
    /// scenarios that need a pre-existing `state.db` in place when the
    /// daemon first boots (e.g. S13's unusable/downgraded database).
    #[must_use]
    pub fn start_with_seed(cfg: DaemonConfig, seed_data_dir: impl FnOnce(&Path)) -> Self {
        let root = tempfile::tempdir().expect("create root tempdir");
        let home_dir = root.path().join("home");
        let data_dir = root.path().join("data");
        let tmux_tmp = root.path().join("tmux-tmp");
        let fakebin_dir = root.path().join("fakebin");
        for dir in [&home_dir, &data_dir, &tmux_tmp, &fakebin_dir] {
            std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("create {dir:?}: {e}"));
        }
        std::fs::create_dir_all(home_dir.join(".claude").join("projects")).ok();
        seed_data_dir(&data_dir);

        let pulpod_bin = debug_bin("pulpod");
        let pulpo_bin = debug_bin("pulpo");
        // Stage every fake harness binary under `fakebin_dir`, named exactly like the
        // real CLI it imitates — `HarnessRegistry` resolves a command by the basename
        // of argv[0], so a scenario using `daemon.fake_bin("codex")`'s path (not the
        // bare word) reaches the Codex adapter the same way spawning real `codex`
        // would.
        for (bin_name, harness_name) in [
            ("fake-claude", "claude"),
            ("fake-codex", "codex"),
            ("fake-pi", "pi"),
        ] {
            let fake_bin = debug_bin(bin_name);
            let link = fakebin_dir.join(harness_name);
            std::fs::copy(&fake_bin, &link)
                .unwrap_or_else(|e| panic!("stage {bin_name} binary: {e}"));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&link)
                    .unwrap_or_else(|e| panic!("stat staged {bin_name} binary: {e}"))
                    .permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&link, perms)
                    .unwrap_or_else(|e| panic!("chmod {bin_name} binary: {e}"));
            }
        }

        let port = free_port();
        let token = format!("e2e-test-token-{}", uuid::Uuid::new_v4());
        let config_path = root.path().join("config.toml");
        let webhooks_toml = cfg.webhook_url.as_deref().map_or_else(String::new, |url| {
            format!("\n[[webhooks]]\nname = \"e2e\"\nurl = \"{url}\"\nevents = []\n")
        });
        let config = format!(
            "[node]\n\
             port = {port}\n\
             data_dir = \"{data_dir}\"\n\
             bind = \"local\"\n\
             \n\
             [auth]\n\
             token = \"{token}\"\n\
             \n\
             [watchdog]\n\
             check_interval_secs = {check_interval_secs}\n\
             idle_threshold_secs = {idle_threshold_secs}\n\
             idle_timeout_secs = {idle_timeout_secs}\n\
             idle_action = \"{idle_action}\"\n\
             \n\
             [scheduler]\n\
             tick_secs = {scheduler_tick_secs}\n\
             {webhooks_toml}",
            data_dir = data_dir.display(),
            check_interval_secs = cfg.check_interval_secs,
            idle_threshold_secs = cfg.idle_threshold_secs,
            idle_timeout_secs = cfg.idle_timeout_secs,
            idle_action = cfg.idle_action,
            scheduler_tick_secs = cfg.scheduler_tick_secs,
        );
        std::fs::write(&config_path, &config).unwrap_or_else(|e| panic!("write config.toml: {e}"));

        // Belt-and-suspenders per the harness-adapters doc: the spawn commands this
        // suite uses always give `claude`'s *absolute* path (so PATH resolution
        // inside the wrapped session — and `TmuxBackend`'s own login-shell PATH
        // probe re-sourcing rc files — can never hide it), but staging it on PATH
        // too costs nothing and covers any code path that resolves it by bare name.
        let path_env = {
            let mut path = std::ffi::OsString::from(&fakebin_dir);
            path.push(":");
            path.push(std::env::var_os("PATH").unwrap_or_default());
            path
        };

        let log_path = root.path().join("pulpod.log");
        let log_file =
            std::fs::File::create(&log_path).unwrap_or_else(|e| panic!("create pulpod.log: {e}"));
        let child = Command::new(&pulpod_bin)
            .arg("--config")
            .arg(&config_path)
            .env("HOME", &home_dir)
            .env("PATH", &path_env)
            .env("TMUX_TMPDIR", &tmux_tmp)
            .env_remove("PULPO_URL")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file.try_clone().expect("clone log handle")))
            .stderr(Stdio::from(log_file))
            .spawn()
            .unwrap_or_else(|e| panic!("spawn pulpod: {e}"));

        let base_url = format!("http://127.0.0.1:{port}");
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("build http client");

        let daemon = Self {
            child: Some(child),
            url: format!("localhost:{port}"),
            base_url,
            port,
            data_dir,
            home_dir,
            tmux_tmp,
            fakebin_dir,
            path_env,
            config_path,
            log_path,
            pulpod_bin,
            pulpo_bin,
            client,
            _root: root,
        };
        daemon.wait_healthy(Duration::from_secs(20));
        daemon
    }

    /// Absolute path to a staged fake harness binary, named `claude`/`codex`/`pi` so
    /// the daemon's harness registry matches it by basename — pass this (not the
    /// bare harness name) as the spawned command's argv0 in every scenario.
    #[must_use]
    pub fn fake_bin(&self, harness: &str) -> PathBuf {
        self.fakebin_dir.join(harness)
    }

    /// Absolute path to the staged fake Claude Code binary (`fake_bin("claude")`).
    #[must_use]
    pub fn fake_claude_bin(&self) -> PathBuf {
        self.fake_bin("claude")
    }

    /// Absolute path to the staged fake Codex binary (`fake_bin("codex")`).
    #[must_use]
    pub fn fake_codex_bin(&self) -> PathBuf {
        self.fake_bin("codex")
    }

    /// Absolute path to the staged fake pi binary (`fake_bin("pi")`).
    #[must_use]
    pub fn fake_pi_bin(&self) -> PathBuf {
        self.fake_bin("pi")
    }

    /// The daemon's own stdout/stderr log so far — useful for debugging a scenario
    /// failure (a `tracing::warn!` the daemon logged but never surfaced through the
    /// HTTP API/session state).
    #[must_use]
    pub fn daemon_log(&self) -> String {
        std::fs::read_to_string(&self.log_path).unwrap_or_default()
    }

    fn wait_healthy(&self, timeout: Duration) {
        let start = Instant::now();
        loop {
            if let Ok(resp) = self
                .client
                .get(format!("{}/api/v1/health", self.base_url))
                .send()
                && resp.status().is_success()
            {
                return;
            }
            if start.elapsed() > timeout {
                let log = std::fs::read_to_string(&self.log_path).unwrap_or_default();
                panic!(
                    "pulpod did not become healthy within {timeout:?}\n--- pulpod.log ---\n{log}"
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Run the `pulpo` CLI with `--url` pre-filled, waiting up to 20s for it to
    /// exit. Some subcommands (`resume`, `attach`) try to attach a real terminal
    /// afterwards, which fails fast (no controlling TTY in a test process) rather
    /// than hanging — the timeout is a safety net, not the expected path.
    pub fn pulpo(&self, args: &[&str]) -> std::process::Output {
        let mut cmd = Command::new(&self.pulpo_bin);
        cmd.arg("--url").arg(&self.url);
        cmd.args(args);
        cmd.env("HOME", &self.home_dir);
        cmd.env("PATH", &self.path_env);
        cmd.stdin(Stdio::null());
        run_with_timeout(cmd, Duration::from_secs(20))
    }

    /// Spawn a detached session: `pulpo spawn <name> -d --workdir <workdir> <extra>
    /// -- <command>`. `extra` carries any additional spawn flags
    /// (`--worktree`, `--budget-cost`, `--idle-threshold`, ...).
    pub fn spawn(
        &self,
        name: &str,
        workdir: &Path,
        extra: &[&str],
        command: &[&str],
    ) -> std::process::Output {
        let workdir_str = workdir.to_string_lossy().into_owned();
        let mut args: Vec<&str> = vec!["spawn", name, "-d", "--workdir", &workdir_str];
        args.extend_from_slice(extra);
        args.push("--");
        args.extend_from_slice(command);
        self.pulpo(&args)
    }

    /// Fetch one session by name or id. `None` on a 404 (or any transport error).
    #[must_use]
    pub fn session(&self, name: &str) -> Option<Session> {
        let resp = self
            .client
            .get(format!("{}/api/v1/sessions/{name}", self.base_url))
            .send()
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json::<Session>().ok()
    }

    /// List every session (live and terminal) known to the daemon.
    #[must_use]
    pub fn sessions_all(&self) -> Vec<Session> {
        self.client
            .get(format!("{}/api/v1/sessions", self.base_url))
            .send()
            .ok()
            .and_then(|r| r.json().ok())
            .unwrap_or_default()
    }

    /// Poll `session(name)` until its status matches, or panic after `timeout`.
    pub fn wait_status(&self, name: &str, status: SessionStatus, timeout: Duration) -> Session {
        self.wait_for(name, timeout, |s| s.status == status)
    }

    /// Poll `session(name)` until `pred` accepts it, or panic after `timeout` with
    /// the last observed session for debuggability.
    pub fn wait_for(
        &self,
        name: &str,
        timeout: Duration,
        mut pred: impl FnMut(&Session) -> bool,
    ) -> Session {
        let start = Instant::now();
        let mut last: Option<Session> = None;
        loop {
            if let Some(session) = self.session(name) {
                if pred(&session) {
                    return session;
                }
                last = Some(session);
            }
            if start.elapsed() > timeout {
                panic!(
                    "timed out after {timeout:?} waiting for session {name:?}; \
                     last observed: {last:#?}"
                );
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    /// `pulpo stop <name> [--purge]`. Asserts the CLI itself reported success —
    /// unlike `resume`/`attach`, nothing about `stop` can fail headlessly.
    pub fn stop(&self, name: &str, purge: bool) {
        let mut args = vec!["stop", name];
        if purge {
            args.push("--purge");
        }
        let output = self.pulpo(&args);
        assert!(
            output.status.success(),
            "pulpo stop {name} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// `pulpo rm <name>` — purge a single session outright (row, intervention
    /// events, exit markers, session log, harness dir). Unlike `stop`, this does
    /// not assert success — callers exercising the 409 (active/idle) path need the
    /// raw output.
    pub fn remove(&self, name: &str) -> std::process::Output {
        self.pulpo(&["rm", name])
    }

    /// `pulpo resume <name>`. The CLI always tries to auto-attach afterwards, which
    /// fails fast with no controlling terminal — the resume itself already
    /// happened server-side by the time that attach attempt runs, so the process's
    /// own exit status is deliberately not asserted on here; check the outcome via
    /// `session`/`wait_status` instead.
    pub fn resume(&self, name: &str) -> std::process::Output {
        self.pulpo(&["resume", name])
    }

    /// `pulpo input <name> [text]` — omit `text` to send a bare Enter.
    pub fn input(&self, name: &str, text: Option<&str>) {
        let mut args = vec!["input", name];
        if let Some(t) = text {
            args.push(t);
        }
        let output = self.pulpo(&args);
        assert!(
            output.status.success(),
            "pulpo input {name} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// `pulpo cleanup`.
    pub fn cleanup(&self) -> std::process::Output {
        self.pulpo(&["cleanup"])
    }

    /// `pulpo schedule add <name> <cron> --workdir <workdir> -- <command>`.
    pub fn schedule_add(
        &self,
        name: &str,
        cron: &str,
        workdir: &Path,
        command: &[&str],
    ) -> std::process::Output {
        let workdir_str = workdir.to_string_lossy().into_owned();
        let mut args: Vec<&str> = vec!["schedule", "add", name, cron, "--workdir", &workdir_str];
        args.push("--");
        args.extend_from_slice(command);
        self.pulpo(&args)
    }

    /// Kill this daemon's *private* tmux server (scoped by `TMUX_TMPDIR`, never the
    /// developer's real one) — simulates a crash/reboot wiping every tmux session
    /// out from under the daemon.
    pub fn kill_tmux_server(&self) {
        let _ = Command::new("tmux")
            .env("TMUX_TMPDIR", &self.tmux_tmp)
            .args(["kill-server"])
            .output();
    }

    /// Whether a tmux session literally named `name` exists on this daemon's
    /// private server. `backend_session_id` gets upgraded from a name to tmux's
    /// own `$N` id shortly after creation (see `CLAUDE.md`'s "Session IDs" note),
    /// so this — not the session's `backend_session_id` field — is the reliable
    /// way to check what a newly (re)created tmux session was actually named:
    /// tmux accepts either its name or its `$N` id as a `-t` target for the same
    /// session, and the name never changes after creation regardless of which
    /// alias pulpod later stores as canonical.
    #[must_use]
    pub fn tmux_has_session(&self, name: &str) -> bool {
        Command::new("tmux")
            .env("TMUX_TMPDIR", &self.tmux_tmp)
            .args(["has-session", "-t", name])
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// Kill and respawn `pulpod` against the same config/data dir/tmux server —
    /// simulates a daemon restart (upgrade, crash-restart, `systemctl restart`)
    /// with sessions still on disk and (unless the caller also calls
    /// `kill_tmux_server` first) still running in tmux.
    pub fn restart_daemon(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Give the OS a moment to release the port before rebinding it.
        std::thread::sleep(Duration::from_millis(300));
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .unwrap_or_else(|e| panic!("reopen pulpod.log: {e}"));
        let child = Command::new(&self.pulpod_bin)
            .arg("--config")
            .arg(&self.config_path)
            .env("HOME", &self.home_dir)
            .env("PATH", &self.path_env)
            .env("TMUX_TMPDIR", &self.tmux_tmp)
            .env_remove("PULPO_URL")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file.try_clone().expect("clone log handle")))
            .stderr(Stdio::from(log_file))
            .spawn()
            .unwrap_or_else(|e| panic!("respawn pulpod: {e}"));
        self.child = Some(child);
        self.wait_healthy(Duration::from_secs(20));
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.kill_tmux_server();
    }
}

/// Run `cmd`, waiting up to `timeout` for it to exit before killing it. Used for
/// every `pulpo` CLI invocation — `resume`/`attach` try to attach a real terminal,
/// which should fail fast with no controlling TTY, but a timeout is a safety net
/// against any code path that instead blocks.
fn run_with_timeout(mut cmd: Command, timeout: Duration) -> std::process::Output {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("spawn pulpo CLI: {e}"));
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("poll pulpo CLI child") {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(mut s) = child.stdout.take() {
                let _ = s.read_to_end(&mut stdout);
            }
            if let Some(mut s) = child.stderr.take() {
                let _ = s.read_to_end(&mut stderr);
            }
            return std::process::Output {
                status,
                stdout,
                stderr,
            };
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            panic!("pulpo CLI invocation timed out after {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A minimal local HTTP receiver for webhook-delivery scenarios: accepts every
/// connection, reads exactly one request's body (by `Content-Length`), stores it,
/// and replies `200 OK`. No routing, no keep-alive — just enough to prove the
/// daemon's webhook sender made a real POST with a real JSON body, which is all
/// `S6` needs.
pub struct WebhookSink {
    addr: std::net::SocketAddr,
    received: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl WebhookSink {
    /// Bind to an ephemeral local port and start accepting connections in the
    /// background. The accept loop runs for the life of the process (daemon
    /// thread) — fine for a short-lived test binary.
    #[must_use]
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind webhook listener");
        let addr = listener.local_addr().expect("read local addr");
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_bg = received.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                if let Some(body) = read_http_request_body(&mut stream)
                    && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body)
                {
                    received_bg
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(value);
                }
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        Self { addr, received }
    }

    /// The URL to configure as a `[[webhooks]] url = "..."` endpoint.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}/webhook", self.addr)
    }

    /// Poll for the first delivered event, panicking after `timeout` with nothing
    /// received.
    #[must_use]
    pub fn wait_for_event(&self, timeout: Duration) -> serde_json::Value {
        let start = Instant::now();
        loop {
            if let Some(value) = self
                .received
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .first()
                .cloned()
            {
                return value;
            }
            if start.elapsed() > timeout {
                panic!("timed out after {timeout:?} waiting for a webhook delivery");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn read_http_request_body(stream: &mut std::net::TcpStream) -> Option<Vec<u8>> {
    use std::io::BufRead;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut reader = std::io::BufReader::new(stream.try_clone().ok()?);
    let mut content_length = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).ok()?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).ok()?;
    Some(body)
}
