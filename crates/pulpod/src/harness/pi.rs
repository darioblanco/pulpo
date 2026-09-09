//! pi adapter — Mario Zechner's coding agent.
//!
//! **Package**: the project moved. `github.com/badlogic/pi-mono` now redirects to
//! `github.com/earendil-works/pi`, and npm `@mariozechner/pi-coding-agent` is
//! deprecated and frozen at `0.73.1` (which has neither `agent_settled` nor
//! `ui_prompt_start`/`ui_prompt_end` nor `--session-id` — the three load-bearing
//! primitives this adapter depends on). The active package is
//! `@earendil-works/pi-coding-agent`; this adapter is verified against `0.85.1`'s
//! shipped `dist/*.d.ts`, `dist/*.js`, and `docs/*.md` (static analysis only — see
//! "Not runtime-tested" below).
//!
//! Injects a generated extension file (`-e <path>`, repeatable/always-safe to add)
//! that wires pi's own event bus (`pi.on("session_start" | "agent_start" |
//! "agent_settled" | "ui_prompt_start" | "ui_prompt_end" | "session_shutdown", ...)`)
//! to spawn `pulpo hook pi --event <name>` per event, and presets `--session-id` so
//! the harness session id is known up front. Unlike Claude Code, pi's `--session-id`
//! is **idempotent create-or-open** (scoped to `(cwd, sessionDir)`) rather than a
//! one-shot creation flag with a separate `--resume`/`--continue` — so there is no
//! separate resume mechanism to switch to: a fresh spawn and a resume look the same
//! shape, `pi --session-id <id> -e <ext_path> <args>`, differing only in whether that
//! id already has a session file on disk. See [`resume_command`][HarnessAdapter::resume_command].
//!
//! ## `--session-id` and `prepare_spawn`'s no-op rule
//!
//! Every other pi flag that selects a session by some other mechanism (`--session`,
//! `--continue`/`-c`, `--resume`/`-r`, `--fork`, `--no-session`) makes `prepare_spawn`
//! a full no-op — pulpo cannot safely force a specific id onto a command the user
//! already pinned to a different session-selection mechanism, and `--session-id`
//! combined with `--session`/`--continue`/`-c`/`--resume`/`-r` is a hard error in pi
//! itself (`validateSessionIdFlags`, exit 1).
//!
//! `--session-id` itself is treated differently: because `resolve_resume_command`
//! (`session/manager.rs`) calls [`resume_command`][HarnessAdapter::resume_command]
//! *before* `prepare_spawn`, and `resume_command` already spliced in
//! `--session-id <id>`, a `prepare_spawn` that fully no-ops on seeing `--session-id`
//! would mean hooks never get re-wired on resume (no `-e` ever added). So
//! `prepare_spawn` special-cases `--session-id`: when present alone (none of the
//! other five flags above), it is left exactly as-is and only `-e <ext_path>` is
//! added (a freshly-rewritten extension file). This is what makes the fresh-spawn and
//! resume shapes match exactly, per the spec's own worked example:
//!
//! | input | `prepare_spawn` output |
//! |---|---|
//! | `pi` | `pi --session-id <sid> -e <ext_path>` |
//! | `pi --session-id <sid>` (from `resume_command`) | `pi --session-id <sid> -e <ext_path>` |
//!
//! ## Open questions / UNVERIFIED (carried from the research spec)
//!
//! - **Version drift**: `matches()` on argv0 basename `pi` can't tell whether an
//!   installed binary is new enough for `agent_settled`/`ui_prompt_*`/
//!   `--session-id` (added somewhere between `0.73.1` and `0.85.1`, exact version
//!   unverified). An older `pi` would silently not fire any of the events this
//!   adapter wires up; it would still run (extension load failures for missing APIs
//!   are pi's problem, not this adapter's), just without lifecycle reporting.
//! - **Not runtime-tested**: everything here is static analysis of shipped
//!   `.d.ts`/`.js`/docs, not a real `pi -e pulpo.ts` run.
//! - **`resumed` on `session_start`**: pi cannot itself distinguish "opened an
//!   existing session" from "created a fresh one with the given id" at process
//!   `"startup"`; `resumed` is advisory only (pulpo's call site already knows the
//!   real answer from which code path ran).
//! - **`--session-id` file-creation timing**: unclear whether pi's session `.jsonl`
//!   is touched on disk at process start or only on first appended entry.

