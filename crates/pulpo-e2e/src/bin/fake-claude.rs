//! `fake-claude` — a fake harness binary imitating Claude Code's CLI surface, for the
//! end-to-end scenario suite (`crates/pulpo-e2e`).
//!
//! Pulpo's Claude adapter (`crates/pulpod/src/harness/claude.rs`) rewrites a spawned
//! `claude ...` command to `claude --session-id <uuid> --settings <path> ...` (or, on
//! resume, `claude --settings <path> --resume <uuid> ...`), where `<path>` is a
//! generated `--settings` JSON file wiring every lifecycle hook of interest to
//! `<pulpo-bin> hook claude`. This binary plays the part of `claude` for the scenario
//! suite: it reads the flags pulpod's adapter cares about, reads the same hook
//! commands out of the same settings file, and runs them exactly the way Claude Code
//! does — spawned through a shell, with the event JSON piped to the hook's stdin.
//!
//! Behavior is driven entirely by the comma-separated `FAKE_AGENT_SCENARIO` env var
//! (default `"start,prompt,stop,exit"`) — see [`run_step`] for the full step
//! vocabulary. This keeps the binary a dumb, deterministic script interpreter rather
//! than anything resembling a real agent, which is exactly what a scenario test wants:
//! the daemon's *reaction* to a lifecycle event is what's under test, not an agent.
//!
//! Designed so a `fake-codex`/`fake-pi` sibling is one more `src/bin/*.rs` file later:
//! everything below the flag-parsing block (`Settings`, `run_step`, `run_hook`,
//! `write_env_dump`, `append_transcript_spend`) only cares about the *harness id*
//! ("claude") and payload shapes, not anything Claude-specific in its control flow.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// Parsed subset of Claude Code's CLI surface that pulpo's adapter (and this fake)
/// cares about. Every other flag/arg is accepted and ignored, matching the docs'
/// "ignores unknown flags" contract.
#[derive(Debug, Default)]
struct Args {
    session_id: Option<String>,
    resume_id: Option<String>,
    settings_path: Option<String>,
    prompt: Option<String>,
    model: Option<String>,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1).peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--session-id" => args.session_id = iter.next(),
            "--settings" => args.settings_path = iter.next(),
            "--resume" => args.resume_id = iter.next(),
            "-p" | "--print" => args.prompt = iter.next(),
            "--model" => args.model = iter.next(),
            _ => {
                // Unknown flag: if it looks like `--flag value` (next token doesn't
                // itself start with `-`), swallow the value too so it isn't
                // mistaken for a later recognized flag's value. A bare unknown
                // token (or a flag with no value, e.g. a boolean switch) is simply
                // dropped.
                if arg.starts_with('-') && iter.peek().is_some_and(|next| !next.starts_with('-')) {
                    iter.next();
                }
            }
        }
    }
    args
}

/// Event name -> shell command strings, read from the `--settings` JSON file the
/// adapter generates (`build_settings_json` in `harness/claude.rs`):
/// `{"hooks": {"<Event>": [{"hooks": [{"type": "command", "command": "..."}], ...}]}}`.
/// A missing/unparsable settings file yields an empty map — hooks simply don't fire,
/// same as a real Claude Code run with no `--settings` at all.
struct Settings {
    hooks: HashMap<String, Vec<String>>,
}

impl Settings {
    fn load(path: Option<&str>) -> Self {
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        if let Some(path) = path
            && let Ok(raw) = std::fs::read_to_string(path)
            && let Ok(value) = serde_json::from_str::<Value>(&raw)
            && let Some(events) = value.get("hooks").and_then(Value::as_object)
        {
            for (event, entries) in events {
                let Some(entries) = entries.as_array() else {
                    continue;
                };
                let mut commands = Vec::new();
                for entry in entries {
                    let Some(inner_hooks) = entry.get("hooks").and_then(Value::as_array) else {
                        continue;
                    };
                    for hook in inner_hooks {
                        if let Some(command) = hook.get("command").and_then(Value::as_str) {
                            commands.push(command.to_owned());
                        }
                    }
                }
                if !commands.is_empty() {
                    hooks.insert(event.clone(), commands);
                }
            }
        }
        Self { hooks }
    }

    /// Run every hook command wired to `event`, piping `payload` to each one's
    /// stdin — exactly how Claude Code invokes a `"type": "command"` hook. Best
    /// effort: a hook that fails to spawn or exits non-zero is logged to stderr and
    /// otherwise ignored, matching the real contract ("a hook must never block or
    /// break the agent it's wired into").
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
                    eprintln!("[fake-claude] failed to run hook for {event}: {error}");
                }
            }
        }
    }
}

