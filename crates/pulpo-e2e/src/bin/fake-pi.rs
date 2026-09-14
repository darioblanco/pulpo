//! `fake-pi` — a fake harness binary imitating pi's (`@earendil-works/pi-coding-agent`)
//! CLI surface, for the end-to-end scenario suite (`crates/pulpo-e2e`).
//!
//! Pulpo's pi adapter (`crates/pulpod/src/harness/pi.rs`) rewrites a spawned `pi ...`
//! command to `pi --session-id <uuid> -e <ext_path> ...`, where `<ext_path>` is a
//! generated TypeScript extension (`pulpo.ts.tmpl`) wiring pi's own event bus to
//! `pulpo hook pi --event <name>`. Since this binary cannot run TypeScript, it instead
//! reads the extension file to confirm it exists and parses the `PULPO_BIN` path out
//! of it, then emits the same events the extension would by spawning
//! `pulpo hook pi --event <name>` itself with the template's exact JSON payload shape
//! (`event`, `session_id`, `session_file`, `cwd`, `reason`, `kind`,
//! `last_assistant_message`, `stop_reason`, `error`).
//!
//! `--session-id` is pi's idempotent create-or-open flag (scoped to `(cwd,
//! sessionDir)`): this binary honors that by reopening the session file already on
//! disk for the given id under this cwd if one exists, or creating a fresh one
//! otherwise — the same fresh-spawn/resume distinction the adapter's `resume_command`
//! relies on. `--session-id` combined with `-c`/`--continue`, `-r`/`--resume`,
//! `--session`, or `--fork` exits 1, matching real pi's `validateSessionIdFlags`.
//!
//! Behavior is driven entirely by the comma-separated `FAKE_AGENT_SCENARIO` env var
//! (or a `pulpo-fake-scenario.txt` file in the session's workdir), same vocabulary as
//! `fake-claude`/`fake-codex` — see [`run_step`].

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::Utc;
use serde_json::{Value, json};

/// Parsed subset of pi's CLI surface that pulpo's adapter (and this fake) cares
/// about. Every other flag/arg is accepted and ignored.
#[derive(Debug, Default)]
struct Args {
    session_id: Option<String>,
    ext_path: Option<String>,
    prompt: Option<String>,
    model: Option<String>,
    continue_flag: bool,
    resume_flag: bool,
    session_flag: Option<String>,
    fork_flag: Option<String>,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1).peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--session-id" => args.session_id = iter.next(),
            "-e" => args.ext_path = iter.next(),
            "-p" | "--print" => args.prompt = iter.next(),
            "--model" => args.model = iter.next(),
            "-c" | "--continue" => args.continue_flag = true,
            "-r" | "--resume" => args.resume_flag = true,
            "--session" => args.session_flag = iter.next(),
            "--fork" => args.fork_flag = iter.next(),
            "--" => break, // everything after is positional, never a flag
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

/// Read the `-e`-loaded extension file to confirm it exists and pull out the
/// resolved pulpo binary path pulpod templated into it
/// (`const PULPO_BIN = "<path>";` — see `harness::pi::render_extension`). `None`
/// when the file is missing/unreadable or doesn't contain the expected marker,
/// mirroring "the extension load itself degrading gracefully is pi's problem" for a
/// harness too old to have `-e` wired at all.
fn parse_pulpo_bin(ext_path: &str) -> Option<String> {
    let content = std::fs::read_to_string(ext_path).ok()?;
    let marker = "const PULPO_BIN = \"";
    let start = content.find(marker)? + marker.len();
    let end = content[start..].find('"')?;
    Some(content[start..start + end].to_owned())
}

/// Spawn `<pulpo_bin> hook pi --event <name>`, piping `payload` to its stdin —
/// exactly what `pulpo.ts.tmpl`'s `reportEvent` does (minus the JS extension's own
/// detached-process/ordering-chain plumbing, which only matters for a real
/// multi-process pi runtime; this fake runs its steps sequentially already).
fn report_event(pulpo_bin: &str, name: &str, payload: &Value) {
    let child = Command::new(pulpo_bin)
        .args(["hook", "pi", "--event", name])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn();
    match child {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(payload.to_string().as_bytes());
            }
            let _ = child.wait();
        }
        Err(error) => {
            eprintln!("[fake-pi] failed to run hook for {name}: {error}");
        }
    }
}