use std::path::Path;

use anyhow::Result;
use serde_json::Value;
use tracing::{info, warn};

use super::{
    HarnessAdapter, HarnessEvent, NeedsInputReason, SpawnContext, SpawnPlan, resolve_pulpo_bin,
};

/// The `pulpo.ts` extension file template, with `PULPO_BIN_PLACEHOLDER` templated to
/// the resolved pulpo binary path at generation time. See [`render_extension`].
const EXTENSION_TEMPLATE: &str = include_str!("pulpo.ts.tmpl");

/// Sentinel in [`EXTENSION_TEMPLATE`] replaced with the resolved pulpo binary path.
const PULPO_BIN_PLACEHOLDER: &str = "__PULPO_BIN_PATH__";

/// Flags that mean session selection is already fully pinned down via a mechanism
/// other than `--session-id` — `prepare_spawn` is a full no-op when any of these is
/// already present (see the module doc's "`--session-id` and `prepare_spawn`'s no-op
/// rule" section for why `--session-id` itself is not in this list).
const SESSION_SELECTION_FLAGS: &[&str] = &[
    "--session",
    "--continue",
    "-c",
    "--resume",
    "-r",
    "--fork",
    "--no-session",
];

/// The subset of [`SESSION_SELECTION_FLAGS`] pi itself rejects when combined with
/// `--session-id` (`validateSessionIdFlags`, exit 1) — `resume_command` refuses to
/// force a specific id onto a command carrying any of these, same "no-op on
/// conflict" rule as `prepare_spawn`.
const RESUME_CONFLICT_FLAGS: &[&str] = &["--session", "--continue", "-c", "--resume", "-r"];

pub struct PiAdapter;

impl HarnessAdapter for PiAdapter {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn matches(&self, argv0: &str) -> bool {
        argv0 == "pi"
    }

    fn prepare_spawn(&self, ctx: &SpawnContext) -> Result<SpawnPlan> {
        let Ok(tokens) = shell_words::split(ctx.command) else {
            warn!(
                session = %ctx.session_name,
                "pi adapter: command did not parse as shell words, spawning unchanged"
            );
            return Ok(SpawnPlan::unchanged(ctx.command));
        };

        if has_flag(&tokens, SESSION_SELECTION_FLAGS) {
            info!(
                session = %ctx.session_name,
                "pi adapter: command already pins session selection via another flag, spawning unchanged"
            );
            return Ok(SpawnPlan::unchanged(ctx.command));
        }

        let Some(pi_idx) = pi_token_index(&tokens) else {
            return Ok(SpawnPlan::unchanged(ctx.command));
        };

        match rewrite_spawn(ctx, tokens, pi_idx) {
            Ok(plan) => Ok(plan),
            Err(error) => {
                warn!(
                    session = %ctx.session_name,
                    %error,
                    "pi adapter: failed to prepare spawn, spawning unchanged"
                );
                Ok(SpawnPlan::unchanged(ctx.command))
            }
        }
    }

    fn resume_command(&self, original_command: &str, harness_session_id: &str) -> Option<String> {
        let mut tokens = shell_words::split(original_command).ok()?;
        let pi_idx = pi_token_index(&tokens)?;

        if has_flag(&tokens, RESUME_CONFLICT_FLAGS) {
            return None;
        }

        strip_flag_with_value(&mut tokens, "--session-id");
        tokens.splice(
            (pi_idx + 1)..=pi_idx,
            ["--session-id".to_owned(), harness_session_id.to_owned()],
        );
        Some(shell_words::join(&tokens))
    }