/// Dump the process environment to `<cwd>/pulpo-fake-env.txt` — one `KEY=VALUE` line
/// per var, sorted for a stable diff. Lets a scenario test assert on exactly what the
/// session wrapper exported (`PULPO_SESSION_ID`, `PULPO_SESSION_NAME`, `PULPO_URL`)
/// without needing to inspect tmux itself.
fn write_env_dump(cwd: &std::path::Path) {
    let mut vars: Vec<(String, String)> = std::env::vars().collect();
    vars.sort();
    let content = vars
        .into_iter()
        .map(|(k, v)| format!("{k}={v}\n"))
        .collect::<String>();
    let _ = std::fs::write(cwd.join("pulpo-fake-env.txt"), content);
}

/// Overwrite `<cwd>/pulpo-fake-state.json` with the current run's identity and
/// progress — `pid` changes on every resume (a fresh process), `source` says whether
/// this run believes it was a fresh start or a resume, and `steps` accumulates the
/// step names processed so far. A scenario test reads this to confirm a resume
/// actually spawned a *new* fake-claude process (not just that pulpod's own view of
/// the session looks right).
fn write_state(cwd: &std::path::Path, session_id: &str, resumed: bool, steps: &[String]) {
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

/// Sanitize a working directory into Claude Code's project-directory name — mirrors
/// `pulpod::usage::claude::sanitize_workdir` exactly (every non-alphanumeric byte
/// becomes `-`) so the transcript this binary writes lands exactly where the
/// daemon's usage reader looks for it.
fn sanitize_workdir(workdir: &str) -> String {
    workdir
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Haiku's built-in output-token rate (USD per million tokens) — see
/// `pulpod::usage::mod::HAIKU_RATES`. Used to translate a `spend:<usd>` step into a
/// token count that produces that exact cost.
const HAIKU_OUTPUT_RATE_PER_MILLION: f64 = 5.0;

/// Append one assistant transcript record to
/// `$HOME/.claude/projects/<sanitize(cwd)>/<session_id>.jsonl`, in the shape
/// `pulpod::usage::claude::apply_transcript_line` parses: a `timestamp`, and a
/// `message` with a unique `id` and a `usage` block. All-output-tokens, priced at
/// the built-in Haiku rate, so `usd` USD becomes an exact `usage.output_tokens`
/// count — no need to reverse-engineer the full rate table for a round number.
fn append_transcript_spend(cwd: &std::path::Path, session_id: &str, usd: f64) {
    let Ok(home) = std::env::var("HOME") else {
        eprintln!("[fake-claude] spend: HOME not set, cannot write transcript");
        return;
    };
    let project_dir = std::path::Path::new(&home)
        .join(".claude")
        .join("projects")
        .join(sanitize_workdir(&cwd.to_string_lossy()));
    if let Err(error) = std::fs::create_dir_all(&project_dir) {
        eprintln!("[fake-claude] spend: failed to create project dir: {error}");
        return;
    }
    let output_tokens = (usd * 1_000_000.0 / HAIKU_OUTPUT_RATE_PER_MILLION).round() as u64;
    let record = json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "requestId": uuid::Uuid::new_v4().to_string(),
        "type": "assistant",
        "message": {
            "id": uuid::Uuid::new_v4().to_string(),
            "model": "claude-haiku-4-5-fake",
            "usage": {
                "input_tokens": 0,
                "output_tokens": output_tokens,
            },
        },
    });
    let path = project_dir.join(format!("{session_id}.jsonl"));
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        eprintln!("[fake-claude] spend: failed to open transcript {path:?}");
        return;
    };
    let _ = writeln!(file, "{record}");
}

/// Block reading one line from stdin — how `pulpo input <name>` (which sends a
/// literal newline plus a tmux `Enter` keypress) unblocks a fake sitting at
/// `needs_input`. Returns on EOF too, so a closed pipe never hangs the process.
fn block_on_stdin() {
    let stdin = std::io::stdin();
    let mut line = String::new();
    let _ = stdin.lock().read_line(&mut line);
}

