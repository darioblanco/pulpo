//! Codex CLI adapter — isolated `CODEX_HOME` + `--dangerously-bypass-hook-trust`.
//!
//! Codex has no `--settings`-equivalent flag and no confirmed way to inject a
//! `[hooks]` table via `-c`, so this adapter mirrors the Claude adapter's
//! isolated-file approach at one remove: instead of writing one settings file, it
//! redirects the whole `CODEX_HOME` (which governs `config.toml`, `auth.json`,
//! `history.jsonl`, and the `sessions/` directory together) to a per-session
//! directory under `<data_dir>/harness/<session_id>/codex-home/`. It seeds that
//! directory with:
//! - a copy of the user's real `auth.json`, mode forced to `0600` regardless of the
//!   source file's mode (skipped silently when absent — see [`seed_auth_json`]);
//! - symlinks to the user's real `AGENTS.md`/`skills`/`rules`/`plugins`/`prompts`/
//!   `memories`, cached model list, and installation id, so a pulpo-spawned
//!   session still sees them (see [`symlink_real_home_entries`]);
//! - a `config.toml` merged from the user's real one — via a real `toml::Table`
//!   merge, not string concatenation — plus pulpo's own `notify`/
//!   `[[hooks.<Event>]]` entries (see [`build_config_toml`]);
//!
//! and rewrites the command to add `--dangerously-bypass-hook-trust` — without it,
//! the first hook fires a blocking "Hooks need review" TUI prompt pulpo can't
//! answer.
//!
//! Verified against Codex CLI 0.153.0 docs/source (see
//! `docs/architecture/harness-adapters.md` for the full source list).
//! `--dangerously-bypass-hook-trust` was broken for the interactive TUI in
//! 0.131.0–0.133.0 (issue #24093) but fixed by PR #24317 — safe to rely on at
//! 0.153.0 or newer.
//!
//! UNVERIFIED / deviations from the literal research spec:
//! - Codex's own hook JSON payload has no confirmed field naming which event
//!   fired (unlike Claude's `hook_event_name`). Rather than guess a field name,
//!   each hook command passes `--event <Name>` — the same disambiguation
//!   mechanism `pulpo hook <harness> --event <Name>` already offers generically
//!   (see `pulpo-cli/src/hook.rs::build_hook_body`).
//! - The exact field spelling in Codex's `notify` payload for the session/thread
//!   id and the assistant's turn summary: both `snake_case` (`thread_id`,
//!   `last_assistant_message`, as the spec's mapping table lists them) and
//!   `kebab-case` (Codex's other documented notify fields use it elsewhere) are
//!   checked, by [`CodexAdapter::parse_event`] and the CLI's
//!   `execute_codex_notify_hook` alike.
//! - [`CodexAdapter::resume_command`]'s exact shape for `codex exec` (nesting
//!   `resume` after `exec`: `codex exec resume <id>`) has no confirmed docs/
//!   example — inferred from PR #26434 ("Preserve hook trust bypass in codex exec
//!   threads"). See its doc comment.
//!
//! Deviation from the literal spec (correctness fix): the spec describes writing
//! the user's real `config.toml` first, then appending pulpo's `notify`/`hooks`
//! keys as text. [`build_config_toml`] instead parses the user's config into a
//! `toml::Table` and merges pulpo's own keys into it as structured data — see its
//! doc comment for why a duplicate `notify` key (this machine's own
//! `~/.codex/config.toml` already has a root `notify`, and the old text-append
//! approach produced invalid TOML for it) and the bare-key/table-scoping hazard
//! the original approach had are both entirely avoided this way, not just
//! repositioned around.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;
use toml::Value as TomlValue;
use tracing::{info, warn};

use super::{
    HarnessAdapter, HarnessEvent, HarnessSignals, NeedsInputReason, SpawnContext, SpawnPlan,
    resolve_pulpo_bin,
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

/// Offset from `codex_idx` to where a `resume` subcommand — existing, or about to
/// be inserted by [`CodexAdapter::resume_command`] — belongs: right after `codex`,
/// or, for `codex exec ...`, right after `exec` instead (see that method's doc
/// comment for why `exec` nests differently).
fn resume_subcommand_index(tokens: &[String], codex_idx: usize) -> usize {
    if tokens.get(codex_idx + 1).map(String::as_str) == Some("exec") {
        codex_idx + 2
    } else {
        codex_idx + 1
    }
}

/// True for a token that can follow `resume` as its target: a session id, or the
/// literal `--last` — the only resume-target token that itself looks like a flag.
fn is_resume_target(token: &str) -> bool {
    token == "--last" || !token.starts_with('-')
}

/// Remove an existing `resume <target>` pair at `resume_idx`, if present.
/// `<target>` is only removed when it's actually a resume target (see
/// [`is_resume_target`]) — e.g. `codex resume -m x` (a bare resume immediately
/// followed by an unrelated flag) keeps `-m x` rather than mistaking it for the
/// target.
fn strip_existing_resume(tokens: &mut Vec<String>, resume_idx: usize) {
    if tokens.get(resume_idx).map(String::as_str) != Some("resume") {
        return;
    }
    tokens.remove(resume_idx);
    if tokens
        .get(resume_idx)
        .map(String::as_str)
        .is_some_and(is_resume_target)
    {
        tokens.remove(resume_idx);
    }
}

/// Global Codex flags documented (research spec / CLI reference) to take a value —
/// the token immediately following one of these in a command line is that flag's
/// value, never a trailing positional. A heuristic bounded by today's known flags:
/// an undocumented value-taking flag added later could be misread as ending in a
/// bare positional, but the failure mode is limited to [`strip_trailing_positionals`]
/// keeping one extra token it should have dropped, never breaking a working command.
const VALUE_FLAGS: &[&str] = &[
    "-m",
    "--model",
    "-s",
    "--sandbox",
    "-a",
    "--ask-for-approval",
    "-c",
    "--config",
    "-C",
    "--cd",
    "-p",
    "--profile",
];

/// Remove a trailing run of positional (non-flag, not-a-flag's-value) tokens from
/// `tokens[start..]` in place — Codex's positional prompt argument
/// (`codex 'fix the bug'`, or `codex exec 'fix the bug'`'s own prompt), which
/// [`CodexAdapter::resume_command`] must not replay as a new turn (see its doc
/// comment).
fn strip_trailing_positionals(tokens: &mut Vec<String>, start: usize) {
    let mut positional = vec![false; tokens.len().saturating_sub(start)];
    let mut skip_next_as_value = false;
    for (offset, token) in tokens[start..].iter().enumerate() {
        if skip_next_as_value {
            skip_next_as_value = false;
        } else if VALUE_FLAGS.contains(&token.as_str()) {
            skip_next_as_value = true;
        } else if !token.starts_with('-') {
            positional[offset] = true;
        }
    }
    let trailing_positionals = positional
        .iter()
        .rev()
        .take_while(|is_positional| **is_positional)
        .count();
    tokens.truncate(tokens.len() - trailing_positionals);
}

/// The real Codex home pulpo copies `auth.json`/`config.toml`/extra entries from:
/// `$CODEX_HOME` when pulpod's own process has it set, else `~/.codex`.
fn source_codex_home() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(path) = TEST_REAL_HOME.with(|cell| cell.borrow().clone()) {
        return Some(path);
    }
    source_codex_home_from(std::env::var("CODEX_HOME").ok(), dirs::home_dir())
}