    fn parse_event(&self, raw: &Value) -> Result<Option<HarnessEvent>> {
        let event_name = raw.get("event").and_then(Value::as_str).unwrap_or("");

        Ok(match event_name {
            "session_start" => {
                let harness_session_id = raw
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let resumed = raw.get("reason").and_then(Value::as_str) != Some("new");
                Some(HarnessEvent::SessionStarted {
                    harness_session_id,
                    resumed,
                })
            }
            "agent_start" | "ui_prompt_end" => Some(HarnessEvent::Working),
            "agent_settled" => Some(raw.get("error").and_then(Value::as_str).map_or_else(
                || {
                    HarnessEvent::TurnFinished {
                        summary: raw
                            .get("last_assistant_message")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    }
                },
                |error| HarnessEvent::Failed {
                    rate_limited: is_rate_limited(error),
                    error: error.to_owned(),
                },
            )),
            "ui_prompt_start" => {
                let reason = if raw.get("kind").and_then(Value::as_str) == Some("confirm") {
                    NeedsInputReason::Permission
                } else {
                    NeedsInputReason::Question
                };
                Some(HarnessEvent::NeedsInput { reason })
            }
            "session_shutdown" => {
                (raw.get("reason").and_then(Value::as_str) == Some("quit")).then(|| {
                    HarnessEvent::SessionEnded {
                        reason: Some("quit".to_owned()),
                    }
                })
            }
            _ => None,
        })
    }

    fn emits_events(&self) -> bool {
        true
    }
}

/// The fallible part of [`PiAdapter::prepare_spawn`]: write the extension file and
/// splice in the new flags. Isolated so the caller can catch any I/O error and fall
/// back to the unchanged command instead of failing the spawn.
fn rewrite_spawn(ctx: &SpawnContext, mut tokens: Vec<String>, pi_idx: usize) -> Result<SpawnPlan> {
    let harness_dir = ctx.data_dir.join("harness").join(ctx.session_id);
    std::fs::create_dir_all(&harness_dir)?;
    let ext_path = harness_dir.join("pulpo.ts");
    let pulpo_bin = resolve_pulpo_bin();
    std::fs::write(&ext_path, render_extension(&pulpo_bin))?;
    let ext_arg = ext_path.to_string_lossy().into_owned();

    // If `--session-id` is already present (from a prior `resume_command` splice, or
    // a user-supplied one), keep it exactly as-is and just wire up `-e`. Otherwise
    // this is a genuinely fresh spawn: mint the id (reusing the pulpo session uuid —
    // it already matches pi's id regex) and insert both flags after argv0.
    let harness_session_id = if let Some(idx) = tokens.iter().position(|t| t == "--session-id") {
        let existing = tokens.get(idx + 1).cloned();
        let insert_at = idx + 1 + usize::from(existing.is_some());
        tokens.splice(insert_at..insert_at, ["-e".to_owned(), ext_arg]);
        existing
    } else {
        let sid = ctx.session_id.to_owned();
        tokens.splice(
            (pi_idx + 1)..=pi_idx,
            [
                "--session-id".to_owned(),
                sid.clone(),
                "-e".to_owned(),
                ext_arg,
            ],
        );
        Some(sid)
    };

    Ok(SpawnPlan {
        command: shell_words::join(&tokens),
        env: Vec::new(),
        files: vec![ext_path],
        harness_session_id,
    })
}

/// Render the `pulpo.ts` extension file, templating [`PULPO_BIN_PLACEHOLDER`] to the
/// resolved pulpo binary path.
fn render_extension(pulpo_bin: &str) -> String {
    EXTENSION_TEMPLATE.replace(PULPO_BIN_PLACEHOLDER, &escape_ts_string(pulpo_bin))
}