/// Mangle a cwd into pi's per-directory session folder name convention
/// (`--<cwd-with-slashes-replaced-by-dashes>--`). Not itself load-bearing for
/// `pulpod::usage::pi::scan_sessions` (which walks every `.jsonl` file recursively,
/// regardless of directory naming) — only needs to be a stable, reproducible mapping
/// so a resumed `--session-id` looks in the same directory a prior spawn wrote to.
fn mangle_cwd(cwd: &str) -> String {
    let trimmed = cwd.trim_start_matches('/');
    format!("--{}--", trimmed.replace('/', "-"))
}

/// Find an existing session file for `session_id` under `dir` (matched by filename
/// suffix `_<session_id>.jsonl` — real pi's own filenames are timestamp-prefixed, so
/// a resume can't recompute the exact path, only search for it), or allocate a fresh
/// timestamp-prefixed path when none exists yet. Returns `(path, already_existed)`.
fn find_or_create_session_file(dir: &Path, session_id: &str) -> (PathBuf, bool) {
    let suffix = format!("_{session_id}.jsonl");
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().ends_with(&suffix) {
                return (entry.path(), true);
            }
        }
    }
    let ts = Utc::now().timestamp_millis();
    (dir.join(format!("{ts}{suffix}")), false)
}

/// Append one assistant message record — the shape
/// `pulpod::usage::pi::parse_assistant_message` reads (`usage.input/output/
/// cacheRead/cacheWrite`, `usage.cost.total`).
fn append_assistant_message(path: &Path, model: &str) {
    let record = json!({
        "type": "message",
        "id": uuid::Uuid::new_v4().to_string(),
        "parentId": null,
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "Turn complete."}],
            "api": "anthropic-messages",
            "provider": "anthropic",
            "model": model,
            "usage": {
                "input": 500,
                "output": 50,
                "cacheRead": 100,
                "cacheWrite": 0,
                "totalTokens": 650,
                "cost": {
                    "input": 0.0015,
                    "output": 0.00075,
                    "cacheRead": 0.00003,
                    "cacheWrite": 0.0,
                    "total": 0.00228,
                },
            },
            "stopReason": "stop",
            "timestamp": Utc::now().timestamp_millis(),
        }
    });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{record}");
    }
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

fn base_payload(session_id: &str, session_file: &Path, cwd: &Path) -> Value {
    json!({
        "session_id": session_id,
        "session_file": session_file.to_string_lossy(),
        "cwd": cwd.to_string_lossy(),
    })
}

