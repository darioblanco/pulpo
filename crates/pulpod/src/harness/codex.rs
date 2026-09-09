//! Codex CLI adapter — isolated `CODEX_HOME` + `--dangerously-bypass-hook-trust`.
//!
//! Codex has no `--settings`-equivalent flag and no confirmed way to inject a
//! `[hooks]` table via `-c`, so this adapter mirrors the Claude adapter's
//! isolated-file approach at one remove: instead of writing one settings file,
//! it redirects the whole `CODEX_HOME` (which governs `config.toml`, `auth.json`,
//! `history.jsonl`, and the `sessions/` directory together) to a per-session
//! directory under `<data_dir>/harness/<session_id>/codex-home/`, seeds it with a
//! copy of the user's real `auth.json` (skipped silently when absent) and a
//! `config.toml` built from the user's real one plus pulpo's own `notify`/`hooks`
//! entries, and rewrites the command to add `--dangerously-bypass-hook-trust` —
//! without it, the first hook fires a blocking "Hooks need review" TUI prompt pulpo
//! can't answer.
//!
//! Verified against Codex CLI 0.153.0 docs/source (see
//! `docs/architecture/harness-adapters.md` for the full source list).
//! `--dangerously-bypass-hook-trust` was broken for the interactive TUI in
//! 0.131.0–0.133.0 (issue #24093) but fixed by PR #24317 — safe to rely on at
//! 0.153.0 or newer.
//!
//! UNVERIFIED / deviation from the literal research spec: Codex's own hook JSON
//! payload has no confirmed field naming which event fired (unlike Claude's
//! `hook_event_name`). Rather than guess a field name, each hook command below
//! passes `--event <Name>` — the same disambiguation mechanism
//! `pulpo hook <harness> --event <Name>` already offers generically (see
//! `pulpo-cli/src/hook.rs::build_hook_body`), which fills in `hook_event_name` when
//! the payload doesn't already carry one. This means the `command` lines pulpo
//! writes differ from the research spec's literal `"<pulpo-bin> hook codex"` — the
//! resulting behavior (a distinct, correctly-typed event per hook) is what the spec
//! actually calls for.
//!
//! Also UNVERIFIED: the exact field spelling in Codex's `notify` payload for the
//! session/thread id and the assistant's turn summary. The mapping table in the
//! spec lists `thread_id`/`last_assistant_message` (`snake_case`), but Codex's other
//! documented notify fields are known to use `kebab-case` elsewhere, so both
//! [`parse_event`](CodexAdapter::parse_event) and the CLI's
//! `execute_codex_notify_hook` check both spellings.
//!
//! Deviation from the literal spec (correctness fix): the spec describes writing
//! the user's real `config.toml` first, then appending pulpo's `notify`/`hooks`
//! keys. Appending a bare `notify = [...]` key *after* arbitrary existing content
//! is unsafe — TOML scopes a bare key to whichever `[table]` most recently appeared
//! above it, so if the user's file ends inside (or consists entirely of) one or
//! more tables (e.g. `[mcp_servers.demo]`), the appended `notify` would be silently
//! absorbed into that table instead of landing at the document root, corrupting the
//! merge (proven by `test_build_config_toml_is_valid_toml_with_expected_shape`,
//! which fails against the literal ordering). [`build_config_toml`] instead emits
//! `notify` *before* the user's content — always root-scoped — and keeps the
//! `[[hooks.<Event>]]` array-of-tables after it (position-independent: a bracketed
//! header always names its full path from the document root). A pre-existing
//! `notify` key in the user's own config would still collide (Codex's config format
//! has only one `notify` slot) — an accepted limitation, since a `CODEX_HOME`-wide
//! hook takeover is inherently exclusive with the user's own `notify` setup.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;
use tracing::{info, warn};

use super::{
    HarnessAdapter, HarnessEvent, HarnessSignals, NeedsInputReason, SpawnContext, SpawnPlan,
};

/// Tokens that unconditionally mean pulpo must not rewrite this spawn: the command
/// already carries the bypass flag (a rewrite already fully happened), or already
/// pins its own `CODEX_HOME` (redirecting it again would conflict).
fn already_bypassed_or_pinned(tokens: &[String]) -> bool {
    tokens
        .iter()
        .any(|token| token == "--dangerously-bypass-hook-trust" || token.starts_with("CODEX_HOME="))
}

/// True if `tokens` already contains a `resume` subcommand.
///
/// Ambiguous on its own: it's produced both by a user directly resuming a session
/// pulpo doesn't manage (redirecting `CODEX_HOME` would break it — the target
/// wouldn't exist under a fresh isolated dir) *and* by pulpo's own
/// [`CodexAdapter::resume_command`] resuming a session pulpo *does* manage (whose
/// rollout files live under the isolated `CODEX_HOME` pulpo must keep using). See
/// [`CodexAdapter::prepare_spawn`] for how the two are told apart.
fn contains_resume(tokens: &[String]) -> bool {
    tokens.iter().any(|token| token == "resume")
}

