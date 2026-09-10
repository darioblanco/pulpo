//! Claude Code adapter — the first concrete [`super::HarnessAdapter`].
//!
//! Injects a one-shot `--settings` file (highest CLI precedence) that wires every
//! lifecycle hook of interest to `pulpo hook claude`, and presets `--session-id` so
//! the harness session id is known up front (resume works even if the `SessionStart`
//! hook never fires — e.g. the agent crashes before Claude Code's hook runner starts).
//!
//! Verified against Claude Code v2.1.266 (`claude --help`): `--session-id <uuid>`,
//! `--settings <file-or-json>`, `-r/--resume [session-id]`, `-c/--continue` all exist.
//! The exact JSON field a `Notification` hook uses to carry its matcher tag
//! (`permission_prompt`, `idle_prompt`, ...) could not be independently confirmed
//! from the CLI surface alone, so [`classify_notification`] checks a few plausible
//! field names before falling back to keyword-matching the human-readable `message`
//! — see its doc comment.

use std::path::Path;

use anyhow::Result;
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

#[cfg(test)]
use super::resolve_pulpo_bin_from;
use super::{
    HarnessAdapter, HarnessEvent, NeedsInputReason, SpawnContext, SpawnPlan, resolve_pulpo_bin,
};

/// Flags that mean session identity is already fully pinned down (by the user, or by
/// a previous pulpo rewrite) — pulpo must never double-inject on top of these.
const IDENTITY_FLAGS: &[&str] = &["--session-id", "--settings"];

/// Flags that mean this invocation is resuming/continuing an existing conversation.
/// `prepare_spawn` still re-injects `--settings` for these (hooks must stay wired on
/// resume) but must not add `--session-id` — Claude Code doesn't expect both a resume
/// target and a preset id on the same invocation.
const RESUME_FLAGS: &[&str] = &["--resume", "-r", "--continue", "-c"];

/// The `Notification` matcher regex installed in pulpo's own settings file — every
/// notification "kind" pulpo's Claude adapter currently understands.
const NOTIFICATION_MATCHER: &str =
    "permission_prompt|idle_prompt|agent_needs_input|elicitation_dialog|elicitation_url_dialog";

pub struct ClaudeAdapter;

impl HarnessAdapter for ClaudeAdapter {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn matches(&self, argv0: &str) -> bool {
        argv0 == "claude"
    }

    fn prepare_spawn(&self, ctx: &SpawnContext) -> Result<SpawnPlan> {
        let Ok(tokens) = shell_words::split(ctx.command) else {
            warn!(
                session = %ctx.session_name,
                "claude adapter: command did not parse as shell words, spawning unchanged"
            );
            return Ok(SpawnPlan::unchanged(ctx.command));
        };

        if has_flag(&tokens, IDENTITY_FLAGS) {
            info!(
                session = %ctx.session_name,
                "claude adapter: command already sets --session-id/--settings, spawning unchanged"
            );
            return Ok(SpawnPlan::unchanged(ctx.command));
        }

        let Some(claude_idx) = claude_token_index(&tokens) else {
            return Ok(SpawnPlan::unchanged(ctx.command));
        };

        // Resuming/continuing: still re-inject --settings (hooks must stay wired) but
        // never --session-id — Claude Code doesn't expect both on one invocation, and
        // `resume_command` already carries the real id via --resume.
        let resuming = has_flag(&tokens, RESUME_FLAGS);

        match rewrite_spawn(ctx, tokens, claude_idx, resuming) {
            Ok(plan) => Ok(plan),
            Err(error) => {
                warn!(
                    session = %ctx.session_name,
                    %error,
                    "claude adapter: failed to prepare spawn, spawning unchanged"
                );
                Ok(SpawnPlan::unchanged(ctx.command))
            }
        }
    }