fn run_step(
    step: &str,
    pulpo_bin: Option<&str>,
    session_id: &str,
    resumed: bool,
    cwd: &Path,
    session_file: &Path,
    model: &str,
) {
    let Some(pulpo_bin) = pulpo_bin else {
        if step != "hang" && !step.starts_with("exit") {
            eprintln!(
                "[fake-pi] no PULPO_BIN resolved (missing/unreadable -e extension), skipping event for step {step:?}"
            );
        }
        if let Some(("exit", arg)) = step.split_once(':') {
            std::process::exit(arg.parse().unwrap_or(1));
        }
        match step {
            "exit" => std::process::exit(0),
            "hang" => hang_forever(),
            _ => return,
        }
    };

    let mut payload = base_payload(session_id, session_file, cwd);
    let obj = payload.as_object_mut().expect("payload is an object");

    if let Some(("exit", arg)) = step.split_once(':') {
        let code: i32 = arg.parse().unwrap_or(1);
        // A coded exit models a crash: no session_shutdown event fires.
        std::process::exit(code);
    }

    match step {
        "start" => {
            obj.insert("event".into(), json!("session_start"));
            obj.insert(
                "reason".into(),
                json!(if resumed { "startup" } else { "new" }),
            );
            obj.insert("previous_session_file".into(), Value::Null);
            report_event(pulpo_bin, "session_start", &payload);
        }
        "prompt" => {
            obj.insert("event".into(), json!("agent_start"));
            report_event(pulpo_bin, "agent_start", &payload);
        }
        "needs_input" => {
            obj.insert("event".into(), json!("ui_prompt_start"));
            obj.insert("kind".into(), json!("confirm"));
            obj.insert("title".into(), json!("Allow this action?"));
            report_event(pulpo_bin, "ui_prompt_start", &payload);
        }
        "wait" => {
            block_on_stdin();
            let mut end = base_payload(session_id, session_file, cwd);
            end.as_object_mut().expect("payload is an object").extend([
                ("event".to_owned(), json!("ui_prompt_end")),
                ("kind".to_owned(), json!("confirm")),
                ("title".to_owned(), json!("Allow this action?")),
            ]);
            report_event(pulpo_bin, "ui_prompt_end", &end);

            append_assistant_message(session_file, model);
            let mut settled = base_payload(session_id, session_file, cwd);
            settled
                .as_object_mut()
                .expect("payload is an object")
                .extend([
                    ("event".to_owned(), json!("agent_settled")),
                    (
                        "last_assistant_message".to_owned(),
                        json!("Resolved the permission prompt."),
                    ),
                    ("stop_reason".to_owned(), json!("stop")),
                    ("error".to_owned(), Value::Null),
                ]);
            report_event(pulpo_bin, "agent_settled", &settled);
        }
        "stop" => {
            append_assistant_message(session_file, model);
            obj.insert("event".into(), json!("agent_settled"));
            obj.insert("last_assistant_message".into(), json!("Turn complete."));
            obj.insert("stop_reason".into(), json!("stop"));
            obj.insert("error".into(), Value::Null);
            report_event(pulpo_bin, "agent_settled", &payload);
        }
        "exit" => {
            obj.insert("event".into(), json!("session_shutdown"));
            obj.insert("reason".into(), json!("quit"));
            report_event(pulpo_bin, "session_shutdown", &payload);
            std::process::exit(0);
        }
        "hang" => hang_forever(),
        other => {
            eprintln!("[fake-pi] unknown scenario step, ignoring: {other}");
        }
    }
}

fn main() {
    let args = parse_args();
    let _ = args.prompt;
    let model = args
        .model
        .clone()
        .unwrap_or_else(|| "fake-pi-model".to_owned());

    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    write_env_dump(&cwd);
    write_argv_dump(&cwd);

    // Mirrors pi's own `validateSessionIdFlags`/`createSessionManager` behavior
    // (verified against 0.85.1 per `harness::pi`'s module doc): `--session-id`
    // combined with any of these is a hard error, exit 1, nothing else runs.
    if args.session_id.is_some()
        && (args.continue_flag
            || args.resume_flag
            || args.session_flag.is_some()
            || args.fork_flag.is_some())
    {
        eprintln!(
            "[fake-pi] Error: --session-id cannot be combined with --continue/-c, --resume/-r, --session, or --fork"
        );
        std::process::exit(1);
    }

    let pulpo_bin = args.ext_path.as_deref().and_then(parse_pulpo_bin);

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let sessions_root = Path::new(&home).join(".pi").join("agent").join("sessions");
    let cwd_str = cwd.to_string_lossy().into_owned();
    let session_dir = sessions_root.join(mangle_cwd(&cwd_str));
    let _ = std::fs::create_dir_all(&session_dir);

    let session_id = args
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let (session_file, existed) = find_or_create_session_file(&session_dir, &session_id);
    if !existed {
        let header = json!({
            "type": "session",
            "version": 3,
            "id": session_id,
            "timestamp": Utc::now().to_rfc3339(),
            "cwd": cwd_str,
        });
        let _ = std::fs::write(&session_file, format!("{header}\n"));
    }
    let resumed = existed;

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
            pulpo_bin.as_deref(),
            &session_id,
            resumed,
            &cwd,
            &session_file,
            &model,
        );
        completed.push(step.clone());
        write_state(&cwd, &session_id, resumed, &completed);
    }

    hang_forever();
}