/// Index of the `codex` token in `tokens`, skipping a leading `env` invocation and
/// its assignments/flags — the same convention [`super::registry`]'s basename
/// resolution and the Claude adapter use.
fn codex_token_index(tokens: &[String]) -> Option<usize> {
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
    (Path::new(token).file_name().and_then(|f| f.to_str()) == Some("codex")).then_some(idx)
}

/// Remove an existing `resume <target>` pair right after the `codex` token, if
/// present (`<target>` is either a session id or `--last`).
fn strip_existing_resume(tokens: &mut Vec<String>, codex_idx: usize) {
    if tokens.get(codex_idx + 1).map(String::as_str) != Some("resume") {
        return;
    }
    tokens.remove(codex_idx + 1);
    if codex_idx + 1 < tokens.len() {
        tokens.remove(codex_idx + 1);
    }
}

/// Resolve the absolute path of the running `pulpo` binary — the sibling of the
/// running `pulpod` binary — falling back to the plain `pulpo` (resolved via
/// `$PATH` at hook-invocation time) when that sibling doesn't exist. Duplicated
/// from `claude.rs` rather than shared: each adapter is self-contained, and the
/// logic is a handful of lines.
fn resolve_pulpo_bin() -> String {
    resolve_pulpo_bin_from(std::env::current_exe().ok().as_deref())
}

fn resolve_pulpo_bin_from(current_exe: Option<&Path>) -> String {
    current_exe
        .and_then(Path::parent)
        .map(|dir| dir.join("pulpo"))
        .filter(|candidate| candidate.is_file())
        .map_or_else(
            || "pulpo".to_owned(),
            |candidate| candidate.to_string_lossy().into_owned(),
        )
}

/// The real Codex home pulpo copies `auth.json`/`config.toml` from: `$CODEX_HOME`
/// when pulpod's own process has it set, else `~/.codex`.
fn source_codex_home() -> Option<PathBuf> {
    source_codex_home_from(std::env::var("CODEX_HOME").ok(), dirs::home_dir())
}

/// Testable core of [`source_codex_home`]: given the (possibly absent) `CODEX_HOME`
/// env value and home dir explicitly, resolve which one wins. Kept separate so
/// tests never have to mutate the real process environment (`set_var`/`remove_var`
/// require `unsafe`, forbidden workspace-wide, and would race with any other test
/// in this binary reading/writing the same process-wide state concurrently).
fn source_codex_home_from(
    codex_home_env: Option<String>,
    home_dir: Option<PathBuf>,
) -> Option<PathBuf> {
    codex_home_env
        .map(PathBuf::from)
        .or_else(|| home_dir.map(|home| home.join(".codex")))
}

/// Escape a value for embedding inside a TOML basic (double-quoted) string.
fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The `notify` line: Codex delivers its payload as a trailing argv element, not
/// stdin, so this wraps `pulpo hook codex-notify` in `sh -c '... "$0"'` to turn
/// that trailing argument into `$0` (see `pulpo-cli/src/hook.rs::execute_codex_notify_hook`).
fn notify_line(pulpo_bin: &str) -> String {
    // Escape a literal single quote in the path for the shell's single-quoted
    // segment first, then escape the result for the outer TOML string.
    let shell_escaped = pulpo_bin.replace('\'', r"'\''");
    let toml_value = toml_escape(&shell_escaped);
    format!("notify = [\"sh\", \"-c\", \"'{toml_value}' hook codex-notify \\\"$0\\\"\"]\n")
}

/// One `[[hooks.<Event>]]` table wiring `<Event>` to `pulpo hook codex --event <Event>`.
fn hook_table(event: &str, pulpo_bin: &str, timeout: u32) -> String {
    let command = toml_escape(&format!("{pulpo_bin} hook codex --event {event}"));
    format!(
        "\n[[hooks.{event}]]\n[[hooks.{event}.hooks]]\ntype = \"command\"\ncommand = \"{command}\"\ntimeout = {timeout}\n"
    )
}

/// Hooks pulpo wires, in the order written, with their timeout in seconds.
/// `SessionEnd` gets 2s — Codex caps `SessionEnd`/`Interrupt` hooks at 3s.
const HOOK_EVENTS: &[(&str, u32)] = &[
    ("SessionStart", 5),
    ("UserPromptSubmit", 5),
    ("Stop", 5),
    ("SessionEnd", 2),
    ("PermissionRequest", 5),
];

/// Build the full `config.toml` contents: the user's real config (if any) verbatim,
/// followed by pulpo's `notify` line and hook tables.
///
/// Deviation from the literal research spec (which reads "copy the user's config in
/// first, then append the notify/hooks keys"): `notify` is emitted *before* the
/// user's content instead. TOML scopes a bare `key = value` to whichever `[table]`
/// most recently appeared above it in the file — if the user's config ends inside,
/// or consists entirely of, one or more tables (e.g. `[mcp_servers.demo]`),
/// appending a bare `notify = [...]` after it would be silently absorbed into that
/// table instead of landing at the document root where Codex expects it, corrupting
/// the merge. `[[hooks.<Event>]]` array-of-tables don't have this problem — a
/// bracketed header always names its full path from the document root regardless of
/// what came before — so those stay appended after, per the spec.
fn build_config_toml(existing: &str, pulpo_bin: &str) -> String {
    let mut out = notify_line(pulpo_bin);
    let trimmed = existing.trim_end();
    if !trimmed.is_empty() {
        out.push('\n');
        out.push_str(trimmed);
        out.push('\n');
    }
    for (event, timeout) in HOOK_EVENTS {
        out.push_str(&hook_table(event, pulpo_bin, *timeout));
    }
    out
}