/// Sleep forever — models an agent that's still alive but will never itself produce
/// another lifecycle event (a hung turn, or an agent left sitting at its prompt).
fn hang_forever() -> ! {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn base_payload(
    session_id: &str,
    cwd: &std::path::Path,
    transcript_path: &std::path::Path,
) -> Value {
    json!({
        "session_id": session_id,
        "cwd": cwd.to_string_lossy(),
        "transcript_path": transcript_path.to_string_lossy(),
    })
}

fn run_step(
    step: &str,
    settings: &Settings,
    session_id: &str,
    resumed: bool,
    cwd: &std::path::Path,
    transcript_path: &std::path::Path,
) {
    let mut payload = base_payload(session_id, cwd, transcript_path);
    let obj = payload.as_object_mut().expect("payload is an object");

    if let Some((kind, arg)) = step.split_once(':') {
        match kind {
            "spend" => {
                if let Ok(usd) = arg.parse::<f64>() {
                    append_transcript_spend(cwd, session_id, usd);
                }
                return;
            }
            "exit" => {
                let code: i32 = arg.parse().unwrap_or(1);
                // A coded exit models a crash/abrupt exit: no SessionEnd hook fires
                // (a crashed process can't run its own cleanup hooks).
                std::process::exit(code);
            }
            _ => {}
        }
    }

    match step {
        "start" => {
            obj.insert("hook_event_name".into(), json!("SessionStart"));
            obj.insert(
                "source".into(),
                json!(if resumed { "resume" } else { "startup" }),
            );
            settings.fire("SessionStart", &payload);
        }
        "prompt" => {
            obj.insert("hook_event_name".into(), json!("UserPromptSubmit"));
            settings.fire("UserPromptSubmit", &payload);
        }
        "needs_input" => {
            obj.insert("hook_event_name".into(), json!("Notification"));
            obj.insert("notification_type".into(), json!("permission_prompt"));
            obj.insert("message".into(), json!("Allow this action?"));
            settings.fire("Notification", &payload);
        }
        "wait" => {
            block_on_stdin();
            // The input resolved the prompt: the turn continues, then finishes.
            let mut working = base_payload(session_id, cwd, transcript_path);
            working
                .as_object_mut()
                .expect("payload is an object")
                .insert("hook_event_name".into(), json!("UserPromptSubmit"));
            settings.fire("UserPromptSubmit", &working);

            let mut stop = base_payload(session_id, cwd, transcript_path);
            stop.as_object_mut().expect("payload is an object").extend([
                ("hook_event_name".to_owned(), json!("Stop")),
                (
                    "last_assistant_message".to_owned(),
                    json!("Resolved the permission prompt."),
                ),
            ]);
            settings.fire("Stop", &stop);
        }
        "stop" => {
            obj.insert("hook_event_name".into(), json!("Stop"));
            obj.insert("last_assistant_message".into(), json!("Turn complete."));
            settings.fire("Stop", &payload);
        }
        "exit" => {
            obj.insert("hook_event_name".into(), json!("SessionEnd"));
            obj.insert("reason".into(), json!("other"));
            settings.fire("SessionEnd", &payload);
            std::process::exit(0);
        }
        "hang" => hang_forever(),
        other => {
            eprintln!("[fake-claude] unknown scenario step, ignoring: {other}");
        }
    }
}

fn main() {
    let args = parse_args();
    let _ = args.model; // accepted, unused — matches "ignores unknown flags" for flags we do parse but don't act on
    let _ = args.prompt;

    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    write_env_dump(&cwd);

    let resumed = args.resume_id.is_some();
    let session_id = args
        .resume_id
        .or(args.session_id)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let transcript_path = std::env::var("HOME").map_or_else(
        |_| std::path::PathBuf::from(format!("{session_id}.jsonl")),
        |home| {
            std::path::Path::new(&home)
                .join(".claude")
                .join("projects")
                .join(sanitize_workdir(&cwd.to_string_lossy()))
                .join(format!("{session_id}.jsonl"))
        },
    );

    let settings = Settings::load(args.settings_path.as_deref());

    // Resolution order: a `pulpo-fake-scenario.txt` file in the session's workdir
    // (highest priority — lets a scenario test change behavior *between* a spawn
    // and a later resume without touching the tmux command line, which a resume
    // otherwise reuses verbatim apart from the harness-adapter's own flags), then
    // `FAKE_AGENT_SCENARIO` (the common case — set once at spawn time via `env
    // FAKE_AGENT_SCENARIO=... <claude-bin> ...`, which the Claude adapter's
    // `env`-prefix handling passes straight through), then the default.
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
    for step in &steps {
        run_step(
            step,
            &settings,
            &session_id,
            resumed,
            &cwd,
            &transcript_path,
        );
        completed.push(step.clone());
        write_state(&cwd, &session_id, resumed, &completed);
    }

    // Every scenario ends in an explicit `exit`/`exit:N` (which never returns) or a
    // `hang` (which never returns either). Falling off the end of the step list
    // without one of those means the scenario forgot to say how the process should
    // end — default to hanging rather than exiting 0, since a silent clean exit
    // would otherwise mask that as a passing "generic command" run instead of a
    // still-running agent.
    hang_forever();
}
