//! `fake-codex` — a fake harness binary imitating the Codex CLI's surface, for the
//! end-to-end scenario suite (`crates/pulpo-e2e`).
//!
//! Pulpo's Codex adapter (`crates/pulpod/src/harness/codex.rs`) redirects `CODEX_HOME`
//! to a per-session isolated directory, seeds it with a copy of `auth.json`, symlinks
//! to the user's real `AGENTS.md`/`skills`/`rules`/`plugins`/`prompts`/`memories`/
//! cached model list/installation id, and a generated `config.toml` wiring `notify`
//! and `[[hooks.<Event>]]` entries to `pulpo hook codex`/`pulpo hook codex-notify`,
//! then rewrites the command to add `--dangerously-bypass-hook-trust`. This binary
//! plays the part of `codex` for the scenario suite: it honors `CODEX_HOME`, reads the
//! same `config.toml` the adapter wrote, fires the same hook commands (Codex-shaped
//! JSON on stdin) and the `notify` program (JSON as the last argv element, not
//! stdin) a real Codex CLI would, and writes a real rollout file under
//! `$CODEX_HOME/sessions/YYYY/MM/DD/` so `pulpo usage`'s Codex reader
//! (`pulpod::usage::codex`) has real data too.
//!
//! Behavior is driven entirely by the comma-separated `FAKE_AGENT_SCENARIO` env var
//! (or a `pulpo-fake-scenario.txt` file in the session's workdir — see [`run_step`]
//! for the full step vocabulary, shared with `fake-claude`/`fake-pi`).

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Datelike, Utc};
use serde_json::{Value, json};

/// Parsed subset of Codex's CLI surface that pulpo's adapter (and this fake) cares
/// about. Every other flag/arg is accepted and ignored.
#[derive(Debug, Default)]
struct Args {
    exec_mode: bool,
    resume_id: Option<String>,
    resume_last: bool,
    model: Option<String>,
    prompt: Option<String>,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1).peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--dangerously-bypass-hook-trust" => {}
            "exec" => args.exec_mode = true,
            "resume" => match iter.peek().map(String::as_str) {
                Some("--last") => {
                    args.resume_last = true;
                    iter.next();
                }
                Some(next) if !next.starts_with('-') => {
                    args.resume_id = iter.next();
                }
                _ => {}
            },
            "-m" | "--model" => args.model = iter.next(),
            _ => {
                if arg.starts_with('-') {
                    if iter.peek().is_some_and(|next| !next.starts_with('-')) {
                        iter.next();
                    }
                } else if args.prompt.is_none() {
                    args.prompt = Some(arg);
                }
            }
        }
    }
    args
}

/// Hook/notify commands read from `$CODEX_HOME/config.toml`, exactly as
/// `harness::codex::build_config_toml` writes them: `notify = [...]` and
/// `[[hooks.<Event>]]` tables (`{ hooks = [{ type = "command", command = "..." }] }`).
struct Config {
    hooks: HashMap<String, Vec<String>>,
    notify: Option<Vec<String>>,
}

impl Config {
    fn load(codex_home: &Path) -> Self {
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        let mut notify = None;
        if let Ok(raw) = std::fs::read_to_string(codex_home.join("config.toml"))
            && let Ok(value) = raw.parse::<toml::Value>()
        {
            if let Some(array) = value.get("notify").and_then(toml::Value::as_array) {
                notify = Some(
                    array
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect(),
                );
            }
            if let Some(hooks_table) = value.get("hooks").and_then(toml::Value::as_table) {
                for (event, entries) in hooks_table {
                    let Some(entries) = entries.as_array() else {
                        continue;
                    };
                    let mut commands = Vec::new();
                    for entry in entries {
                        let Some(inner) = entry.get("hooks").and_then(toml::Value::as_array) else {
                            continue;
                        };
                        for hook in inner {
                            if let Some(command) = hook.get("command").and_then(toml::Value::as_str)
                            {
                                commands.push(command.to_owned());
                            }
                        }
                    }
                    if !commands.is_empty() {
                        hooks.insert(event.clone(), commands);
                    }
                }
            }
        }
        Self { hooks, notify }
    }