/// Best-effort copy of the real `auth.json` into the isolated `codex_home`. Skips
/// silently when the source is unknown or the file doesn't exist; a copy failure
/// only warns — losing auth is recoverable (Codex just prompts to log in) and must
/// never abort the whole spawn rewrite.
fn seed_auth_json(source_dir: Option<&Path>, codex_home: &Path) {
    let Some(source_dir) = source_dir else {
        return;
    };
    let source_auth = source_dir.join("auth.json");
    if !source_auth.is_file() {
        return;
    }
    if let Err(error) = std::fs::copy(&source_auth, codex_home.join("auth.json")) {
        warn!(%error, "codex adapter: failed to copy auth.json, spawning without it");
    }
}

/// Write `<codex_home>/config.toml`, merging in the user's real one when present.
fn write_config_toml(
    codex_home: &Path,
    source_dir: Option<&Path>,
    pulpo_bin: &str,
) -> Result<PathBuf> {
    let existing = source_dir
        .map(|dir| dir.join("config.toml"))
        .filter(|path| path.is_file())
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();

    let content = build_config_toml(&existing, pulpo_bin);
    let path = codex_home.join("config.toml");
    std::fs::write(&path, content)?;
    Ok(path)
}

/// The fallible part of [`CodexAdapter::prepare_spawn`]: create/reuse the isolated
/// `codex-home` dir, seed it, and splice in the bypass flag. Isolated so the caller
/// can catch any I/O error and fall back to the unchanged command.
///
/// Reuses the same `<data_dir>/harness/<session_id>/codex-home` directory on every
/// call (spawn *and* resume) — `create_dir_all` is a no-op when it already exists,
/// so a resume's rollout files (which live under this same `CODEX_HOME`) are never
/// wiped.
fn rewrite_spawn(
    ctx: &SpawnContext,
    mut tokens: Vec<String>,
    codex_idx: usize,
) -> Result<SpawnPlan> {
    let codex_home = ctx
        .data_dir
        .join("harness")
        .join(ctx.session_id)
        .join("codex-home");
    std::fs::create_dir_all(&codex_home)?;

    let pulpo_bin = resolve_pulpo_bin();
    let source_dir = source_codex_home();
    seed_auth_json(source_dir.as_deref(), &codex_home);
    let config_path = write_config_toml(&codex_home, source_dir.as_deref(), &pulpo_bin)?;

    tokens.splice(
        (codex_idx + 1)..=codex_idx,
        ["--dangerously-bypass-hook-trust".to_owned()],
    );

    Ok(SpawnPlan {
        command: shell_words::join(&tokens),
        env: vec![(
            "CODEX_HOME".to_owned(),
            codex_home.to_string_lossy().into_owned(),
        )],
        files: vec![config_path],
        // Codex has no flag to preset a session/thread id at launch — only known
        // once `SessionStart` fires (or via disk discovery on resume).
        harness_session_id: None,
    })
}

pub struct CodexAdapter;

impl HarnessAdapter for CodexAdapter {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn matches(&self, argv0: &str) -> bool {
        argv0 == "codex"
    }

    fn prepare_spawn(&self, ctx: &SpawnContext) -> Result<SpawnPlan> {
        let Ok(tokens) = shell_words::split(ctx.command) else {
            warn!(
                session = %ctx.session_name,
                "codex adapter: command did not parse as shell words, spawning unchanged"
            );
            return Ok(SpawnPlan::unchanged(ctx.command));
        };

        let Some(codex_idx) = codex_token_index(&tokens) else {
            return Ok(SpawnPlan::unchanged(ctx.command));
        };

        if already_bypassed_or_pinned(&tokens) {
            info!(
                session = %ctx.session_name,
                "codex adapter: command already bypasses hook trust or pins CODEX_HOME, spawning unchanged"
            );
            return Ok(SpawnPlan::unchanged(ctx.command));
        }

        // A `resume` subcommand is ambiguous (see `contains_resume`'s doc comment):
        // only proceed when this session's own isolated `codex-home` dir already
        // exists — i.e. this is pulpo's own `resume_command` output being
        // reprocessed, whose rollout files live under that same dir — never for a
        // user's own resume of a session pulpo has never spawned into isolation.
        let codex_home = ctx
            .data_dir
            .join("harness")
            .join(ctx.session_id)
            .join("codex-home");
        if contains_resume(&tokens) && !codex_home.is_dir() {
            info!(
                session = %ctx.session_name,
                "codex adapter: command resumes a session pulpo doesn't manage, spawning unchanged"
            );
            return Ok(SpawnPlan::unchanged(ctx.command));
        }

        match rewrite_spawn(ctx, tokens, codex_idx) {
            Ok(plan) => Ok(plan),
            Err(error) => {
                warn!(
                    session = %ctx.session_name,
                    %error,
                    "codex adapter: failed to prepare spawn, spawning unchanged"
                );
                Ok(SpawnPlan::unchanged(ctx.command))
            }
        }
    }