/// Escape a string for embedding in a double-quoted TypeScript string literal.
/// Pulpo only targets macOS/Linux, so a resolved binary path is a plain POSIX path in
/// practice — this is defense-in-depth, not a load-bearing path.
fn escape_ts_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Case-insensitive match for the rate-limit subset of pi's own broader
/// retry-classifier (`@earendil-works/pi-ai`'s `isRetryableAssistantError`, which
/// additionally treats 500/502/503/504/524/timeouts/overloaded as retryable but not
/// necessarily rate-limited — narrowed here to pulpo's `rate_limited` flag).
fn is_rate_limited(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("429")
        || lower.contains("too many requests")
        || ["rate limit", "ratelimit", "rate-limit", "rate_limit"]
            .iter()
            .any(|needle| lower.contains(needle))
}

/// True if `tokens` contains any of `flags`. Unlike Claude Code, pi's own CLI parser
/// (`dist/cli/args.js`) only ever accepts the space-separated form for its flags —
/// no `--flag=value` — so an exact token match is sufficient.
fn has_flag(tokens: &[String], flags: &[&str]) -> bool {
    tokens.iter().any(|token| flags.contains(&token.as_str()))
}

/// Index of the `pi` token in `tokens`, skipping a leading `env` invocation and its
/// assignments/flags the same way [`super::registry`]'s basename resolution does.
fn pi_token_index(tokens: &[String]) -> Option<usize> {
    let mut idx = 0;
    if Path::new(tokens.first()?)
        .file_name()
        .and_then(|f| f.to_str())
        == Some("env")
    {
        idx = 1;
        while idx < tokens.len() {
            let token = &tokens[idx];
            if token.starts_with('-') || token.contains('=') {
                idx += 1;
                continue;
            }
            break;
        }
    }
    let token = tokens.get(idx)?;
    (Path::new(token).file_name().and_then(|f| f.to_str()) == Some("pi")).then_some(idx)
}