    /// Run every hook command wired to `event`, piping `payload` to each one's
    /// stdin — same contract as `fake-claude`'s `Settings::fire`.
    fn fire(&self, event: &str, payload: &Value) {
        let Some(commands) = self.hooks.get(event) else {
            return;
        };
        let body = payload.to_string();
        for command in commands {
            let child = Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn();
            match child {
                Ok(mut child) => {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(body.as_bytes());
                    }
                    let _ = child.wait();
                }
                Err(error) => {
                    eprintln!("[fake-codex] failed to run hook for {event}: {error}");
                }
            }
        }
    }

    /// Run the `notify` program with `payload` appended as the LAST argv element —
    /// Codex's notify mechanism delivers its JSON that way, never on stdin (see
    /// `harness::codex::notify_value`/`pulpo-cli::hook::execute_codex_notify_hook`).
    fn notify(&self, payload: &Value) {
        let Some(argv) = &self.notify else {
            return;
        };
        let Some((program, rest)) = argv.split_first() else {
            return;
        };
        let mut cmd = Command::new(program);
        cmd.args(rest);
        cmd.arg(payload.to_string());
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        match cmd.spawn() {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(error) => {
                eprintln!("[fake-codex] failed to run notify: {error}");
            }
        }
    }
}

fn write_env_dump(cwd: &Path) {
    let mut vars: Vec<(String, String)> = std::env::vars().collect();
    vars.sort();
    let content = vars
        .into_iter()
        .map(|(k, v)| format!("{k}={v}\n"))
        .collect::<String>();
    let _ = std::fs::write(cwd.join("pulpo-fake-env.txt"), content);
}

fn write_argv_dump(cwd: &Path) {
    let argv: Vec<String> = std::env::args().collect();
    let content = serde_json::to_string_pretty(&argv).unwrap_or_default();
    let _ = std::fs::write(cwd.join("pulpo-fake-argv.json"), content);
}

fn write_state(cwd: &Path, session_id: &str, resumed: bool, steps: &[String]) {
    let state = json!({
        "pid": std::process::id(),
        "session_id": session_id,
        "resumed": resumed,
        "source": if resumed { "resume" } else { "startup" },
        "steps": steps,
    });
    let _ = std::fs::write(
        cwd.join("pulpo-fake-state.json"),
        serde_json::to_string_pretty(&state).unwrap_or_default(),
    );
}

/// Real-home entries the Codex adapter symlinks in
/// (`harness::codex::SYMLINKED_HOME_ENTRIES`) — checked here for existence/
/// resolvability so a scenario test can prove the adapter's seeding actually worked,
/// not just that the isolated `CODEX_HOME` directory exists.
const SYMLINKED_HOME_ENTRIES: &[&str] = &[
    "AGENTS.md",
    "skills",
    "rules",
    "plugins",
    "prompts",
    "memories",
    "models_cache.json",
    "installation_id",
];

/// Verify `$CODEX_HOME/auth.json` exists and that every symlinked entry the adapter
/// creates (`harness::codex::symlink_real_home_entries`) actually resolves —
/// written to `<cwd>/pulpo-fake-codex-env.json` for a scenario test to assert on.
fn check_codex_home(codex_home: &Path) -> Value {
    let auth_json_exists = codex_home.join("auth.json").is_file();
    let mut symlinks = serde_json::Map::new();
    for name in SYMLINKED_HOME_ENTRIES {
        let path = codex_home.join(name);
        let present = path.symlink_metadata().is_ok();
        let resolves = path.exists();
        symlinks.insert(
            (*name).to_owned(),
            json!({"present": present, "resolves": resolves}),
        );
    }
    json!({
        "codex_home": codex_home.to_string_lossy(),
        "auth_json_exists": auth_json_exists,
        "symlinks": symlinks,
    })
}

fn resolve_codex_home() -> PathBuf {
    std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|home| Path::new(&home).join(".codex"))
                .unwrap_or_else(|_| PathBuf::from(".codex"))
        })
}

fn rollout_dir(codex_home: &Path, now: DateTime<Utc>) -> PathBuf {
    codex_home
        .join("sessions")
        .join(format!("{:04}", now.year()))
        .join(format!("{:02}", now.month()))
        .join(format!("{:02}", now.day()))
}