    fn resume_command(&self, original_command: &str, harness_session_id: &str) -> Option<String> {
        let mut tokens = shell_words::split(original_command).ok()?;
        let codex_idx = codex_token_index(&tokens)?;
        strip_existing_resume(&mut tokens, codex_idx);
        tokens.splice(
            (codex_idx + 1)..=codex_idx,
            ["resume".to_owned(), harness_session_id.to_owned()],
        );
        Some(shell_words::join(&tokens))
    }

    fn parse_event(&self, raw: &Value) -> Result<Option<HarnessEvent>> {
        if let Some(notify_type) = raw.get("type").and_then(Value::as_str) {
            return Ok(parse_notify_event(raw, notify_type));
        }

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
            "Stop" => Some(HarnessEvent::TurnFinished {
                summary: turn_summary(raw),
            }),
            "SessionEnd" => {
                let reason = raw.get("reason").and_then(Value::as_str).map(str::to_owned);
                Some(HarnessEvent::SessionEnded { reason })
            }
            // No `hookSpecificOutput` is ever emitted here — pulpo stays purely
            // observational and the real TUI approval prompt still runs.
            "PermissionRequest" => Some(HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Permission,
            }),
            // PreToolUse/PostToolUse/PreCompact/PostCompact/SubagentStart/
            // SubagentStop/Interrupt exist but are out of scope for pulpo's state
            // machine today.
            _ => None,
        })
    }

    fn emits_events(&self) -> bool {
        true
    }

    fn owned_signals(&self) -> HarnessSignals {
        // Codex has no error/rate-limit hook or notify event today — keep those two
        // scrollback heuristics running even once lifecycle events are flowing.
        HarnessSignals::lifecycle_only()
    }
}