    fn resume_command(&self, original_command: &str, harness_session_id: &str) -> Option<String> {
        let mut tokens = shell_words::split(original_command).ok()?;
        let claude_idx = claude_token_index(&tokens)?;
        strip_flag_with_value(&mut tokens, "--session-id");
        strip_flag_with_value(&mut tokens, "--settings");
        // Also strip any resume/continue flags already on the original command (e.g.
        // a session spawned with `claude --continue`, or already resumed once with
        // `--resume <id>`) — otherwise splicing in pulpo's own `--resume
        // <harness_session_id>` below would produce two conflicting resume/continue
        // flags on the same invocation, which Claude Code rejects.
        strip_optional_value_flag(&mut tokens, "--resume");
        strip_optional_value_flag(&mut tokens, "-r");
        tokens.retain(|t| t != "--continue" && t != "-c");
        tokens.splice(
            (claude_idx + 1)..=claude_idx,
            ["--resume".to_owned(), harness_session_id.to_owned()],
        );
        Some(shell_words::join(&tokens))
    }

    fn parse_event(&self, raw: &Value) -> Result<Option<HarnessEvent>> {
        let hook_event_name = raw
            .get("hook_event_name")
            .and_then(Value::as_str)
            .unwrap_or("");

        Ok(match hook_event_name {
            "SessionStart" => {
                let harness_session_id = raw
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let resumed = raw.get("source").and_then(Value::as_str) == Some("resume");
                Some(HarnessEvent::SessionStarted {
                    harness_session_id,
                    resumed,
                })
            }
            "UserPromptSubmit" => Some(HarnessEvent::Working),
            "Stop" => {
                let summary = raw
                    .get("last_assistant_message")
                    .or_else(|| raw.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                Some(HarnessEvent::TurnFinished { summary })
            }
            "StopFailure" => {
                let error = raw
                    .get("error_type")
                    .or_else(|| raw.get("error"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
                    .to_owned();
                let lower = error.to_lowercase();
                let rate_limited = ["rate", "429", "overloaded"]
                    .iter()
                    .any(|needle| lower.contains(needle));
                Some(HarnessEvent::Failed {
                    error,
                    rate_limited,
                })
            }
            "SessionEnd" => {
                let reason = raw.get("reason").and_then(Value::as_str).map(str::to_owned);
                // `clear` (`/clear`) and `resume` (`/resume`) are in-process session
                // replacement, not the harness process exiting — verified against the
                // v2.1.266 binary's reason enum: `clear, resume, logout,
                // prompt_input_exit, other`. Treating either as `SessionEnded` would
                // flip a still-running session to `Ready`/`Stopped` out from under
                // itself; `Ok(None)` leaves state untouched, matching how pi's adapter
                // treats its own in-process `session_shutdown` reasons.
                if matches!(reason.as_deref(), Some("clear" | "resume")) {
                    None
                } else {
                    Some(HarnessEvent::SessionEnded { reason })
                }
            }
            "Notification" => {
                classify_notification(raw).map(|reason| HarnessEvent::NeedsInput { reason })
            }
            _ => None,
        })
    }

    fn emits_events(&self) -> bool {
        true
    }
}

/// Classify a `Notification` hook payload into a [`NeedsInputReason`].
///
/// Checks a few plausible field names (`matcher`, `notification_type`, `type`) for
/// the notification "kind" tag pulpo's own settings matcher regex
/// ([`NOTIFICATION_MATCHER`]) filters on; falls back to keyword-matching the
/// human-readable `message` text when none of those are present. Returns `None` only
/// when there's nothing to classify at all (no tag, no message).
fn classify_notification(raw: &Value) -> Option<NeedsInputReason> {
    let tag = raw
        .get("matcher")
        .or_else(|| raw.get("notification_type"))
        .or_else(|| raw.get("type"))
        .and_then(Value::as_str);
    if let Some(tag) = tag {
        return Some(reason_for_tag(tag));
    }

    let message = raw.get("message").and_then(Value::as_str)?.to_lowercase();
    if message.contains("permission") {
        Some(NeedsInputReason::Permission)
    } else if message.contains("idle") {
        Some(NeedsInputReason::Idle)
    } else if ["input", "question", "elicit"]
        .iter()
        .any(|needle| message.contains(needle))
    {
        Some(NeedsInputReason::Question)
    } else if message.is_empty() {
        None
    } else {
        Some(NeedsInputReason::Other(message))
    }
}

fn reason_for_tag(tag: &str) -> NeedsInputReason {
    match tag {
        "permission_prompt" => NeedsInputReason::Permission,
        "idle_prompt" => NeedsInputReason::Idle,
        "agent_needs_input" | "elicitation_dialog" | "elicitation_url_dialog" => {
            NeedsInputReason::Question
        }
        other => NeedsInputReason::Other(other.to_owned()),
    }
}

/// True if `tokens` contains any of `flags`, matching either the bare flag
/// (`--session-id`) or its `--flag=value` form.
fn has_flag(tokens: &[String], flags: &[&str]) -> bool {
    tokens.iter().any(|token| {
        flags
            .iter()
            .any(|flag| token == flag || token.starts_with(&format!("{flag}=")))
    })
}

/// Index of the `claude` token in `tokens`, skipping a leading `env` invocation and
/// its assignments/flags the same way [`super::registry`]'s basename resolution does.
fn claude_token_index(tokens: &[String]) -> Option<usize> {
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
    (Path::new(token).file_name().and_then(|f| f.to_str()) == Some("claude")).then_some(idx)
}

/// Remove every occurrence of `flag <value>` or `flag=value` from `tokens`.
fn strip_flag_with_value(tokens: &mut Vec<String>, flag: &str) {
    let prefix = format!("{flag}=");
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == flag {
            tokens.remove(i);
            if i < tokens.len() {
                tokens.remove(i);
            }
        } else if tokens[i].starts_with(&prefix) {
            tokens.remove(i);
        } else {
            i += 1;
        }
    }
}

/// Remove every occurrence of `flag` from `tokens`, along with a following bare value
/// when one is present — used for flags whose value is optional (Claude's
/// `-r`/`--resume [sessionId]`), where a value, if given, never itself looks like
/// another `-`-prefixed flag. Also strips the `flag=value` form.
fn strip_optional_value_flag(tokens: &mut Vec<String>, flag: &str) {
    let prefix = format!("{flag}=");
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == flag {
            tokens.remove(i);
            if i < tokens.len() && !tokens[i].starts_with('-') {
                tokens.remove(i);
            }
        } else if tokens[i].starts_with(&prefix) {
            tokens.remove(i);
        } else {
            i += 1;
        }
    }
}

/// The fallible part of [`ClaudeAdapter::prepare_spawn`]: write the settings file and
/// splice in the new flags. Isolated so the caller can catch any I/O error and fall
/// back to the unchanged command instead of failing the spawn.
///
/// `resuming` is true when the command already carries `--resume`/`-r`/`--continue`/
/// `-c`: `--settings` is still injected (so hooks stay wired) but `--session-id` is
/// not, and no `harness_session_id` is reported (the caller already knows it).
fn rewrite_spawn(
    ctx: &SpawnContext,
    mut tokens: Vec<String>,
    claude_idx: usize,
    resuming: bool,
) -> Result<SpawnPlan> {
    let harness_dir = ctx.data_dir.join("harness").join(ctx.session_id);
    std::fs::create_dir_all(&harness_dir)?;
    let settings_path = harness_dir.join("claude-settings.json");
    let pulpo_bin = resolve_pulpo_bin();
    std::fs::write(&settings_path, build_settings_json(&pulpo_bin))?;
    let settings_arg = settings_path.to_string_lossy().into_owned();

    let harness_session_id = if resuming {
        tokens.splice(
            (claude_idx + 1)..=claude_idx,
            ["--settings".to_owned(), settings_arg],
        );
        None
    } else {
        let sid = Uuid::new_v4().to_string();
        tokens.splice(
            (claude_idx + 1)..=claude_idx,
            [
                "--session-id".to_owned(),
                sid.clone(),
                "--settings".to_owned(),
                settings_arg,
            ],
        );
        Some(sid)
    };

    Ok(SpawnPlan {
        command: shell_words::join(&tokens),
        env: Vec::new(),
        files: vec![settings_path],
        harness_session_id,
    })
}

/// Build the `--settings` JSON that wires every lifecycle hook of interest to
/// `<pulpo_bin> hook claude`.
fn build_settings_json(pulpo_bin: &str) -> String {
    // Claude Code runs a `command`-type hook through a shell, so a pulpo install
    // path containing a space (or any other shell metacharacter) must be quoted —
    // otherwise the hook command splits on the space and Claude Code either fails to
    // find the binary or invokes the wrong thing entirely.
    let command = format!("{} hook claude", shell_words::quote(pulpo_bin));
    let hook = |matcher: Option<&str>| -> Value {
        let mut entry = serde_json::json!({
            "hooks": [{"type": "command", "command": command, "timeout": 5}],
        });
        if let Some(matcher) = matcher {
            entry["matcher"] = Value::String(matcher.to_owned());
        }
        entry
    };
    let settings = serde_json::json!({
        "hooks": {
            "SessionStart": [hook(None)],
            "UserPromptSubmit": [hook(None)],
            "Stop": [hook(None)],
            "StopFailure": [hook(None)],
            "SessionEnd": [hook(None)],
            "Notification": [hook(Some(NOTIFICATION_MATCHER))],
        }
    });
    serde_json::to_string_pretty(&settings).unwrap_or_else(|_| "{}".to_owned())
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
    fn test_matches_claude_only() {
        assert!(ClaudeAdapter.matches("claude"));
        assert!(!ClaudeAdapter.matches("codex"));
        assert!(!ClaudeAdapter.matches(""));
    }

    #[test]
    fn test_id_and_emits_events() {
        assert_eq!(ClaudeAdapter.id(), "claude");
        assert!(ClaudeAdapter.emits_events());
    }

    // -- prepare_spawn: rewrite path --

    #[test]
    fn test_prepare_spawn_rewrites_plain_claude() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "claude -p 'fix the bug'"))
            .unwrap();

        assert!(plan.harness_session_id.is_some());
        let sid = plan.harness_session_id.clone().unwrap();
        assert!(plan.command.contains(&format!("--session-id {sid}")));
        assert!(plan.command.contains("--settings"));
        assert!(plan.command.ends_with("'fix the bug'"));
        assert_eq!(plan.files.len(), 1);
        assert!(plan.files[0].exists());

        let written = std::fs::read_to_string(&plan.files[0]).unwrap();
        let json: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(
            json["hooks"]["Stop"][0]["hooks"][0]["command"],
            "pulpo hook claude"
        );
        assert_eq!(
            json["hooks"]["Notification"][0]["matcher"],
            NOTIFICATION_MATCHER
        );
    }

    #[test]
    fn test_prepare_spawn_inserts_flags_right_after_argv0() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "claude --model opus"))
            .unwrap();
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(tokens[0], "claude");
        assert_eq!(tokens[1], "--session-id");
        assert_eq!(tokens[3], "--settings");
        assert_eq!(&tokens[5..], ["--model", "opus"]);
    }

    #[test]
    fn test_prepare_spawn_handles_absolute_path_argv0() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "/usr/local/bin/claude -p hi"))
            .unwrap();
        assert!(plan.harness_session_id.is_some());
        assert!(
            plan.command
                .starts_with("/usr/local/bin/claude --session-id")
        );
    }

    #[test]
    fn test_prepare_spawn_handles_env_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "env FOO=bar claude -p hi"))
            .unwrap();
        assert!(plan.harness_session_id.is_some());
        // shell_words::join re-quotes "FOO=bar" defensively; assert on the parsed
        // tokens (logical correctness) rather than the exact byte string.
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(tokens[0], "env");
        assert_eq!(tokens[1], "FOO=bar");
        assert_eq!(tokens[2], "claude");
        assert_eq!(tokens[3], "--session-id");
    }

    #[test]
    fn test_prepare_spawn_writes_under_harness_dir_for_session_id() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "claude"))
            .unwrap();
        let expected_dir = tmp
            .path()
            .join("harness")
            .join("11111111-1111-1111-1111-111111111111");
        assert_eq!(plan.files[0].parent().unwrap(), expected_dir);
    }

    // -- prepare_spawn: no-op paths --

    #[test]
    fn test_prepare_spawn_resume_flag_reinjects_settings_only() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "claude --resume abc-123"))
            .unwrap();
        // No new harness_session_id minted — the caller already knows it (abc-123).
        assert!(plan.harness_session_id.is_none());
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(tokens[0], "claude");
        assert_eq!(tokens[1], "--settings");
        assert!(!tokens.contains(&"--session-id".to_owned()));
        assert_eq!(&tokens[3..], ["--resume", "abc-123"]);
        assert_eq!(plan.files.len(), 1);
        assert!(plan.files[0].exists());
    }

    #[test]
    fn test_prepare_spawn_short_resume_flag_reinjects_settings_only() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "claude -r abc-123"))
            .unwrap();
        assert!(plan.harness_session_id.is_none());
        let tokens = shell_words::split(&plan.command).unwrap();
        assert!(tokens.contains(&"--settings".to_owned()));
        assert!(!tokens.contains(&"--session-id".to_owned()));
    }

    #[test]
    fn test_prepare_spawn_continue_flag_reinjects_settings_only() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "claude --continue"))
            .unwrap();
        assert!(plan.harness_session_id.is_none());
        let tokens = shell_words::split(&plan.command).unwrap();
        assert!(tokens.contains(&"--settings".to_owned()));
        assert!(!tokens.contains(&"--session-id".to_owned()));
    }

    #[test]
    fn test_prepare_spawn_short_continue_flag_reinjects_settings_only() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "claude -c"))
            .unwrap();
        assert!(plan.harness_session_id.is_none());
        let tokens = shell_words::split(&plan.command).unwrap();
        assert!(tokens.contains(&"--settings".to_owned()));
        assert!(!tokens.contains(&"--session-id".to_owned()));
    }

    #[test]
    fn test_prepare_spawn_skips_when_session_id_flag_present() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "claude --session-id my-id"))
            .unwrap();
        assert_eq!(plan.command, "claude --session-id my-id");
    }

    #[test]
    fn test_prepare_spawn_skips_when_settings_flag_present() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "claude --settings /tmp/x.json"))
            .unwrap();
        assert_eq!(plan.command, "claude --settings /tmp/x.json");
    }

    #[test]
    fn test_prepare_spawn_falls_back_on_unparseable_command() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(tmp.path(), "claude \"unterminated"))
            .unwrap();
        assert_eq!(plan.command, "claude \"unterminated");
        assert!(plan.harness_session_id.is_none());
    }

    #[test]
    fn test_prepare_spawn_falls_back_when_data_dir_cannot_be_created() {
        // Point data_dir at a path whose parent is a file, not a directory —
        // create_dir_all must fail, and prepare_spawn must fall back cleanly.
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let plan = ClaudeAdapter
            .prepare_spawn(&ctx(&blocker, "claude -p hi"))
            .unwrap();
        assert_eq!(plan.command, "claude -p hi");
        assert!(plan.harness_session_id.is_none());
        assert!(plan.files.is_empty());
    }

    // -- resume_command --

    #[test]
    fn test_resume_command_plain() {
        let cmd = ClaudeAdapter
            .resume_command("claude -p hi", "sid-1")
            .unwrap();
        assert_eq!(cmd, "claude --resume sid-1 -p hi");
    }

    #[test]
    fn test_resume_command_strips_pulpo_inserted_flags() {
        let original = "claude --session-id old-sid --settings /tmp/x.json -p hi";
        let cmd = ClaudeAdapter.resume_command(original, "sid-new").unwrap();
        assert_eq!(cmd, "claude --resume sid-new -p hi");
    }

    #[test]
    fn test_resume_command_strips_equals_form_flags() {
        let original = "claude --session-id=old-sid --settings=/tmp/x.json -p hi";
        let cmd = ClaudeAdapter.resume_command(original, "sid-new").unwrap();
        assert_eq!(cmd, "claude --resume sid-new -p hi");
    }

    #[test]
    fn test_resume_command_preserves_env_prefix() {
        let cmd = ClaudeAdapter
            .resume_command("env FOO=bar claude -p hi", "sid-1")
            .unwrap();
        let tokens = shell_words::split(&cmd).unwrap();
        assert_eq!(
            tokens,
            ["env", "FOO=bar", "claude", "--resume", "sid-1", "-p", "hi"]
        );
    }

    #[test]
    fn test_resume_command_strips_existing_continue_flag() {
        let cmd = ClaudeAdapter
            .resume_command("claude --continue", "sid-new")
            .unwrap();
        assert_eq!(cmd, "claude --resume sid-new");
    }

    #[test]
    fn test_resume_command_strips_existing_short_continue_flag() {
        let cmd = ClaudeAdapter
            .resume_command("claude -c", "sid-new")
            .unwrap();
        assert_eq!(cmd, "claude --resume sid-new");
    }

    #[test]
    fn test_resume_command_strips_existing_resume_flag_and_value() {
        let cmd = ClaudeAdapter
            .resume_command("claude --resume X", "sid-new")
            .unwrap();
        assert_eq!(cmd, "claude --resume sid-new");
    }

    #[test]
    fn test_resume_command_strips_existing_short_resume_flag_and_value() {
        let cmd = ClaudeAdapter
            .resume_command("claude -r X", "sid-new")
            .unwrap();
        assert_eq!(cmd, "claude --resume sid-new");
    }

    #[test]
    fn test_resume_command_strips_existing_resume_equals_form() {
        let cmd = ClaudeAdapter
            .resume_command("claude --resume=X -p hi", "sid-new")
            .unwrap();
        assert_eq!(cmd, "claude --resume sid-new -p hi");
    }

    #[test]
    fn test_resume_command_strips_bare_resume_flag_no_value() {
        // `--resume` with no value at all (e.g. the user meant to trigger Claude's
        // interactive picker) followed directly by another flag — only the bare flag
        // is stripped, the following flag is left alone.
        let cmd = ClaudeAdapter
            .resume_command("claude --resume -p hi", "sid-new")
            .unwrap();
        assert_eq!(cmd, "claude --resume sid-new -p hi");
    }

    #[test]
    fn test_resume_command_none_when_not_claude() {
        assert!(ClaudeAdapter.resume_command("bash", "sid-1").is_none());
    }

    #[test]
    fn test_resume_command_none_when_unparseable() {
        assert!(
            ClaudeAdapter
                .resume_command("claude \"unterminated", "sid-1")
                .is_none()
        );
    }

    // -- parse_event --

    #[test]
    fn test_parse_event_session_start() {
        let raw = serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": "sid-1",
            "source": "startup",
        });
        let event = ClaudeAdapter.parse_event(&raw).unwrap().unwrap();
        assert_eq!(
            event,
            HarnessEvent::SessionStarted {
                harness_session_id: Some("sid-1".into()),
                resumed: false,
            }
        );
    }

    #[test]
    fn test_parse_event_session_start_resumed() {
        let raw = serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": "sid-1",
            "source": "resume",
        });
        let event = ClaudeAdapter.parse_event(&raw).unwrap().unwrap();
        assert_eq!(
            event,
            HarnessEvent::SessionStarted {
                harness_session_id: Some("sid-1".into()),
                resumed: true,
            }
        );
    }

    #[test]
    fn test_parse_event_user_prompt_submit_is_working() {
        let raw = serde_json::json!({"hook_event_name": "UserPromptSubmit"});
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::Working
        );
    }

    #[test]
    fn test_parse_event_stop_turn_finished_with_summary() {
        let raw = serde_json::json!({
            "hook_event_name": "Stop",
            "last_assistant_message": "Fixed the bug",
        });
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::TurnFinished {
                summary: Some("Fixed the bug".into())
            }
        );
    }

    #[test]
    fn test_parse_event_stop_falls_back_to_message_field() {
        let raw = serde_json::json!({"hook_event_name": "Stop", "message": "Done"});
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::TurnFinished {
                summary: Some("Done".into())
            }
        );
    }

    #[test]
    fn test_parse_event_stop_no_summary() {
        let raw = serde_json::json!({"hook_event_name": "Stop"});
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::TurnFinished { summary: None }
        );
    }

    #[test]
    fn test_parse_event_stop_failure_rate_limited() {
        for error_type in ["rate_limit_error", "http_429", "overloaded_error"] {
            let raw = serde_json::json!({
                "hook_event_name": "StopFailure",
                "error_type": error_type,
            });
            let event = ClaudeAdapter.parse_event(&raw).unwrap().unwrap();
            assert_eq!(
                event,
                HarnessEvent::Failed {
                    error: error_type.into(),
                    rate_limited: true,
                }
            );
        }
    }

    #[test]
    fn test_parse_event_stop_failure_not_rate_limited() {
        let raw = serde_json::json!({
            "hook_event_name": "StopFailure",
            "error_type": "api_error",
        });
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::Failed {
                error: "api_error".into(),
                rate_limited: false,
            }
        );
    }

    #[test]
    fn test_parse_event_stop_failure_missing_error_type_uses_fallback_and_error_field() {
        let raw = serde_json::json!({"hook_event_name": "StopFailure", "error": "boom"});
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::Failed {
                error: "boom".into(),
                rate_limited: false,
            }
        );

        let raw = serde_json::json!({"hook_event_name": "StopFailure"});
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::Failed {
                error: "unknown error".into(),
                rate_limited: false,
            }
        );
    }

    #[test]
    fn test_parse_event_session_end() {
        let raw = serde_json::json!({"hook_event_name": "SessionEnd", "reason": "exit"});
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::SessionEnded {
                reason: Some("exit".into())
            }
        );
    }

    #[test]
    fn test_parse_event_session_end_no_reason() {
        let raw = serde_json::json!({"hook_event_name": "SessionEnd"});
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::SessionEnded { reason: None }
        );
    }

    #[test]
    fn test_parse_event_session_end_clear_reason_is_none() {
        // `/clear` ends the conversation in-process but the harness keeps running —
        // must not be reported as the process exiting.
        let raw = serde_json::json!({"hook_event_name": "SessionEnd", "reason": "clear"});
        assert!(ClaudeAdapter.parse_event(&raw).unwrap().is_none());
    }

    #[test]
    fn test_parse_event_session_end_resume_reason_is_none() {
        // `/resume` likewise replaces the in-process conversation without the harness
        // process exiting.
        let raw = serde_json::json!({"hook_event_name": "SessionEnd", "reason": "resume"});
        assert!(ClaudeAdapter.parse_event(&raw).unwrap().is_none());
    }

    #[test]
    fn test_parse_event_session_end_other_reasons_still_end_session() {
        for reason in ["logout", "prompt_input_exit", "other"] {
            let raw = serde_json::json!({"hook_event_name": "SessionEnd", "reason": reason});
            assert_eq!(
                ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
                HarnessEvent::SessionEnded {
                    reason: Some(reason.into())
                }
            );
        }
    }

    #[test]
    fn test_parse_event_notification_permission() {
        let raw = serde_json::json!({
            "hook_event_name": "Notification",
            "matcher": "permission_prompt",
        });
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Permission
            }
        );
    }

    #[test]
    fn test_parse_event_notification_idle() {
        let raw = serde_json::json!({
            "hook_event_name": "Notification",
            "notification_type": "idle_prompt",
        });
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Idle
            }
        );
    }

    #[test]
    fn test_parse_event_notification_question_variants() {
        for tag in [
            "agent_needs_input",
            "elicitation_dialog",
            "elicitation_url_dialog",
        ] {
            let raw = serde_json::json!({"hook_event_name": "Notification", "type": tag});
            assert_eq!(
                ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
                HarnessEvent::NeedsInput {
                    reason: NeedsInputReason::Question
                }
            );
        }
    }

    #[test]
    fn test_parse_event_notification_unknown_tag_is_other() {
        let raw = serde_json::json!({"hook_event_name": "Notification", "matcher": "auth_success"});
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Other("auth_success".into())
            }
        );
    }

    #[test]
    fn test_parse_event_notification_falls_back_to_message_keyword() {
        let raw = serde_json::json!({
            "hook_event_name": "Notification",
            "message": "Claude needs your permission to use Bash",
        });
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Permission
            }
        );
    }

    #[test]
    fn test_parse_event_notification_message_idle_keyword() {
        let raw = serde_json::json!({
            "hook_event_name": "Notification",
            "message": "Session has been idle for a while",
        });
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Idle
            }
        );
    }

    #[test]
    fn test_parse_event_notification_message_question_keywords() {
        for message in [
            "Claude has a question for you",
            "Waiting for your input",
            "elicitation requested",
        ] {
            let raw = serde_json::json!({"hook_event_name": "Notification", "message": message});
            assert_eq!(
                ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
                HarnessEvent::NeedsInput {
                    reason: NeedsInputReason::Question
                }
            );
        }
    }

    #[test]
    fn test_parse_event_notification_unrecognized_message_is_other() {
        let raw = serde_json::json!({
            "hook_event_name": "Notification",
            "message": "Something happened",
        });
        assert_eq!(
            ClaudeAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Other("something happened".into())
            }
        );
    }

    #[test]
    fn test_parse_event_notification_nothing_to_classify_is_none() {
        let raw = serde_json::json!({"hook_event_name": "Notification"});
        assert!(ClaudeAdapter.parse_event(&raw).unwrap().is_none());
    }

    #[test]
    fn test_parse_event_unknown_hook_is_none() {
        let raw = serde_json::json!({"hook_event_name": "PreToolUse"});
        assert!(ClaudeAdapter.parse_event(&raw).unwrap().is_none());
    }

    #[test]
    fn test_parse_event_missing_hook_event_name_is_none() {
        let raw = serde_json::json!({});
        assert!(ClaudeAdapter.parse_event(&raw).unwrap().is_none());
    }

    // -- small helpers --

    #[test]
    fn test_resolve_pulpo_bin_from_prefers_sibling_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let pulpo_path = tmp.path().join("pulpo");
        std::fs::write(&pulpo_path, b"#!/bin/sh").unwrap();
        let pulpod_path = tmp.path().join("pulpod");
        assert_eq!(
            resolve_pulpo_bin_from(Some(&pulpod_path)),
            pulpo_path.to_string_lossy()
        );
    }

    #[test]
    fn test_resolve_pulpo_bin_from_falls_back_when_sibling_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let pulpod_path = tmp.path().join("pulpod");
        assert_eq!(resolve_pulpo_bin_from(Some(&pulpod_path)), "pulpo");
    }

    #[test]
    fn test_resolve_pulpo_bin_from_none_falls_back() {
        assert_eq!(resolve_pulpo_bin_from(None), "pulpo");
    }

    #[test]
    fn test_build_settings_json_wires_every_hook() {
        let json_str = build_settings_json("/opt/pulpo/pulpo");
        let json: Value = serde_json::from_str(&json_str).unwrap();
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "Stop",
            "StopFailure",
            "SessionEnd",
        ] {
            assert_eq!(
                json["hooks"][event][0]["hooks"][0]["command"],
                "/opt/pulpo/pulpo hook claude"
            );
            assert_eq!(json["hooks"][event][0]["hooks"][0]["timeout"], 5);
        }
        assert_eq!(
            json["hooks"]["Notification"][0]["matcher"],
            NOTIFICATION_MATCHER
        );
    }

    #[test]
    fn test_build_settings_json_quotes_pulpo_bin_with_space() {
        // The hook command is run through a shell — an unquoted path containing a
        // space would split into two argv elements and Claude Code would fail to
        // invoke the hook (or invoke the wrong binary).
        let json_str = build_settings_json("/opt/my pulpo/pulpo");
        let json: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(
            json["hooks"]["Stop"][0]["hooks"][0]["command"],
            "'/opt/my pulpo/pulpo' hook claude"
        );
    }

    #[test]
    fn test_has_flag_detects_equals_form() {
        let tokens = vec!["claude".to_owned(), "--session-id=abc".to_owned()];
        assert!(has_flag(&tokens, IDENTITY_FLAGS));
    }

    #[test]
    fn test_has_flag_false_for_clean_command() {
        let tokens = vec!["claude".to_owned(), "-p".to_owned(), "hi".to_owned()];
        assert!(!has_flag(&tokens, IDENTITY_FLAGS));
        assert!(!has_flag(&tokens, RESUME_FLAGS));
    }

    #[test]
    fn test_has_flag_detects_resume_flags() {
        assert!(has_flag(&["--resume".to_owned()], RESUME_FLAGS));
        assert!(has_flag(&["-r".to_owned()], RESUME_FLAGS));
        assert!(has_flag(&["--continue".to_owned()], RESUME_FLAGS));
        assert!(has_flag(&["-c".to_owned()], RESUME_FLAGS));
    }

    #[test]
    fn test_claude_token_index_not_found() {
        let tokens = vec!["bash".to_owned()];
        assert!(claude_token_index(&tokens).is_none());
    }

    #[test]
    fn test_claude_token_index_env_with_no_command_after() {
        let tokens = vec!["env".to_owned(), "FOO=bar".to_owned()];
        assert!(claude_token_index(&tokens).is_none());
    }
}