/// Remove every occurrence of `flag <value>` from `tokens` (space-separated form
/// only — see [`has_flag`]).
fn strip_flag_with_value(tokens: &mut Vec<String>, flag: &str) {
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == flag {
            tokens.remove(i);
            if i < tokens.len() {
                tokens.remove(i);
            }
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(data_dir: &'a Path, command: &'a str) -> SpawnContext<'a> {
        SpawnContext {
            session_id: "11111111-1111-1111-1111-111111111111",
            session_name: "sess",
            workdir: "/tmp/repo",
            command,
            data_dir,
        }
    }

    // -- matches --

    #[test]
    fn test_matches_pi_only() {
        assert!(PiAdapter.matches("pi"));
        assert!(!PiAdapter.matches("claude"));
        assert!(!PiAdapter.matches(""));
    }

    #[test]
    fn test_matches_does_not_match_npx_pi() {
        // matches() receives only the basename of argv[0] (per the trait contract,
        // resolved by `HarnessRegistry`) — "npx" is a different binary than "pi" and
        // is never rewritten to "pi" by basename resolution, so a command like
        // `npx pi` resolves to argv0 basename "npx", which this adapter correctly
        // does not claim. pi run via `npx` therefore falls through to
        // `GenericAdapter` — a known, documented gap (not a bug): pulpo would need
        // `npx`-aware unwrapping (peeking at npx's own argv) to special-case this,
        // which is out of scope here.
        assert!(!PiAdapter.matches("npx"));
    }

    #[test]
    fn test_id_and_emits_events() {
        assert_eq!(PiAdapter.id(), "pi");
        assert!(PiAdapter.emits_events());
    }

    // -- prepare_spawn: fresh rewrite --

    #[test]
    fn test_prepare_spawn_rewrites_plain_pi() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = PiAdapter.prepare_spawn(&ctx(tmp.path(), "pi")).unwrap();

        let sid = "11111111-1111-1111-1111-111111111111";
        assert_eq!(plan.harness_session_id.as_deref(), Some(sid));
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(tokens[0], "pi");
        assert_eq!(tokens[1], "--session-id");
        assert_eq!(tokens[2], sid);
        assert_eq!(tokens[3], "-e");
        assert_eq!(plan.files.len(), 1);
        assert!(plan.files[0].exists());
        assert_eq!(tokens[4], plan.files[0].to_string_lossy());
    }

    #[test]
    fn test_prepare_spawn_preserves_original_args() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = PiAdapter
            .prepare_spawn(&ctx(tmp.path(), "pi --model x"))
            .unwrap();
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(&tokens[5..], ["--model", "x"]);
    }

    #[test]
    fn test_prepare_spawn_preserves_print_prompt_arg() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = PiAdapter
            .prepare_spawn(&ctx(tmp.path(), "pi -p 'prompt'"))
            .unwrap();
        assert!(plan.command.ends_with("-p prompt"));
    }

    #[test]
    fn test_prepare_spawn_writes_extension_under_harness_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = PiAdapter.prepare_spawn(&ctx(tmp.path(), "pi")).unwrap();
        let expected_dir = tmp
            .path()
            .join("harness")
            .join("11111111-1111-1111-1111-111111111111");
        assert_eq!(plan.files[0].parent().unwrap(), expected_dir);
        assert_eq!(plan.files[0].file_name().unwrap(), "pulpo.ts");
    }

    #[test]
    fn test_prepare_spawn_extension_contains_templated_bin_and_every_handler() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = PiAdapter.prepare_spawn(&ctx(tmp.path(), "pi")).unwrap();
        let written = std::fs::read_to_string(&plan.files[0]).unwrap();

        let pulpo_bin = resolve_pulpo_bin();
        assert!(written.contains(&format!("const PULPO_BIN = \"{pulpo_bin}\";")));
        assert!(!written.contains(PULPO_BIN_PLACEHOLDER));

        for event in [
            "session_start",
            "agent_start",
            "agent_end",
            "agent_settled",
            "ui_prompt_start",
            "ui_prompt_end",
            "session_shutdown",
        ] {
            assert!(
                written.contains(&format!("pi.on(\"{event}\",")),
                "missing pi.on(\"{event}\", ...) registration"
            );
        }
    }

    #[test]
    fn test_prepare_spawn_handles_absolute_path_argv0() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = PiAdapter
            .prepare_spawn(&ctx(tmp.path(), "/usr/local/bin/pi -p hi"))
            .unwrap();
        assert!(plan.harness_session_id.is_some());
        assert!(plan.command.starts_with("/usr/local/bin/pi --session-id"));
    }

    #[test]
    fn test_prepare_spawn_handles_env_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = PiAdapter
            .prepare_spawn(&ctx(tmp.path(), "env FOO=bar pi -p hi"))
            .unwrap();
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(tokens[0], "env");
        assert_eq!(tokens[1], "FOO=bar");
        assert_eq!(tokens[2], "pi");
        assert_eq!(tokens[3], "--session-id");
    }

    // -- prepare_spawn: existing --session-id (resume shape) --

    #[test]
    fn test_prepare_spawn_keeps_existing_session_id_and_adds_extension_only() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = PiAdapter
            .prepare_spawn(&ctx(tmp.path(), "pi --session-id my-sid --model x"))
            .unwrap();
        assert_eq!(plan.harness_session_id.as_deref(), Some("my-sid"));
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(tokens[0], "pi");
        assert_eq!(tokens[1], "--session-id");
        assert_eq!(tokens[2], "my-sid");
        assert_eq!(tokens[3], "-e");
        assert_eq!(&tokens[5..], ["--model", "x"]);
        assert_eq!(plan.files.len(), 1);
        assert!(plan.files[0].exists());
    }

    #[test]
    fn test_prepare_spawn_fresh_and_resume_produce_identical_shape() {
        // The invariant the spec calls out explicitly: because `--session-id` is
        // idempotent create-or-open, a fresh spawn and `resume_command`'s output run
        // back through `prepare_spawn` must yield the exact same command shape.
        let tmp = tempfile::tempdir().unwrap();
        let fresh = PiAdapter.prepare_spawn(&ctx(tmp.path(), "pi")).unwrap();
        let sid = fresh.harness_session_id.clone().unwrap();

        let resumed_base = PiAdapter.resume_command("pi", &sid).unwrap();
        let resumed = PiAdapter
            .prepare_spawn(&ctx(tmp.path(), &resumed_base))
            .unwrap();

        assert_eq!(fresh.command, resumed.command);
    }

    // -- prepare_spawn: no-op paths --

    #[test]
    fn test_prepare_spawn_skips_for_each_session_selection_flag() {
        let tmp = tempfile::tempdir().unwrap();
        for command in [
            "pi --session /tmp/x.jsonl",
            "pi --continue",
            "pi -c",
            "pi --resume",
            "pi -r",
            "pi --fork abc",
            "pi --no-session",
        ] {
            let plan = PiAdapter.prepare_spawn(&ctx(tmp.path(), command)).unwrap();
            assert_eq!(plan.command, command, "expected no-op for {command:?}");
            assert!(plan.harness_session_id.is_none());
            assert!(plan.files.is_empty());
        }
    }

    #[test]
    fn test_prepare_spawn_falls_back_on_unparseable_command() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = PiAdapter
            .prepare_spawn(&ctx(tmp.path(), "pi \"unterminated"))
            .unwrap();
        assert_eq!(plan.command, "pi \"unterminated");
        assert!(plan.harness_session_id.is_none());
    }

    #[test]
    fn test_prepare_spawn_not_pi_returns_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = PiAdapter.prepare_spawn(&ctx(tmp.path(), "bash")).unwrap();
        assert_eq!(plan.command, "bash");
        assert!(plan.harness_session_id.is_none());
    }

    #[test]
    fn test_prepare_spawn_falls_back_when_data_dir_cannot_be_created() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let plan = PiAdapter.prepare_spawn(&ctx(&blocker, "pi")).unwrap();
        assert_eq!(plan.command, "pi");
        assert!(plan.harness_session_id.is_none());
        assert!(plan.files.is_empty());
    }

    // -- resume_command --

    #[test]
    fn test_resume_command_plain() {
        let cmd = PiAdapter.resume_command("pi", "sid-1").unwrap();
        assert_eq!(cmd, "pi --session-id sid-1");
    }

    #[test]
    fn test_resume_command_preserves_other_args() {
        let cmd = PiAdapter.resume_command("pi --model x", "sid-1").unwrap();
        assert_eq!(cmd, "pi --session-id sid-1 --model x");
    }

    #[test]
    fn test_resume_command_preserves_print_prompt_arg() {
        let cmd = PiAdapter.resume_command("pi -p 'prompt'", "sid-1").unwrap();
        assert_eq!(cmd, "pi --session-id sid-1 -p prompt");
    }

    #[test]
    fn test_resume_command_strips_existing_session_id() {
        let cmd = PiAdapter
            .resume_command("pi --session-id old-sid --model x", "sid-new")
            .unwrap();
        assert_eq!(cmd, "pi --session-id sid-new --model x");
    }

    #[test]
    fn test_resume_command_preserves_env_prefix() {
        let cmd = PiAdapter
            .resume_command("env FOO=bar pi -p hi", "sid-1")
            .unwrap();
        let tokens = shell_words::split(&cmd).unwrap();
        assert_eq!(
            tokens,
            ["env", "FOO=bar", "pi", "--session-id", "sid-1", "-p", "hi"]
        );
    }

    #[test]
    fn test_resume_command_none_when_session_flag_present() {
        assert!(
            PiAdapter
                .resume_command("pi --session /tmp/x.jsonl", "sid-1")
                .is_none()
        );
    }

    #[test]
    fn test_resume_command_none_when_continue_flag_present() {
        assert!(PiAdapter.resume_command("pi --continue", "sid-1").is_none());
        assert!(PiAdapter.resume_command("pi -c", "sid-1").is_none());
    }

    #[test]
    fn test_resume_command_none_when_resume_flag_present() {
        assert!(PiAdapter.resume_command("pi --resume", "sid-1").is_none());
        assert!(PiAdapter.resume_command("pi -r", "sid-1").is_none());
    }

    #[test]
    fn test_resume_command_none_when_not_pi() {
        assert!(PiAdapter.resume_command("bash", "sid-1").is_none());
    }

    #[test]
    fn test_resume_command_none_when_unparseable() {
        assert!(
            PiAdapter
                .resume_command("pi \"unterminated", "sid-1")
                .is_none()
        );
    }

    #[test]
    fn test_resume_command_fork_and_no_session_are_not_conflicts() {
        // --fork and --no-session are not in pi's own validateSessionIdFlags
        // conflict set, so resume_command still rewrites them (whether that
        // combination is *useful* is the caller's problem, not this adapter's).
        assert!(PiAdapter.resume_command("pi --fork abc", "sid-1").is_some());
        assert!(
            PiAdapter
                .resume_command("pi --no-session", "sid-1")
                .is_some()
        );
    }

    // -- parse_event --

    #[test]
    fn test_parse_event_session_start_new_is_not_resumed() {
        let raw = serde_json::json!({
            "event": "session_start",
            "session_id": "sid-1",
            "reason": "new",
        });
        assert_eq!(
            PiAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::SessionStarted {
                harness_session_id: Some("sid-1".into()),
                resumed: false,
            }
        );
    }

    #[test]
    fn test_parse_event_session_start_other_reasons_are_resumed() {
        for reason in ["startup", "reload", "resume", "fork"] {
            let raw = serde_json::json!({
                "event": "session_start",
                "session_id": "sid-1",
                "reason": reason,
            });
            assert_eq!(
                PiAdapter.parse_event(&raw).unwrap().unwrap(),
                HarnessEvent::SessionStarted {
                    harness_session_id: Some("sid-1".into()),
                    resumed: true,
                },
                "reason {reason:?} should be treated as resumed"
            );
        }
    }

    #[test]
    fn test_parse_event_agent_start_is_working() {
        let raw = serde_json::json!({"event": "agent_start"});
        assert_eq!(
            PiAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::Working
        );
    }

    #[test]
    fn test_parse_event_agent_settled_turn_finished_with_summary() {
        let raw = serde_json::json!({
            "event": "agent_settled",
            "last_assistant_message": "Fixed the bug",
            "stop_reason": "stop",
            "error": null,
        });
        assert_eq!(
            PiAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::TurnFinished {
                summary: Some("Fixed the bug".into())
            }
        );
    }

    #[test]
    fn test_parse_event_agent_settled_no_summary() {
        let raw = serde_json::json!({"event": "agent_settled"});
        assert_eq!(
            PiAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::TurnFinished { summary: None }
        );
    }

    #[test]
    fn test_parse_event_agent_settled_failed_rate_limited() {
        for error in [
            "rate limit exceeded",
            "ratelimit hit",
            "rate-limit",
            "rate_limit_error",
            "429",
            "HTTP 429 received",
            "Too Many Requests",
        ] {
            let raw = serde_json::json!({"event": "agent_settled", "error": error});
            assert_eq!(
                PiAdapter.parse_event(&raw).unwrap().unwrap(),
                HarnessEvent::Failed {
                    error: error.into(),
                    rate_limited: true,
                },
                "error {error:?} should be classified as rate-limited"
            );
        }
    }

    #[test]
    fn test_parse_event_agent_settled_failed_not_rate_limited() {
        let raw = serde_json::json!({"event": "agent_settled", "error": "connection reset"});
        assert_eq!(
            PiAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::Failed {
                error: "connection reset".into(),
                rate_limited: false,
            }
        );
    }

    #[test]
    fn test_parse_event_ui_prompt_start_confirm_is_permission() {
        let raw = serde_json::json!({"event": "ui_prompt_start", "kind": "confirm"});
        assert_eq!(
            PiAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Permission
            }
        );
    }

    #[test]
    fn test_parse_event_ui_prompt_start_other_kinds_are_question() {
        for kind in ["select", "input", "editor", "custom"] {
            let raw = serde_json::json!({"event": "ui_prompt_start", "kind": kind});
            assert_eq!(
                PiAdapter.parse_event(&raw).unwrap().unwrap(),
                HarnessEvent::NeedsInput {
                    reason: NeedsInputReason::Question
                },
                "kind {kind:?} should map to Question"
            );
        }
    }

    #[test]
    fn test_parse_event_ui_prompt_start_missing_kind_is_question() {
        let raw = serde_json::json!({"event": "ui_prompt_start"});
        assert_eq!(
            PiAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Question
            }
        );
    }

    #[test]
    fn test_parse_event_ui_prompt_end_is_working() {
        let raw = serde_json::json!({"event": "ui_prompt_end"});
        assert_eq!(
            PiAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::Working
        );
    }

    #[test]
    fn test_parse_event_session_shutdown_quit_is_session_ended() {
        let raw = serde_json::json!({"event": "session_shutdown", "reason": "quit"});
        assert_eq!(
            PiAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::SessionEnded {
                reason: Some("quit".into())
            }
        );
    }

    #[test]
    fn test_parse_event_session_shutdown_non_quit_reasons_are_ignored() {
        for reason in ["reload", "new", "resume", "fork"] {
            let raw = serde_json::json!({"event": "session_shutdown", "reason": reason});
            assert!(
                PiAdapter.parse_event(&raw).unwrap().is_none(),
                "reason {reason:?} should be ignored"
            );
        }
    }

    #[test]
    fn test_parse_event_session_shutdown_missing_reason_is_ignored() {
        let raw = serde_json::json!({"event": "session_shutdown"});
        assert!(PiAdapter.parse_event(&raw).unwrap().is_none());
    }

    #[test]
    fn test_parse_event_unknown_event_is_none() {
        let raw = serde_json::json!({"event": "turn_start"});
        assert!(PiAdapter.parse_event(&raw).unwrap().is_none());
    }

    #[test]
    fn test_parse_event_missing_event_field_is_none() {
        let raw = serde_json::json!({});
        assert!(PiAdapter.parse_event(&raw).unwrap().is_none());
    }

    // -- small helpers --

    #[test]
    fn test_escape_ts_string_escapes_backslash_and_quote() {
        assert_eq!(escape_ts_string(r#"C:\weird"path"#), r#"C:\\weird\"path"#);
        assert_eq!(
            escape_ts_string("/usr/local/bin/pulpo"),
            "/usr/local/bin/pulpo"
        );
    }

    #[test]
    fn test_has_flag_false_for_clean_command() {
        let tokens = vec!["pi".to_owned(), "-p".to_owned(), "hi".to_owned()];
        assert!(!has_flag(&tokens, SESSION_SELECTION_FLAGS));
    }

    #[test]
    fn test_pi_token_index_not_found() {
        let tokens = vec!["bash".to_owned()];
        assert!(pi_token_index(&tokens).is_none());
    }

    #[test]
    fn test_pi_token_index_env_with_no_command_after() {
        let tokens = vec!["env".to_owned(), "FOO=bar".to_owned()];
        assert!(pi_token_index(&tokens).is_none());
    }

    #[test]
    fn test_strip_flag_with_value_removes_flag_and_value() {
        let mut tokens = vec![
            "pi".to_owned(),
            "--session-id".to_owned(),
            "old".to_owned(),
            "--model".to_owned(),
            "x".to_owned(),
        ];
        strip_flag_with_value(&mut tokens, "--session-id");
        assert_eq!(tokens, vec!["pi", "--model", "x"]);
    }

    #[test]
    fn test_strip_flag_with_value_noop_when_absent() {
        let mut tokens = vec!["pi".to_owned(), "--model".to_owned(), "x".to_owned()];
        strip_flag_with_value(&mut tokens, "--session-id");
        assert_eq!(tokens, vec!["pi", "--model", "x"]);
    }
}