// Test-only seam for `source_codex_home` — see `tests::set_test_real_home`.
#[cfg(test)]
thread_local! {
    static TEST_REAL_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
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

/// The `notify` value: Codex delivers its payload as a trailing argv element, not
/// stdin, so this wraps `pulpo hook codex-notify` in `sh -c '... "$0"'` to turn
/// that trailing argument into `$0` (see
/// `pulpo-cli/src/hook.rs::execute_codex_notify_hook`). Returned as a real
/// `toml::Value` rather than a hand-built string so `toml::to_string` handles all
/// TOML escaping — only the shell-quoting of a literal `'` in `pulpo_bin` is done
/// by hand here, a different escaping layer entirely.
fn notify_value(pulpo_bin: &str) -> TomlValue {
    let shell_escaped = pulpo_bin.replace('\'', r"'\''");
    let script = format!("'{shell_escaped}' hook codex-notify \"$0\"");
    TomlValue::Array(vec![
        TomlValue::String("sh".to_owned()),
        TomlValue::String("-c".to_owned()),
        TomlValue::String(script),
    ])
}

/// One `hooks.<Event>` array element: `{ hooks = [{ type = "command", command =
/// "<pulpo-bin> hook codex --event <Event>", timeout = <timeout> }] }`. `pulpo_bin`
/// is shell-quoted (`shell_words::quote`) since `command` becomes a shell command
/// string Codex executes verbatim — an unquoted path containing a space would
/// otherwise be split into multiple arguments.
fn hook_entry_value(pulpo_bin: &str, event: &str, timeout: u32) -> TomlValue {
    let command = format!(
        "{} hook codex --event {event}",
        shell_words::quote(pulpo_bin)
    );
    let mut command_table = toml::Table::new();
    command_table.insert("type".to_owned(), TomlValue::String("command".to_owned()));
    command_table.insert("command".to_owned(), TomlValue::String(command));
    command_table.insert("timeout".to_owned(), TomlValue::Integer(i64::from(timeout)));

    let mut entry_table = toml::Table::new();
    entry_table.insert(
        "hooks".to_owned(),
        TomlValue::Array(vec![TomlValue::Table(command_table)]),
    );
    TomlValue::Table(entry_table)
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

/// Parse the user's real `config.toml` into a table pulpo can merge its own keys
/// into. An absent or unparseable file yields an empty table — a parse failure
/// only warns; starting from empty still lets pulpo spawn Codex with its own
/// hooks, which is far better than refusing to spawn at all because the user's
/// file has a syntax error.
fn parse_existing_config(existing: &str) -> toml::Table {
    let trimmed = existing.trim();
    if trimmed.is_empty() {
        return toml::Table::new();
    }
    toml::from_str(trimmed).unwrap_or_else(|error| {
        warn!(
            %error,
            "codex adapter: user's config.toml failed to parse, starting from an empty config"
        );
        toml::Table::new()
    })
}

/// Build the full `config.toml` contents by merging pulpo's own `notify` and
/// `[[hooks.<Event>]]` entries into the user's real config (if any) — via a real
/// TOML merge (`toml::Table`), not string concatenation.
///
/// `notify` is *set*: any pre-existing `notify` in the user's config is replaced,
/// not merged — Codex's config format has only one `notify` slot, and a
/// `CODEX_HOME`-wide hook takeover is inherently exclusive with the user's own
/// `notify` setup. `hooks.<Event>` entries are *appended*: any hooks the user
/// already configured for that event are preserved, with pulpo's own handler added
/// as one more array element.
///
/// Deviation from the literal research spec (which reads "copy the user's config in
/// first, then append the notify/hooks keys" — i.e. string concatenation): the
/// user's real `~/.codex/config.toml` may already have its own root `notify` key
/// (this machine's does) — naive string concatenation of a second `notify = [...]`
/// line produces two `notify` keys in the same document, which is invalid TOML and
/// makes Codex refuse to start. It also has a companion table-scoping hazard: a
/// bare key appended after one or more `[table]` headers gets silently absorbed
/// into whichever table came last, rather than landing at the document root.
/// Parsing into a `toml::Table` and merging as structured data sidesteps both
/// entirely — there's no such thing as "the wrong scope" once `notify`/`hooks` are
/// inserted as keys of the root `Table` value directly, and inserting under an
/// existing key name (`notify`) replaces it rather than duplicating it.
fn build_config_toml(existing: &str, pulpo_bin: &str) -> Result<String> {
    let mut table = parse_existing_config(existing);
    table.insert("notify".to_owned(), notify_value(pulpo_bin));

    let mut hooks_table = match table.remove("hooks") {
        Some(TomlValue::Table(hooks_table)) => hooks_table,
        _ => toml::Table::new(),
    };
    for &(event, timeout) in HOOK_EVENTS {
        let mut array = match hooks_table.remove(event) {
            Some(TomlValue::Array(array)) => array,
            _ => Vec::new(),
        };
        array.push(hook_entry_value(pulpo_bin, event, timeout));
        hooks_table.insert(event.to_owned(), TomlValue::Array(array));
    }
    table.insert("hooks".to_owned(), TomlValue::Table(hooks_table));

    Ok(toml::to_string(&table)?)
}

/// Set `path`'s mode to `0600` regardless of the source file's own mode — `auth.json`
/// holds Codex credentials and must never be group/world-readable in the isolated
/// home, even if the source happened to have looser permissions (or the platform
/// default `std::fs::copy` mode is looser). Best-effort: a failure only warns, same
/// contract as the copy itself.
#[cfg(unix)]
fn set_auth_json_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        warn!(%error, "codex adapter: failed to set auth.json permissions to 0600");
    }
}