/// Write the leading `session_meta` line of a fresh rollout file — mirrors real
/// Codex's shape exactly: the session/thread id lives under `payload.id` (as
/// `pulpod::usage::codex`'s own test fixtures use), never a `session_id` alias — real
/// Codex's rollout header has no such field, so a fake-only alias would let
/// [`find_last_rollout_session_id`] pass against a shape the real reader never
/// produces.
fn write_session_meta(path: &Path, session_id: &str, cwd: &Path, now: DateTime<Utc>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = json!({
        "timestamp": now.to_rfc3339(),
        "type": "session_meta",
        "payload": {
            "id": session_id,
            "timestamp": now.to_rfc3339(),
            "cwd": cwd.to_string_lossy(),
            "originator": "codex_cli_rs",
        }
    });
    let _ = std::fs::write(path, format!("{line}\n"));
}

/// Append a `token_count` event line — the shape
/// `pulpod::usage::codex::parse_token_count` reads. Cumulative totals: each call's
/// values replace (not add to) what the reader last saw, matching real Codex.
fn append_token_count(path: &Path, input: u64, cached: u64, output: u64) {
    let line = json!({
        "timestamp": Utc::now().to_rfc3339(),
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "total_token_usage": {
                    "input_tokens": input,
                    "cached_input_tokens": cached,
                    "output_tokens": output,
                    "total_tokens": input + output,
                }
            },
            "rate_limits": {
                "primary": {"used_percent": 1.0, "window_minutes": 300, "resets_at": 0},
                "secondary": {"used_percent": 0.0, "window_minutes": 10080, "resets_at": 0},
                "plan_type": "plus",
            }
        }
    });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// Find the session id of the most recently written rollout file under
/// `<codex_home>/sessions/` (recursively) — used to resolve `resume --last` (real
/// Codex resolves the same way: by file position, not a stored id pulpo ever learns).
/// Reads the id from `payload.id` — real Codex's actual rollout header field (see
/// [`write_session_meta`]) — not a fake-only `payload.session_id` alias, so this
/// exercises the same lookup shape real Codex's own `resume --last` would.
fn find_last_rollout_session_id(codex_home: &Path) -> Option<String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
            {
                out.push(path);
            }
        }
    }
    let mut candidates = Vec::new();
    walk(&codex_home.join("sessions"), &mut candidates);
    // Filenames embed a sortable timestamp (`rollout-<ts>-<id>.jsonl`), so the
    // lexicographically last one is the most recently created.
    candidates.sort();
    let last = candidates.pop()?;
    let content = std::fs::read_to_string(last).ok()?;
    let first_line = content.lines().next()?;
    let value: Value = serde_json::from_str(first_line).ok()?;
    value
        .get("payload")?
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn block_on_stdin() {
    let stdin = std::io::stdin();
    let mut line = String::new();
    let _ = stdin.lock().read_line(&mut line);
}