/// `raw["last_assistant_message"]` (as documented) or `raw["last-assistant-message"]`
/// (Codex's other notify-adjacent fields are known to use kebab-case) — whichever is
/// present.
fn turn_summary(raw: &Value) -> Option<String> {
    raw.get("last_assistant_message")
        .or_else(|| raw.get("last-assistant-message"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// A `notify` payload (identified by its `type` field, e.g. `agent-turn-complete`;
/// hook payloads carry no such field). Only `agent-turn-complete` maps to anything —
/// other notify types Codex may add later are out of scope for pulpo's state
/// machine, same as the hook-only events.
fn parse_notify_event(raw: &Value, notify_type: &str) -> Option<HarnessEvent> {
    match notify_type {
        "agent-turn-complete" => Some(HarnessEvent::TurnFinished {
            summary: turn_summary(raw),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(data_dir: &'a Path, command: &'a str) -> SpawnContext<'a> {
        SpawnContext {
            session_id: "22222222-2222-2222-2222-222222222222",
            session_name: "sess",
            workdir: "/tmp/repo",
            command,
            data_dir,
        }
    }

    // -- matches / id / emits_events / owned_signals --

    #[test]
    fn test_matches_codex_only() {
        assert!(CodexAdapter.matches("codex"));
        assert!(!CodexAdapter.matches("claude"));
        assert!(!CodexAdapter.matches(""));
    }

    #[test]
    fn test_id_emits_events_and_owned_signals() {
        assert_eq!(CodexAdapter.id(), "codex");
        assert!(CodexAdapter.emits_events());
        assert_eq!(
            CodexAdapter.owned_signals(),
            HarnessSignals::lifecycle_only()
        );
    }

    // -- prepare_spawn: rewrite path --
    //
    // These exercise the real `prepare_spawn` end to end, including the real
    // `source_codex_home()`/`dirs::home_dir()` lookup. Assertions only ever check
    // for pulpo's own fixed content (never assert exact-equality of the whole
    // config file), so they stay deterministic regardless of whether the machine
    // running them happens to have a real `~/.codex` — the merge/copy *logic*
    // itself is covered by direct, env-var-free unit tests below
    // (`write_config_toml`, `seed_auth_json`, `source_codex_home_from`).

    #[test]
    fn test_prepare_spawn_rewrites_plain_codex() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "codex -m gpt-5-codex 'fix the bug'"))
            .unwrap();

        assert!(plan.harness_session_id.is_none());
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(tokens[0], "codex");
        assert_eq!(tokens[1], "--dangerously-bypass-hook-trust");
        assert_eq!(&tokens[2..], ["-m", "gpt-5-codex", "fix the bug"]);

        assert_eq!(plan.env.len(), 1);
        assert_eq!(plan.env[0].0, "CODEX_HOME");
        let codex_home = PathBuf::from(&plan.env[0].1);
        assert!(codex_home.ends_with("harness/22222222-2222-2222-2222-222222222222/codex-home"));
        assert!(codex_home.is_dir());

        assert_eq!(plan.files.len(), 1);
        assert!(plan.files[0].exists());
        let config = std::fs::read_to_string(&plan.files[0]).unwrap();
        assert!(config.contains("notify = [\"sh\", \"-c\","));
        assert!(config.contains("hook codex-notify"));
        assert!(config.contains("[[hooks.SessionStart]]"));
        assert!(config.contains("[[hooks.UserPromptSubmit]]"));
        assert!(config.contains("[[hooks.Stop]]"));
        assert!(config.contains("[[hooks.SessionEnd]]"));
        assert!(config.contains("[[hooks.PermissionRequest]]"));
        assert!(config.contains("hook codex --event SessionStart"));
        assert!(config.contains("timeout = 2"));
    }

    #[test]
    fn test_prepare_spawn_handles_absolute_path_argv0() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "/usr/local/bin/codex exec 'fix'"))
            .unwrap();
        assert!(
            plan.command
                .starts_with("/usr/local/bin/codex --dangerously-bypass-hook-trust")
        );
    }

    #[test]
    fn test_prepare_spawn_handles_env_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "env FOO=bar codex exec 'fix'"))
            .unwrap();
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(tokens[0], "env");
        assert_eq!(tokens[1], "FOO=bar");
        assert_eq!(tokens[2], "codex");
        assert_eq!(tokens[3], "--dangerously-bypass-hook-trust");
    }

    #[test]
    fn test_prepare_spawn_writes_under_harness_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "codex"))
            .unwrap();
        let expected_dir = tmp
            .path()
            .join("harness")
            .join("22222222-2222-2222-2222-222222222222")
            .join("codex-home");
        assert_eq!(plan.files[0].parent().unwrap(), expected_dir);
    }

    #[test]
    fn test_prepare_spawn_reuses_same_codex_home_dir_across_calls() {
        // Simulates spawn followed by resume: `prepare_spawn` must not wipe an
        // existing codex-home dir (Codex's rollout files live under it).
        let tmp = tempfile::tempdir().unwrap();
        let first = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "codex -p hi"))
            .unwrap();
        let codex_home = PathBuf::from(&first.env[0].1);
        let marker = codex_home.join("sessions").join("marker.jsonl");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "{}").unwrap();

        let resumed_command = CodexAdapter.resume_command("codex -p hi", "sid-1").unwrap();
        let second = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), &resumed_command))
            .unwrap();
        let codex_home_again = PathBuf::from(&second.env[0].1);

        assert_eq!(codex_home, codex_home_again);
        assert!(marker.exists(), "resume must not wipe the codex-home dir");
        let tokens = shell_words::split(&second.command).unwrap();
        assert_eq!(tokens[0], "codex");
        assert_eq!(tokens[1], "--dangerously-bypass-hook-trust");
        assert!(tokens.contains(&"resume".to_owned()));
        assert!(tokens.contains(&"sid-1".to_owned()));
    }

    // -- prepare_spawn: no-op paths --

    #[test]
    fn test_prepare_spawn_skips_when_resume_present_and_no_isolated_dir_yet() {
        // A user's own `codex resume <id>` on a session pulpo has never isolated
        // into its own CODEX_HOME — redirecting it would break the resume (the
        // target wouldn't exist under a fresh, empty isolated dir).
        let tmp = tempfile::tempdir().unwrap();
        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "codex resume abc-123"))
            .unwrap();
        assert_eq!(plan.command, "codex resume abc-123");
        assert!(plan.env.is_empty());
    }

    #[test]
    fn test_prepare_spawn_proceeds_when_resume_present_and_isolated_dir_already_exists() {
        // The counterpart to the no-op above: once this session's own isolated
        // codex-home dir exists (from an earlier successful prepare_spawn — see
        // `test_prepare_spawn_reuses_same_codex_home_dir_across_calls` for the full
        // spawn-then-resume round trip), a `resume` command must still be rewritten
        // — it's pulpo's own `resume_command` output, targeting a session whose
        // rollout files live under that exact isolated dir.
        let tmp = tempfile::tempdir().unwrap();
        let codex_home = tmp
            .path()
            .join("harness")
            .join("22222222-2222-2222-2222-222222222222")
            .join("codex-home");
        std::fs::create_dir_all(&codex_home).unwrap();

        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "codex resume abc-123"))
            .unwrap();
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(tokens[0], "codex");
        assert_eq!(tokens[1], "--dangerously-bypass-hook-trust");
        assert_eq!(&tokens[2..], ["resume", "abc-123"]);
        assert_eq!(plan.env[0].1, codex_home.to_string_lossy());
    }

    #[test]
    fn test_prepare_spawn_skips_when_bypass_flag_present() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = CodexAdapter
            .prepare_spawn(&ctx(
                tmp.path(),
                "codex --dangerously-bypass-hook-trust -p hi",
            ))
            .unwrap();
        assert_eq!(plan.command, "codex --dangerously-bypass-hook-trust -p hi");
    }

    #[test]
    fn test_prepare_spawn_skips_when_codex_home_prefix_present() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "env CODEX_HOME=/custom codex -p hi"))
            .unwrap();
        assert_eq!(plan.command, "env CODEX_HOME=/custom codex -p hi");
    }

    #[test]
    fn test_prepare_spawn_falls_back_on_unparseable_command() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "codex \"unterminated"))
            .unwrap();
        assert_eq!(plan.command, "codex \"unterminated");
    }

    #[test]
    fn test_prepare_spawn_unchanged_when_not_codex() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "bash"))
            .unwrap();
        assert_eq!(plan.command, "bash");
    }

    #[test]
    fn test_prepare_spawn_falls_back_when_data_dir_cannot_be_created() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let plan = CodexAdapter
            .prepare_spawn(&ctx(&blocker, "codex -p hi"))
            .unwrap();
        assert_eq!(plan.command, "codex -p hi");
        assert!(plan.files.is_empty());
    }

    // -- resume_command --

    #[test]
    fn test_resume_command_plain() {
        let cmd = CodexAdapter
            .resume_command("codex -m gpt-5-codex 'fix'", "sid-1")
            .unwrap();
        assert_eq!(cmd, "codex resume sid-1 -m gpt-5-codex fix");
    }

    #[test]
    fn test_resume_command_strips_existing_resume_id() {
        let cmd = CodexAdapter
            .resume_command("codex resume old-sid -p hi", "sid-new")
            .unwrap();
        assert_eq!(cmd, "codex resume sid-new -p hi");
    }

    #[test]
    fn test_resume_command_strips_existing_last_flag() {
        let cmd = CodexAdapter
            .resume_command("codex resume --last -p hi", "sid-new")
            .unwrap();
        assert_eq!(cmd, "codex resume sid-new -p hi");
    }

    #[test]
    fn test_resume_command_preserves_env_prefix() {
        let cmd = CodexAdapter
            .resume_command("env FOO=bar codex -p hi", "sid-1")
            .unwrap();
        let tokens = shell_words::split(&cmd).unwrap();
        assert_eq!(
            tokens,
            ["env", "FOO=bar", "codex", "resume", "sid-1", "-p", "hi"]
        );
    }

    #[test]
    fn test_resume_command_none_when_not_codex() {
        assert!(CodexAdapter.resume_command("bash", "sid-1").is_none());
    }

    #[test]
    fn test_resume_command_none_when_unparseable() {
        assert!(
            CodexAdapter
                .resume_command("codex \"unterminated", "sid-1")
                .is_none()
        );
    }

    // -- parse_event: hooks --

    #[test]
    fn test_parse_event_session_start() {
        let raw = serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": "sid-1",
            "cwd": "/repo",
            "source": "startup",
        });
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
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
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::SessionStarted {
                harness_session_id: Some("sid-1".into()),
                resumed: true,
            }
        );
    }

    #[test]
    fn test_parse_event_user_prompt_submit_is_working() {
        let raw = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "sid-1",
            "turn_id": "t1",
            "prompt": "fix it",
        });
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::Working
        );
    }

    #[test]
    fn test_parse_event_stop_turn_finished_with_summary() {
        let raw = serde_json::json!({
            "hook_event_name": "Stop",
            "session_id": "sid-1",
            "turn_id": "t1",
            "last_assistant_message": "Fixed the bug",
            "stop_hook_active": false,
        });
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::TurnFinished {
                summary: Some("Fixed the bug".into())
            }
        );
    }

    #[test]
    fn test_parse_event_stop_no_summary() {
        let raw = serde_json::json!({"hook_event_name": "Stop"});
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::TurnFinished { summary: None }
        );
    }

    #[test]
    fn test_parse_event_session_end() {
        let raw = serde_json::json!({
            "hook_event_name": "SessionEnd",
            "session_id": "sid-1",
            "reason": "other",
        });
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::SessionEnded {
                reason: Some("other".into())
            }
        );
    }

    #[test]
    fn test_parse_event_session_end_no_reason() {
        let raw = serde_json::json!({"hook_event_name": "SessionEnd"});
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::SessionEnded { reason: None }
        );
    }

    #[test]
    fn test_parse_event_permission_request_is_needs_input() {
        let raw = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "sid-1",
            "turn_id": "t1",
            "tool_name": "shell",
            "tool_input": {"command": "rm -rf /"},
        });
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Permission
            }
        );
    }

    #[test]
    fn test_parse_event_unknown_hook_is_none() {
        let raw = serde_json::json!({"hook_event_name": "PreToolUse"});
        assert!(CodexAdapter.parse_event(&raw).unwrap().is_none());
    }

    #[test]
    fn test_parse_event_missing_hook_event_name_is_none() {
        let raw = serde_json::json!({});
        assert!(CodexAdapter.parse_event(&raw).unwrap().is_none());
    }

    // -- parse_event: notify payload --

    #[test]
    fn test_parse_event_notify_turn_complete_snake_case_thread_id() {
        let raw = serde_json::json!({
            "type": "agent-turn-complete",
            "thread_id": "thread-1",
            "turn_id": "t1",
            "cwd": "/repo",
            "last_assistant_message": "All done",
            "input_messages": ["fix it"],
        });
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::TurnFinished {
                summary: Some("All done".into())
            }
        );
    }

    #[test]
    fn test_parse_event_notify_turn_complete_kebab_case_thread_id() {
        let raw = serde_json::json!({
            "type": "agent-turn-complete",
            "thread-id": "thread-1",
            "turn-id": "t1",
            "cwd": "/repo",
            "last-assistant-message": "All done",
            "input-messages": ["fix it"],
        });
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::TurnFinished {
                summary: Some("All done".into())
            }
        );
    }

    #[test]
    fn test_parse_event_notify_turn_complete_no_summary() {
        let raw = serde_json::json!({"type": "agent-turn-complete", "thread_id": "thread-1"});
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::TurnFinished { summary: None }
        );
    }

    #[test]
    fn test_parse_event_notify_unknown_type_is_none() {
        let raw = serde_json::json!({"type": "something-else"});
        assert!(CodexAdapter.parse_event(&raw).unwrap().is_none());
    }

    // -- small helpers --

    #[test]
    fn test_contains_resume_detects_resume() {
        assert!(contains_resume(&["codex".into(), "resume".into()]));
    }

    #[test]
    fn test_contains_resume_false_for_clean_command() {
        assert!(!contains_resume(&[
            "codex".into(),
            "-p".into(),
            "hi".into()
        ]));
    }

    #[test]
    fn test_already_bypassed_or_pinned_detects_bypass_flag() {
        assert!(already_bypassed_or_pinned(&[
            "codex".into(),
            "--dangerously-bypass-hook-trust".into()
        ]));
    }

    #[test]
    fn test_already_bypassed_or_pinned_detects_codex_home_prefix() {
        assert!(already_bypassed_or_pinned(&[
            "CODEX_HOME=/x".into(),
            "codex".into()
        ]));
    }

    #[test]
    fn test_already_bypassed_or_pinned_false_for_clean_command() {
        assert!(!already_bypassed_or_pinned(&[
            "codex".into(),
            "-p".into(),
            "hi".into()
        ]));
    }

    #[test]
    fn test_codex_token_index_not_found() {
        assert!(codex_token_index(&["bash".to_owned()]).is_none());
    }

    #[test]
    fn test_codex_token_index_env_with_no_command_after() {
        let tokens = vec!["env".to_owned(), "FOO=bar".to_owned()];
        assert!(codex_token_index(&tokens).is_none());
    }

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
    fn test_toml_escape_quotes_and_backslashes() {
        assert_eq!(toml_escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn test_notify_line_shape() {
        let line = notify_line("/opt/pulpo/pulpo");
        assert_eq!(
            line,
            "notify = [\"sh\", \"-c\", \"'/opt/pulpo/pulpo' hook codex-notify \\\"$0\\\"\"]\n"
        );
    }

    #[test]
    fn test_notify_line_escapes_single_quote_in_path() {
        // Parse the produced line as real TOML and check the *decoded* shell
        // script text, rather than the raw (TOML-escaped) bytes — round-trips
        // through both escaping layers (shell-quote, then TOML-string) correctly.
        let line = notify_line("/opt/o'brien/pulpo");
        let parsed: toml::Value = toml::from_str(&line).unwrap();
        let script = parsed["notify"].as_array().unwrap()[2].as_str().unwrap();
        assert_eq!(script, r#"'/opt/o'\''brien/pulpo' hook codex-notify "$0""#);
    }

    #[test]
    fn test_hook_table_shape() {
        let table = hook_table("Stop", "/opt/pulpo/pulpo", 5);
        assert!(table.contains("[[hooks.Stop]]"));
        assert!(table.contains("[[hooks.Stop.hooks]]"));
        assert!(table.contains("command = \"/opt/pulpo/pulpo hook codex --event Stop\""));
        assert!(table.contains("timeout = 5"));
    }

    #[test]
    fn test_build_config_toml_no_existing_config() {
        let config = build_config_toml("", "/opt/pulpo/pulpo");
        assert!(config.starts_with("notify ="));
        for (event, _) in HOOK_EVENTS {
            assert!(config.contains(&format!("[[hooks.{event}]]")));
        }
    }

    #[test]
    fn test_build_config_toml_notify_precedes_existing_config() {
        // `notify` must be root-scoped regardless of what table (if any) the
        // user's own config leaves "open" at EOF — see `build_config_toml`'s doc
        // comment. Placing it first guarantees that.
        let config = build_config_toml("model = \"gpt-5-codex\"\n", "/opt/pulpo/pulpo");
        assert!(config.starts_with("notify ="));
        assert!(config.contains("\nmodel = \"gpt-5-codex\"\n"));
        assert!(config.find("notify =").unwrap() < config.find("model =").unwrap());
    }

    #[test]
    fn test_build_config_toml_is_valid_toml_with_expected_shape() {
        // The generated file must actually parse — a syntax error here would
        // silently break Codex entirely for every pulpo-spawned session.
        let config = build_config_toml(
            "model = \"gpt-5-codex\"\n[mcp_servers.demo]\ncommand = \"demo\"\n",
            "/opt/pulpo/pulpo",
        );
        let parsed: toml::Value = toml::from_str(&config).unwrap();
        assert_eq!(parsed["model"].as_str(), Some("gpt-5-codex"));
        assert_eq!(
            parsed["mcp_servers"]["demo"]["command"].as_str(),
            Some("demo")
        );
        assert_eq!(parsed["notify"].as_array().unwrap().len(), 3);
        for (event, timeout) in HOOK_EVENTS {
            let hook = &parsed["hooks"][event][0]["hooks"][0];
            assert_eq!(hook["type"].as_str(), Some("command"));
            assert_eq!(
                hook["command"].as_str(),
                Some(format!("/opt/pulpo/pulpo hook codex --event {event}").as_str())
            );
            assert_eq!(hook["timeout"].as_integer(), Some(i64::from(*timeout)));
        }
    }

    // -- source_codex_home / source_codex_home_from --

    #[test]
    fn test_source_codex_home_from_prefers_env_var() {
        assert_eq!(
            source_codex_home_from(Some("/custom/codex".into()), Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/custom/codex"))
        );
    }

    #[test]
    fn test_source_codex_home_from_falls_back_to_home_dot_codex() {
        assert_eq!(
            source_codex_home_from(None, Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/home/u/.codex"))
        );
    }

    #[test]
    fn test_source_codex_home_from_none_when_no_home() {
        assert_eq!(source_codex_home_from(None, None), None);
    }

    #[test]
    fn test_source_codex_home_delegates_to_real_env_and_home_dir() {
        // Smoke test for the thin real wrapper; branch coverage of the resolution
        // logic itself lives in the `source_codex_home_from` tests above — this
        // never mutates the real process environment (`set_var`/`remove_var`
        // require `unsafe`, forbidden workspace-wide, and would race with any other
        // test in this binary touching the same process-wide state concurrently).
        let _ = source_codex_home();
    }

    // -- seed_auth_json / write_config_toml (direct, no env vars) --

    #[test]
    fn test_seed_auth_json_none_source_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        seed_auth_json(None, tmp.path());
        assert!(!tmp.path().join("auth.json").exists());
    }

    #[test]
    fn test_seed_auth_json_skips_silently_when_source_file_absent() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        seed_auth_json(Some(source.path()), dest.path());
        assert!(!dest.path().join("auth.json").exists());
    }

    #[test]
    fn test_seed_auth_json_copies_when_present() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("auth.json"), br#"{"token":"abc"}"#).unwrap();
        let dest = tempfile::tempdir().unwrap();
        seed_auth_json(Some(source.path()), dest.path());
        let copied = std::fs::read_to_string(dest.path().join("auth.json")).unwrap();
        assert_eq!(copied, r#"{"token":"abc"}"#);
    }

    #[test]
    fn test_seed_auth_json_warns_and_continues_on_copy_failure() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("auth.json"), b"{}").unwrap();
        // `dest` is a file, not a directory — `codex_home.join("auth.json")` is
        // syntactically fine but `std::fs::copy` to it fails. Must not panic.
        let blocker_dir = tempfile::tempdir().unwrap();
        let dest = blocker_dir.path().join("not-a-directory");
        std::fs::write(&dest, b"blocker").unwrap();
        seed_auth_json(Some(source.path()), &dest);
    }

    #[test]
    fn test_write_config_toml_no_source_dir() {
        let codex_home = tempfile::tempdir().unwrap();
        let path = write_config_toml(codex_home.path(), None, "/opt/pulpo/pulpo").unwrap();
        let config = std::fs::read_to_string(&path).unwrap();
        assert!(config.trim_start().starts_with("notify ="));
    }

    #[test]
    fn test_write_config_toml_source_dir_without_config_toml() {
        let source = tempfile::tempdir().unwrap();
        let codex_home = tempfile::tempdir().unwrap();
        let path =
            write_config_toml(codex_home.path(), Some(source.path()), "/opt/pulpo/pulpo").unwrap();
        let config = std::fs::read_to_string(&path).unwrap();
        assert!(config.trim_start().starts_with("notify ="));
    }

    #[test]
    fn test_write_config_toml_merges_existing_user_config() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(
            source.path().join("config.toml"),
            "model = \"gpt-5-codex\"\n[mcp_servers.demo]\ncommand = \"demo\"\n",
        )
        .unwrap();
        let codex_home = tempfile::tempdir().unwrap();
        let path =
            write_config_toml(codex_home.path(), Some(source.path()), "/opt/pulpo/pulpo").unwrap();
        let config = std::fs::read_to_string(&path).unwrap();
        assert!(config.contains("model = \"gpt-5-codex\""));
        assert!(config.contains("[mcp_servers.demo]"));
        assert!(config.contains("notify = "));
        // `notify` must precede the user's content — see `build_config_toml`'s doc
        // comment for why (TOML table-scoping of bare keys).
        assert!(config.find("notify =").unwrap() < config.find("model =").unwrap());
        // The whole file must still parse and preserve the MCP server entry.
        let parsed: toml::Value = toml::from_str(&config).unwrap();
        assert_eq!(
            parsed["mcp_servers"]["demo"]["command"].as_str(),
            Some("demo")
        );
    }

    #[test]
    fn test_strip_existing_resume_noop_when_no_resume() {
        let mut tokens = vec!["codex".to_owned(), "-p".to_owned(), "hi".to_owned()];
        strip_existing_resume(&mut tokens, 0);
        assert_eq!(tokens, vec!["codex", "-p", "hi"]);
    }
}