/// No-op on non-unix targets (no mode bits to set); see [`create_symlink`]'s
/// counterpart for the same unix/non-unix split.
#[cfg(not(unix))]
fn set_auth_json_permissions(_path: &Path) {}

/// Best-effort copy of the real `auth.json` into the isolated `codex_home`, mode
/// forced to `0600`. Skips silently when the source is unknown or the file doesn't
/// exist; a copy failure only warns — losing auth is recoverable (Codex just
/// prompts to log in) and must never abort the whole spawn rewrite.
fn seed_auth_json(source_dir: Option<&Path>, codex_home: &Path) {
    let Some(source_dir) = source_dir else {
        return;
    };
    let source_auth = source_dir.join("auth.json");
    if !source_auth.is_file() {
        return;
    }
    let dest = codex_home.join("auth.json");
    match std::fs::copy(&source_auth, &dest) {
        Ok(_) => set_auth_json_permissions(&dest),
        Err(error) => {
            warn!(%error, "codex adapter: failed to copy auth.json, spawning without it");
        }
    }
}

/// Real-home entries pulpo's isolated `CODEX_HOME` symlinks in (not copies — some
/// can be large directories, and pulpo never needs to modify them) so a
/// pulpo-spawned session still sees them: instructions/skills/rules/plugins/
/// prompts/memories the user configured, plus Codex's own cached model list and
/// installation id. Excludes `auth.json` (copied, not symlinked — see
/// [`seed_auth_json`]), `config.toml` (generated fresh — see [`build_config_toml`]),
/// and anything that must stay genuinely separate per isolated home: `sessions/`/
/// `history.jsonl` (each pulpo session gets its own Codex thread history — see
/// [`has_existing_rollout`] and `usage::scan_local_usage`), `log/`, and Codex's
/// sqlite state (shared/locked state must never be aliased across concurrent pulpo
/// sessions).
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

/// Symlink each of [`SYMLINKED_HOME_ENTRIES`] present in `source_dir` into
/// `codex_home`, skipping any entry absent from the source or already present at
/// the destination (a real file/dir, or an existing symlink — even a broken one).
/// The latter makes this idempotent across repeated `prepare_spawn` calls for the
/// same session (spawn, then every resume). Best-effort: a failed symlink only
/// warns, same contract as [`seed_auth_json`] — losing one of these is recoverable
/// (Codex just runs without it) and must never abort the spawn.
fn symlink_real_home_entries(source_dir: Option<&Path>, codex_home: &Path) {
    let Some(source_dir) = source_dir else {
        return;
    };
    for name in SYMLINKED_HOME_ENTRIES {
        let source = source_dir.join(name);
        if !source.exists() {
            continue;
        }
        let dest = codex_home.join(name);
        if dest.symlink_metadata().is_ok() {
            continue;
        }
        if let Err(error) = create_symlink(&source, &dest) {
            warn!(%error, entry = %name, "codex adapter: failed to symlink real Codex home entry");
        }
    }
}

#[cfg(unix)]
fn create_symlink(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, dest)
}

/// No-op-failing on non-unix targets (pulpo only ships for macOS and Linux today,
/// both unix); see [`set_auth_json_permissions`]'s counterpart for the same split.
#[cfg(not(unix))]
fn create_symlink(_source: &Path, _dest: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "codex adapter: symlinking the real Codex home is only supported on unix",
    ))
}

/// True if `dir` (or any of its subdirectories) contains a `rollout-*.jsonl` file.
fn dir_contains_rollout(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            dir_contains_rollout(&path)
        } else {
            path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
                && path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stem.starts_with("rollout-"))
        }
    })
}