fn hang_forever() -> ! {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn base_payload(session_id: &str, cwd: &Path, transcript_path: &Path) -> Value {
    json!({
        "session_id": session_id,
        "cwd": cwd.to_string_lossy(),
        "transcript_path": transcript_path.to_string_lossy(),
    })
}

fn run_step(
    step: &str,
    config: &Config,
    session_id: &str,
    resumed: bool,
    cwd: &Path,
    rollout_path: &Path,
    turn: &mut u64,
) {
    let mut payload = base_payload(session_id, cwd, rollout_path);
    let obj = payload.as_object_mut().expect("payload is an object");

    if let Some(("exit", arg)) = step.split_once(':') {
        let code: i32 = arg.parse().unwrap_or(1);
        // A coded exit models a crash: no SessionEnd hook fires.
        std::process::exit(code);
    }

    match step {
        "start" => {
            obj.insert("hook_event_name".into(), json!("SessionStart"));
            obj.insert(
                "source".into(),
                json!(if resumed { "resume" } else { "startup" }),
            );
            config.fire("SessionStart", &payload);
        }
        "prompt" => {
            obj.insert("hook_event_name".into(), json!("UserPromptSubmit"));
            config.fire("UserPromptSubmit", &payload);
        }
        "needs_input" => {
            obj.insert("hook_event_name".into(), json!("PermissionRequest"));
            obj.insert("tool_name".into(), json!("shell"));
            config.fire("PermissionRequest", &payload);
        }
        "wait" => {
            block_on_stdin();
            let mut working = base_payload(session_id, cwd, rollout_path);
            working
                .as_object_mut()
                .expect("payload is an object")
                .insert("hook_event_name".into(), json!("UserPromptSubmit"));
            config.fire("UserPromptSubmit", &working);

            *turn += 1;
            let mut stop = base_payload(session_id, cwd, rollout_path);
            stop.as_object_mut().expect("payload is an object").extend([
                ("hook_event_name".to_owned(), json!("Stop")),
                (
                    "last_assistant_message".to_owned(),
                    json!("Resolved the permission prompt."),
                ),
            ]);
            config.fire("Stop", &stop);
            append_token_count(rollout_path, 500 * *turn, 100 * *turn, 50 * *turn);
        }
        "stop" => {
            obj.insert("hook_event_name".into(), json!("Stop"));
            obj.insert("last_assistant_message".into(), json!("Turn complete."));
            config.fire("Stop", &payload);

            *turn += 1;
            append_token_count(rollout_path, 500 * *turn, 100 * *turn, 50 * *turn);

            // Codex's real `notify` payload spells these fields kebab-case (see
            // `harness::codex`'s module doc and `parse_notify_event`/
            // `extract_session_id`'s kebab-case tolerance) — sending only
            // `snake_case` here would exercise a branch real Codex never takes.
            let notify_payload = json!({
                "type": "agent-turn-complete",
                "thread-id": session_id,
                "turn-id": format!("turn-{turn}"),
                "cwd": cwd.to_string_lossy(),
                "last-assistant-message": "Turn complete.",
                "input-messages": [],
            });
            config.notify(&notify_payload);
        }
        "exit" => {
            obj.insert("hook_event_name".into(), json!("SessionEnd"));
            obj.insert("reason".into(), json!("other"));
            config.fire("SessionEnd", &payload);
            std::process::exit(0);
        }
        "hang" => hang_forever(),
        other => {
            eprintln!("[fake-codex] unknown scenario step, ignoring: {other}");
        }
    }
}

fn main() {
    let args = parse_args();
    let _ = args.exec_mode; // accepted, does not change fake behavior beyond argv recording
    let _ = args.model;
    let _ = args.prompt;

    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    write_env_dump(&cwd);
    write_argv_dump(&cwd);

    let codex_home = resolve_codex_home();
    let env_checks = check_codex_home(&codex_home);
    let _ = std::fs::write(
        cwd.join("pulpo-fake-codex-env.json"),
        serde_json::to_string_pretty(&env_checks).unwrap_or_default(),
    );

    let config = Config::load(&codex_home);

    let resumed = args.resume_id.is_some() || args.resume_last;
    let session_id = if let Some(id) = args.resume_id.clone() {
        id
    } else if args.resume_last {
        // Real Codex errors out (non-zero exit) when asked to resume the most
        // recent thread in a `CODEX_HOME` that has no rollout at all — it has
        // nothing to resolve `--last` against. Minting a fresh id here instead
        // would silently paper over that case for a scenario test.
        find_last_rollout_session_id(&codex_home).unwrap_or_else(|| {
            eprintln!(
                "[fake-codex] Error: no conversation to resume (resume --last with an empty CODEX_HOME)"
            );
            std::process::exit(1);
        })
    } else {
        uuid::Uuid::new_v4().to_string()
    };

    // Real Codex writes a fresh rollout file every process invocation, resume
    // included (usage/codex.rs: "restarts produce new files") — independent of
    // whether any hook ever fires, so `pulpo usage` has real data even when a
    // scenario never fires SessionStart.
    let now = Utc::now();
    let rollout_path = rollout_dir(&codex_home, now).join(format!(
        "rollout-{}-{session_id}.jsonl",
        now.format("%Y-%m-%dT%H-%M-%S")
    ));
    write_session_meta(&rollout_path, &session_id, &cwd, now);

    let scenario = std::fs::read_to_string(cwd.join("pulpo-fake-scenario.txt"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("FAKE_AGENT_SCENARIO").ok())
        .unwrap_or_else(|| "start,prompt,stop,exit".to_owned());
    let steps: Vec<String> = scenario
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();

    let mut completed = Vec::new();
    let mut turn = 0u64;
    for step in &steps {
        run_step(
            step,
            &config,
            &session_id,
            resumed,
            &cwd,
            &rollout_path,
            &mut turn,
        );
        completed.push(step.clone());
        write_state(&cwd, &session_id, resumed, &completed);
    }

    hang_forever();
}