/// True if `<codex_home>/sessions/` already contains at least one
/// `rollout-*.jsonl` file, recursively (Codex nests them under
/// `sessions/YYYY/MM/DD/`). Each pulpo session's `CODEX_HOME` is isolated, so
/// finding any rollout there at all — no id needed to disambiguate — means a
/// *previous* spawn into this exact directory already started a Codex thread.
/// Used by [`rewrite_spawn`] to resume that thread with `--last` instead of
/// silently starting a fresh one when hooks never reported its id.
fn has_existing_rollout(codex_home: &Path) -> bool {
    dir_contains_rollout(&codex_home.join("sessions"))
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

    let content = build_config_toml(&existing, pulpo_bin)?;
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
    symlink_real_home_entries(source_dir.as_deref(), &codex_home);
    let config_path = write_config_toml(&codex_home, source_dir.as_deref(), &pulpo_bin)?;

    // A lost session whose hooks never fired leaves `harness_session_id` unknown in
    // the store, so a later resume attempt falls back to replaying the *original*
    // command (see `session::manager::resolve_resume_command`) — with no `resume`
    // token at all. Since this session's `CODEX_HOME` is isolated, any rollout file
    // already under it can only be that same session's own prior thread: resume it
    // by file position (`--last`) instead of silently starting a brand new one.
    if !contains_resume(&tokens) && has_existing_rollout(&codex_home) {
        info!(
            session = %ctx.session_name,
            "codex adapter: isolated codex-home already has a session rollout, resuming --last instead of starting a fresh thread"
        );
        // Same shape as `resume_command`: nest after `exec`, and drop the original
        // positional prompt so it is not replayed as a new turn on resume.
        let resume_idx = resume_subcommand_index(&tokens, codex_idx);
        strip_trailing_positionals(&mut tokens, resume_idx);
        tokens.splice(
            resume_idx..resume_idx,
            ["resume".to_owned(), "--last".to_owned()],
        );
    }

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
        // Codex has no flag to preset a session/thread id at launch. It's known
        // only once `SessionStart` fires — the `--last` rewrite above resumes a
        // lost session's own prior rollout file by position, not by id, so it
        // doesn't learn `harness_session_id` either.
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

    /// Strip any existing `resume <target>` (from a previous `resume_command`
    /// output being resumed again) and any trailing positional prompt argument —
    /// Codex replays a positional argument as a brand new turn
    /// (`codex resume <id> 'fix the bug'` submits "fix the bug" again), which must
    /// not happen here; a bare `codex resume <id>` alone resumes the session at its
    /// own last turn. For `codex exec ...`, `resume` nests *after* `exec` instead
    /// (`codex exec resume <id> <flags>`) — UNVERIFIED: no confirmed docs/example
    /// of this exact shape, but PR #26434 ("Preserve hook trust bypass in codex
    /// exec threads") explicitly forwards the bypass flag for "fresh thread
    /// start/resume/fork" under `codex exec`, implying `resume` nests the same way
    /// there.
    fn resume_command(&self, original_command: &str, harness_session_id: &str) -> Option<String> {
        let mut tokens = shell_words::split(original_command).ok()?;
        let codex_idx = codex_token_index(&tokens)?;
        let resume_idx = resume_subcommand_index(&tokens, codex_idx);
        strip_existing_resume(&mut tokens, resume_idx);
        strip_trailing_positionals(&mut tokens, resume_idx);
        tokens.splice(
            resume_idx..resume_idx,
            ["resume".to_owned(), harness_session_id.to_owned()],
        );
        Some(shell_words::join(&tokens))
    }

    fn parse_event(&self, raw: &Value) -> Result<Option<HarnessEvent>> {
        if let Some(hook_event_name) = raw.get("hook_event_name").and_then(Value::as_str) {
            return Ok(parse_hook_event(raw, hook_event_name));
        }

        Ok(raw
            .get("type")
            .and_then(Value::as_str)
            .and_then(|notify_type| parse_notify_event(raw, notify_type)))
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

/// A `SessionStart` hook's session id: `session_id` (as documented), or
/// `thread_id`/`thread-id` — the same tolerance [`turn_summary`] and
/// [`parse_notify_event`] already apply for the notify payload's fields, in case a
/// hook payload ever ends up using that mechanism's field naming instead.
fn extract_session_id(raw: &Value) -> Option<String> {
    raw.get("session_id")
        .or_else(|| raw.get("thread_id"))
        .or_else(|| raw.get("thread-id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Translate one Codex *hook* payload (identified by `hook_event_name`) into a
/// normalized event. Split out of [`CodexAdapter::parse_event`] so hook payloads
/// are always routed here first, ahead of the `type`-keyed notify payload check —
/// a hook event must never be misrouted as a notify payload even if a future hook
/// payload happens to also carry a `type` field.
fn parse_hook_event(raw: &Value, hook_event_name: &str) -> Option<HarnessEvent> {
    match hook_event_name {
        "SessionStart" => {
            let harness_session_id = extract_session_id(raw);
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
        // No `hookSpecificOutput` is ever emitted here — pulpo stays observational
        // and the real TUI approval prompt still runs.
        "PermissionRequest" => Some(HarnessEvent::NeedsInput {
            reason: NeedsInputReason::Permission,
        }),
        // PreToolUse/PostToolUse/PreCompact/PostCompact/SubagentStart/
        // SubagentStop/Interrupt exist but are out of scope for pulpo's state
        // machine today.
        _ => None,
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

    /// Override [`source_codex_home`] for the current thread only, so
    /// `prepare_spawn` tests exercise the merge/copy/symlink logic against a
    /// controlled fixture directory instead of ever touching the developer's real
    /// `~/.codex`/`$CODEX_HOME`. Resets automatically when the returned guard
    /// drops — bind it with `let _guard = ...`, not `let _ = ...`, or it resets
    /// immediately.
    fn set_test_real_home(path: &Path) -> impl Drop {
        struct ResetOnDrop;
        impl Drop for ResetOnDrop {
            fn drop(&mut self) {
                TEST_REAL_HOME.with(|cell| *cell.borrow_mut() = None);
            }
        }
        TEST_REAL_HOME.with(|cell| *cell.borrow_mut() = Some(path.to_owned()));
        ResetOnDrop
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
    // These exercise the real `prepare_spawn` end to end. Every test that reaches
    // `rewrite_spawn` sets a fake, empty (or fixture-seeded) real-home directory via
    // `set_test_real_home` first, so none of them ever read the developer's actual
    // `~/.codex` — see finding review notes on `source_codex_home`.

    #[test]
    fn test_prepare_spawn_rewrites_plain_codex() {
        let tmp = tempfile::tempdir().unwrap();
        let real_home = tempfile::tempdir().unwrap();
        let _guard = set_test_real_home(real_home.path());

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
        let config: toml::Table = toml::from_str(&std::fs::read_to_string(&plan.files[0]).unwrap())
            .expect("generated config.toml must parse");
        assert_eq!(config["notify"].as_array().unwrap().len(), 3);
        for &(event, timeout) in HOOK_EVENTS {
            let hook = &config["hooks"][event][0]["hooks"][0];
            assert_eq!(hook["type"].as_str(), Some("command"));
            assert!(
                hook["command"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("hook codex --event {event}"))
            );
            assert_eq!(hook["timeout"].as_integer(), Some(i64::from(timeout)));
        }
    }

    #[test]
    fn test_prepare_spawn_handles_absolute_path_argv0() {
        let tmp = tempfile::tempdir().unwrap();
        let real_home = tempfile::tempdir().unwrap();
        let _guard = set_test_real_home(real_home.path());

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
        let real_home = tempfile::tempdir().unwrap();
        let _guard = set_test_real_home(real_home.path());

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
        let real_home = tempfile::tempdir().unwrap();
        let _guard = set_test_real_home(real_home.path());

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
        let real_home = tempfile::tempdir().unwrap();
        let _guard = set_test_real_home(real_home.path());

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
        let real_home = tempfile::tempdir().unwrap();
        let _guard = set_test_real_home(real_home.path());
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

    // -- prepare_spawn: rollout-discovery fallback (`resume --last`) --

    #[test]
    fn test_prepare_spawn_resumes_last_when_isolated_home_already_has_a_rollout() {
        let tmp = tempfile::tempdir().unwrap();
        let real_home = tempfile::tempdir().unwrap();
        let _guard = set_test_real_home(real_home.path());

        // Simulate a lost session: an earlier spawn into this exact isolated
        // CODEX_HOME already wrote a rollout file, but its `SessionStart` hook
        // never fired, so pulpo never learned the session id.
        let rollout_dir = tmp
            .path()
            .join("harness")
            .join("22222222-2222-2222-2222-222222222222")
            .join("codex-home")
            .join("sessions")
            .join("2026")
            .join("01")
            .join("01");
        std::fs::create_dir_all(&rollout_dir).unwrap();
        std::fs::write(
            rollout_dir.join("rollout-2026-01-01T00-00-00-abc.jsonl"),
            "{}",
        )
        .unwrap();

        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "codex -p hi"))
            .unwrap();
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(
            tokens,
            [
                "codex",
                "--dangerously-bypass-hook-trust",
                "resume",
                "--last",
                "-p",
                "hi",
            ]
        );
        assert!(plan.harness_session_id.is_none());
    }

    #[test]
    fn test_prepare_spawn_resume_last_drops_prompt_and_nests_after_exec() {
        // Same rollout-discovery fallback, but the original command carried a
        // positional prompt (`codex 'fix the bug'`) or used `codex exec`: the
        // fallback must not replay the prompt as a fresh turn, and `resume` must
        // nest after `exec` — the same shape `resume_command` produces.
        let tmp = tempfile::tempdir().unwrap();
        let real_home = tempfile::tempdir().unwrap();
        let _guard = set_test_real_home(real_home.path());
        let rollout_dir = tmp
            .path()
            .join("harness")
            .join("22222222-2222-2222-2222-222222222222")
            .join("codex-home")
            .join("sessions")
            .join("2026")
            .join("01")
            .join("01");
        std::fs::create_dir_all(&rollout_dir).unwrap();
        std::fs::write(
            rollout_dir.join("rollout-2026-01-01T00-00-00-abc.jsonl"),
            "{}",
        )
        .unwrap();

        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "codex -m gpt-5 'fix the bug'"))
            .unwrap();
        assert_eq!(
            shell_words::split(&plan.command).unwrap(),
            [
                "codex",
                "--dangerously-bypass-hook-trust",
                "resume",
                "--last",
                "-m",
                "gpt-5",
            ]
        );

        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "codex exec 'fix the bug'"))
            .unwrap();
        assert_eq!(
            shell_words::split(&plan.command).unwrap(),
            [
                "codex",
                "--dangerously-bypass-hook-trust",
                "exec",
                "resume",
                "--last",
            ]
        );
    }

    #[test]
    fn test_prepare_spawn_does_not_double_resume_when_resume_already_present() {
        // A rollout already exists (as above) *and* the command already resumes an
        // explicit id — the fallback must not also inject `resume --last`.
        let tmp = tempfile::tempdir().unwrap();
        let real_home = tempfile::tempdir().unwrap();
        let _guard = set_test_real_home(real_home.path());
        let codex_home = tmp
            .path()
            .join("harness")
            .join("22222222-2222-2222-2222-222222222222")
            .join("codex-home");
        let rollout_dir = codex_home
            .join("sessions")
            .join("2026")
            .join("01")
            .join("01");
        std::fs::create_dir_all(&rollout_dir).unwrap();
        std::fs::write(rollout_dir.join("rollout-x.jsonl"), "{}").unwrap();

        let plan = CodexAdapter
            .prepare_spawn(&ctx(tmp.path(), "codex resume abc-123"))
            .unwrap();
        let tokens = shell_words::split(&plan.command).unwrap();
        assert_eq!(
            tokens,
            [
                "codex",
                "--dangerously-bypass-hook-trust",
                "resume",
                "abc-123"
            ]
        );
    }

    #[test]
    fn test_has_existing_rollout_true_when_file_present_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp
            .path()
            .join("sessions")
            .join("2026")
            .join("01")
            .join("01");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("rollout-x.jsonl"), "{}").unwrap();
        assert!(has_existing_rollout(tmp.path()));
    }

    #[test]
    fn test_has_existing_rollout_false_when_sessions_dir_missing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!has_existing_rollout(tmp.path()));
    }

    #[test]
    fn test_has_existing_rollout_false_when_only_non_rollout_files_present() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("history.jsonl"), "{}").unwrap();
        assert!(!has_existing_rollout(tmp.path()));
    }

    // -- resume_command --

    #[test]
    fn test_resume_command_plain_strips_trailing_prompt() {
        // The prompt positional must be dropped — replaying it would resubmit it
        // as a brand new turn on top of the resumed session.
        let cmd = CodexAdapter
            .resume_command("codex -m gpt-5-codex 'fix'", "sid-1")
            .unwrap();
        assert_eq!(cmd, "codex resume sid-1 -m gpt-5-codex");
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
    fn test_resume_command_bare_resume_keeps_following_flag() {
        // `codex resume -m x` — `resume` has no target here at all; `-m x` is the
        // very next flag, not the resume target, and must survive.
        let cmd = CodexAdapter
            .resume_command("codex resume -m x", "sid-new")
            .unwrap();
        assert_eq!(cmd, "codex resume sid-new -m x");
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
    fn test_resume_command_exec_nests_resume_after_exec() {
        let cmd = CodexAdapter
            .resume_command("codex exec 'fix the bug'", "sid-1")
            .unwrap();
        assert_eq!(cmd, "codex exec resume sid-1");
    }

    #[test]
    fn test_resume_command_exec_with_flags_preserves_flags_and_drops_prompt() {
        let cmd = CodexAdapter
            .resume_command("codex exec -m gpt-5-codex 'fix the bug'", "sid-1")
            .unwrap();
        assert_eq!(cmd, "codex exec resume sid-1 -m gpt-5-codex");
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

    // -- resume_command helpers --

    #[test]
    fn test_resume_subcommand_index_after_codex() {
        let tokens = ["codex".to_owned(), "-p".to_owned(), "hi".to_owned()];
        assert_eq!(resume_subcommand_index(&tokens, 0), 1);
    }

    #[test]
    fn test_resume_subcommand_index_after_exec() {
        let tokens = ["codex".to_owned(), "exec".to_owned(), "hi".to_owned()];
        assert_eq!(resume_subcommand_index(&tokens, 0), 2);
    }

    #[test]
    fn test_is_resume_target_variants() {
        assert!(is_resume_target("abc-123"));
        assert!(is_resume_target("--last"));
        assert!(!is_resume_target("-m"));
        assert!(!is_resume_target("--model"));
    }

    #[test]
    fn test_strip_existing_resume_noop_when_no_resume() {
        let mut tokens = vec!["codex".to_owned(), "-p".to_owned(), "hi".to_owned()];
        strip_existing_resume(&mut tokens, 1);
        assert_eq!(tokens, vec!["codex", "-p", "hi"]);
    }

    #[test]
    fn test_strip_existing_resume_removes_resume_and_target() {
        let mut tokens = vec![
            "codex".to_owned(),
            "resume".to_owned(),
            "sid".to_owned(),
            "-p".to_owned(),
            "hi".to_owned(),
        ];
        strip_existing_resume(&mut tokens, 1);
        assert_eq!(tokens, vec!["codex", "-p", "hi"]);
    }

    #[test]
    fn test_strip_existing_resume_removes_resume_and_last_flag() {
        let mut tokens = vec!["codex".to_owned(), "resume".to_owned(), "--last".to_owned()];
        strip_existing_resume(&mut tokens, 1);
        assert_eq!(tokens, vec!["codex"]);
    }

    #[test]
    fn test_strip_existing_resume_keeps_flag_after_bare_resume() {
        let mut tokens = vec![
            "codex".to_owned(),
            "resume".to_owned(),
            "-m".to_owned(),
            "x".to_owned(),
        ];
        strip_existing_resume(&mut tokens, 1);
        assert_eq!(tokens, vec!["codex", "-m", "x"]);
    }

    #[test]
    fn test_strip_trailing_positionals_removes_trailing_prompt() {
        let mut tokens = vec![
            "codex".to_owned(),
            "-m".to_owned(),
            "x".to_owned(),
            "fix".to_owned(),
        ];
        strip_trailing_positionals(&mut tokens, 1);
        assert_eq!(tokens, vec!["codex", "-m", "x"]);
    }

    #[test]
    fn test_strip_trailing_positionals_keeps_flag_value_pairs() {
        let mut tokens = vec!["codex".to_owned(), "-p".to_owned(), "hi".to_owned()];
        strip_trailing_positionals(&mut tokens, 1);
        assert_eq!(tokens, vec!["codex", "-p", "hi"]);
    }

    #[test]
    fn test_strip_trailing_positionals_noop_when_nothing_trailing() {
        let mut tokens = vec!["codex".to_owned(), "--foo".to_owned()];
        strip_trailing_positionals(&mut tokens, 1);
        assert_eq!(tokens, vec!["codex", "--foo"]);
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
    fn test_parse_event_session_start_accepts_snake_case_thread_id() {
        let raw = serde_json::json!({
            "hook_event_name": "SessionStart",
            "thread_id": "sid-1",
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
    fn test_parse_event_session_start_accepts_kebab_case_thread_id() {
        let raw = serde_json::json!({
            "hook_event_name": "SessionStart",
            "thread-id": "sid-1",
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
    fn test_parse_event_session_start_prefers_session_id_over_thread_id() {
        let raw = serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": "from-session-id",
            "thread_id": "from-thread-id",
        });
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::SessionStarted {
                harness_session_id: Some("from-session-id".into()),
                resumed: false,
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

    #[test]
    fn test_parse_event_prefers_hook_event_name_over_type_field() {
        // Routing must check `hook_event_name` before `type`: a payload carrying
        // both (shouldn't normally happen) must be handled as the hook event, not
        // misrouted as a notify payload.
        let raw = serde_json::json!({
            "hook_event_name": "SessionEnd",
            "reason": "other",
            "type": "agent-turn-complete",
            "last_assistant_message": "should not win",
        });
        assert_eq!(
            CodexAdapter.parse_event(&raw).unwrap().unwrap(),
            HarnessEvent::SessionEnded {
                reason: Some("other".into())
            }
        );
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
    fn test_source_codex_home_override_takes_precedence() {
        let fake = tempfile::tempdir().unwrap();
        let _guard = set_test_real_home(fake.path());
        assert_eq!(source_codex_home(), Some(fake.path().to_path_buf()));
    }

    #[test]
    fn test_source_codex_home_delegates_to_real_env_and_home_dir() {
        // No test-real-home override set here (see `set_test_real_home`) — this
        // proves `source_codex_home` really does delegate to the live
        // environment/home dir via `source_codex_home_from`, without asserting a
        // fixed path (which would be flaky depending on the machine running the
        // test).
        let expected = source_codex_home_from(std::env::var("CODEX_HOME").ok(), dirs::home_dir());
        assert_eq!(source_codex_home(), expected);
    }

    // -- seed_auth_json --

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

    #[cfg(unix)]
    #[test]
    fn test_seed_auth_json_sets_mode_0600_regardless_of_source_mode() {
        use std::os::unix::fs::PermissionsExt;
        let source = tempfile::tempdir().unwrap();
        let source_auth = source.path().join("auth.json");
        std::fs::write(&source_auth, b"{}").unwrap();
        std::fs::set_permissions(&source_auth, std::fs::Permissions::from_mode(0o644)).unwrap();
        let dest = tempfile::tempdir().unwrap();

        seed_auth_json(Some(source.path()), dest.path());

        let mode = std::fs::metadata(dest.path().join("auth.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn test_set_auth_json_permissions_warns_and_does_not_panic_on_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist.json");
        set_auth_json_permissions(&missing);
    }

    // -- symlink_real_home_entries --

    #[test]
    fn test_symlink_real_home_entries_links_present_entries() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("AGENTS.md"), b"# agents").unwrap();
        std::fs::create_dir_all(source.path().join("skills")).unwrap();
        std::fs::write(source.path().join("installation_id"), b"id-123").unwrap();
        let dest = tempfile::tempdir().unwrap();

        symlink_real_home_entries(Some(source.path()), dest.path());

        assert!(dest.path().join("AGENTS.md").is_symlink());
        assert_eq!(
            std::fs::read_to_string(dest.path().join("AGENTS.md")).unwrap(),
            "# agents"
        );
        assert!(dest.path().join("skills").is_symlink());
        assert!(dest.path().join("installation_id").is_symlink());
        // Entries absent from the source aren't created.
        assert!(!dest.path().join("rules").exists());
    }

    #[test]
    fn test_symlink_real_home_entries_none_source_is_noop() {
        let dest = tempfile::tempdir().unwrap();
        symlink_real_home_entries(None, dest.path());
        assert!(std::fs::read_dir(dest.path()).unwrap().next().is_none());
    }

    #[test]
    fn test_symlink_real_home_entries_idempotent_when_link_already_exists() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("AGENTS.md"), b"v1").unwrap();
        let dest = tempfile::tempdir().unwrap();
        symlink_real_home_entries(Some(source.path()), dest.path());
        // Calling again (simulating resume) must not error or replace the link.
        symlink_real_home_entries(Some(source.path()), dest.path());
        assert!(dest.path().join("AGENTS.md").is_symlink());
    }

    #[test]
    fn test_symlink_real_home_entries_skips_when_destination_already_a_real_file() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("AGENTS.md"), b"real").unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(dest.path().join("AGENTS.md"), b"pre-existing").unwrap();

        symlink_real_home_entries(Some(source.path()), dest.path());

        assert!(!dest.path().join("AGENTS.md").is_symlink());
        assert_eq!(
            std::fs::read_to_string(dest.path().join("AGENTS.md")).unwrap(),
            "pre-existing"
        );
    }

    #[test]
    fn test_symlink_real_home_entries_warns_and_continues_on_symlink_failure() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("AGENTS.md"), b"x").unwrap();
        std::fs::write(source.path().join("installation_id"), b"y").unwrap();
        // `dest`'s parent is a file, not a directory — every symlink call fails;
        // the loop must still finish (try every remaining entry) rather than
        // panicking or bailing out early.
        let blocker_dir = tempfile::tempdir().unwrap();
        let blocker = blocker_dir.path().join("not-a-directory");
        std::fs::write(&blocker, b"blocker").unwrap();
        let dest = blocker.join("codex-home");

        symlink_real_home_entries(Some(source.path()), &dest);
    }

    // -- write_config_toml --

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
        // The whole file must still parse and preserve the MCP server entry.
        let parsed: toml::Table = toml::from_str(&config).unwrap();
        assert_eq!(
            parsed["mcp_servers"]["demo"]["command"].as_str(),
            Some("demo")
        );
        assert_eq!(parsed["model"].as_str(), Some("gpt-5-codex"));
        assert_eq!(parsed["notify"].as_array().unwrap().len(), 3);
    }

    // -- strip_existing_resume (direct, no CLI-level round trip) --

    #[test]
    fn test_strip_existing_resume_noop_when_resume_idx_out_of_range() {
        let mut tokens = vec!["codex".to_owned()];
        strip_existing_resume(&mut tokens, 5);
        assert_eq!(tokens, vec!["codex"]);
    }

    // -- notify_value / hook_entry_value --

    #[test]
    fn test_notify_value_shape() {
        let value = notify_value("/opt/pulpo/pulpo");
        let array = value.as_array().unwrap();
        assert_eq!(array[0].as_str(), Some("sh"));
        assert_eq!(array[1].as_str(), Some("-c"));
        assert_eq!(
            array[2].as_str(),
            Some("'/opt/pulpo/pulpo' hook codex-notify \"$0\"")
        );
    }

    #[test]
    fn test_notify_value_escapes_single_quote_in_path() {
        let value = notify_value("/opt/o'brien/pulpo");
        let script = value.as_array().unwrap()[2].as_str().unwrap();
        assert_eq!(script, r#"'/opt/o'\''brien/pulpo' hook codex-notify "$0""#);
    }

    #[test]
    fn test_hook_entry_value_shape() {
        let value = hook_entry_value("/opt/pulpo/pulpo", "Stop", 5);
        assert_eq!(value["hooks"][0]["type"].as_str(), Some("command"));
        assert_eq!(
            value["hooks"][0]["command"].as_str(),
            Some("/opt/pulpo/pulpo hook codex --event Stop")
        );
        assert_eq!(value["hooks"][0]["timeout"].as_integer(), Some(5));
    }

    #[test]
    fn test_hook_entry_value_quotes_path_with_space_for_the_shell() {
        let value = hook_entry_value("/opt/My Pulpo/pulpo", "Stop", 5);
        let command = value["hooks"][0]["command"].as_str().unwrap();
        // Round-trips through shell parsing back to the intended argv — proves the
        // space-containing path is correctly quoted, not split into two arguments.
        let argv = shell_words::split(command).unwrap();
        assert_eq!(
            argv,
            ["/opt/My Pulpo/pulpo", "hook", "codex", "--event", "Stop"]
        );
    }

    // -- parse_existing_config --

    #[test]
    fn test_parse_existing_config_empty_is_empty_table() {
        assert!(parse_existing_config("").is_empty());
        assert!(parse_existing_config("   \n  ").is_empty());
    }

    #[test]
    fn test_parse_existing_config_valid_toml_is_parsed() {
        let table = parse_existing_config("model = \"gpt-5-codex\"\n");
        assert_eq!(table["model"].as_str(), Some("gpt-5-codex"));
    }

    #[test]
    fn test_parse_existing_config_unparseable_warns_and_returns_empty_table() {
        let table = parse_existing_config("this is not [ valid toml");
        assert!(table.is_empty());
    }

    // -- build_config_toml --

    #[test]
    fn test_build_config_toml_no_existing_config() {
        let config = build_config_toml("", "/opt/pulpo/pulpo").unwrap();
        let parsed: toml::Table = toml::from_str(&config).expect("must parse");
        assert_eq!(parsed["notify"].as_array().unwrap().len(), 3);
        for &(event, _) in HOOK_EVENTS {
            assert_eq!(parsed["hooks"][event].as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn test_build_config_toml_replaces_existing_notify_key() {
        // The bug this fixes: naive string concatenation of a second `notify =
        // [...]` line after the user's own produces two `notify` keys in one TOML
        // document, which fails to parse — Codex would refuse to start. A real
        // merge instead ends up with exactly one `notify` key: pulpo's own.
        let existing = "notify = [\"/usr/bin/my-notifier\"]\nmodel = \"gpt-5-codex\"\n";
        let config = build_config_toml(existing, "/opt/pulpo/pulpo").unwrap();

        // Exactly one `notify =` in the raw text — a duplicate key would make this
        // fail to parse at all.
        assert_eq!(config.matches("notify =").count(), 1);

        let parsed: toml::Table = toml::from_str(&config).expect("merged config.toml must parse");
        let notify = parsed["notify"]
            .as_array()
            .expect("notify must be an array");
        assert_eq!(
            notify.len(),
            3,
            "must be pulpo's own notify, not the user's"
        );
        assert!(notify[2].as_str().unwrap().contains("codex-notify"));
        assert_eq!(parsed["model"].as_str(), Some("gpt-5-codex"));
    }

    #[test]
    fn test_build_config_toml_appends_to_existing_hooks_stop_array() {
        let existing = concat!(
            "[[hooks.Stop]]\n",
            "[[hooks.Stop.hooks]]\n",
            "type = \"command\"\n",
            "command = \"user-own-stop-hook\"\n",
            "timeout = 3\n",
        );
        let config = build_config_toml(existing, "/opt/pulpo/pulpo").unwrap();
        let parsed: toml::Table = toml::from_str(&config).expect("merged config.toml must parse");

        let stop_array = parsed["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(
            stop_array.len(),
            2,
            "the user's own Stop hook must be preserved alongside pulpo's"
        );
        let commands: Vec<&str> = stop_array
            .iter()
            .map(|entry| entry["hooks"][0]["command"].as_str().unwrap())
            .collect();
        assert!(commands.contains(&"user-own-stop-hook"));
        assert!(
            commands
                .iter()
                .any(|c| c.contains("hook codex --event Stop"))
        );

        // Every other pulpo hook event is still created even though only Stop
        // pre-existed.
        for &(event, _) in HOOK_EVENTS {
            if event != "Stop" {
                assert_eq!(parsed["hooks"][event].as_array().unwrap().len(), 1);
            }
        }
    }

    #[test]
    fn test_build_config_toml_is_valid_toml_with_expected_shape() {
        // The generated file must actually parse — a syntax error here would
        // silently break Codex entirely for every pulpo-spawned session. The
        // trailing `[mcp_servers.demo]` table is the regression case a naive
        // text-append (rather than a structured merge) would mis-scope pulpo's
        // own keys into.
        let config = build_config_toml(
            "model = \"gpt-5-codex\"\n[mcp_servers.demo]\ncommand = \"demo\"\n",
            "/opt/pulpo/pulpo",
        )
        .unwrap();
        let parsed: toml::Value = toml::from_str(&config).unwrap();
        assert_eq!(parsed["model"].as_str(), Some("gpt-5-codex"));
        assert_eq!(
            parsed["mcp_servers"]["demo"]["command"].as_str(),
            Some("demo")
        );
        assert_eq!(parsed["notify"].as_array().unwrap().len(), 3);
        for &(event, timeout) in HOOK_EVENTS {
            let hook = &parsed["hooks"][event][0]["hooks"][0];
            assert_eq!(hook["type"].as_str(), Some("command"));
            assert_eq!(
                hook["command"].as_str(),
                Some(format!("/opt/pulpo/pulpo hook codex --event {event}").as_str())
            );
            assert_eq!(hook["timeout"].as_integer(), Some(i64::from(timeout)));
        }
    }
}
