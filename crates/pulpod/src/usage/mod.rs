//! Structured usage readers.
//!
//! Instead of scraping token counts from terminal output, these readers parse the
//! session files that agents write to disk themselves (`~/.claude/projects/*.jsonl`
//! for Claude Code, `~/.codex/sessions/**/rollout-*.jsonl` for Codex). The numbers
//! are exact: per-message token usage straight from the API responses, plus — for
//! Codex — the subscription rate-limit snapshot the agent records.
//!
//! Sessions are mapped to files by working directory and spawn time. The mapping is
//! a heuristic: a second agent started manually in the same directory during the
//! session window would be counted too. The keyword-proximity output scraper in
//! `watchdog::output_patterns` remains the fallback for agents without a reader.

pub mod claude;
pub mod codex;
pub mod pool;
pub mod projection;
pub mod scan;

#[cfg(not(coverage))]
use std::sync::OnceLock;

use chrono::{DateTime, TimeDelta, Utc};
use pulpo_common::session::Session;

use crate::auth_info::agent_provider_for_command;

/// Usage source value for Claude Code JSONL transcripts.
pub const SOURCE_CLAUDE: &str = "claude-jsonl";
/// Usage source value for Codex rollout files.
pub const SOURCE_CODEX: &str = "codex-jsonl";

/// Grace period subtracted from the session spawn time when filtering records,
/// to tolerate small clock differences between pulpod and the agent.
const SINCE_GRACE_SECS: i64 = 60;

/// Exact usage totals for one session, read from the agent's own session files.
#[derive(Debug, Clone, PartialEq)]
pub struct ExactUsage {
    /// Which reader produced the data (`SOURCE_CLAUDE` or `SOURCE_CODEX`).
    pub source: &'static str,
    /// Uncached input tokens.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// Cache-creation input tokens.
    pub cache_write_tokens: u64,
    /// Cache-read input tokens.
    pub cache_read_tokens: u64,
    /// Cost in USD computed from per-model rates. `None` when any record used a
    /// model without a known rate (tokens stay exact; cost would be misleading).
    pub cost_usd: Option<f64>,
    /// Subscription quota snapshot, when the agent records one (Codex only).
    pub quota: Option<QuotaSnapshot>,
}

/// One rate-limit window as recorded by the agent.
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaWindow {
    /// Percent of the window's allowance already used.
    pub used_percent: f64,
    /// Window length in minutes (300 = 5h, 10080 = weekly).
    pub window_minutes: Option<u64>,
    /// Unix timestamp when the window resets.
    pub resets_at: Option<i64>,
}

/// Subscription quota snapshot: short (primary) and long (secondary) windows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QuotaSnapshot {
    pub primary: Option<QuotaWindow>,
    pub secondary: Option<QuotaWindow>,
    pub plan: Option<String>,
}

/// Per-model price table in USD per million tokens.
#[derive(Debug, Clone, Copy)]
pub struct ModelRates {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
}

// Built-in default rates (USD per MTok). This table is a convenience default, not an
// authority — Pulpo is model-agnostic: unknown models still report tokens (cost withheld),
// and a `[rates.<model>]` config override is the planned way to add/reprice models without
// a code change. The Fable row is retained only so sessions priced before Fable's worldwide
// withdrawal (June 2026) still resolve; no new session can use it.
const FABLE_RATES: ModelRates = ModelRates {
    input: 10.0,
    output: 50.0,
    cache_read: 1.0,
    cache_write_5m: 12.5,
    cache_write_1h: 20.0,
};
const OPUS_RATES: ModelRates = ModelRates {
    input: 5.0,
    output: 25.0,
    cache_read: 0.5,
    cache_write_5m: 6.25,
    cache_write_1h: 10.0,
};
const SONNET_RATES: ModelRates = ModelRates {
    input: 3.0,
    output: 15.0,
    cache_read: 0.3,
    cache_write_5m: 3.75,
    cache_write_1h: 6.0,
};
const HAIKU_RATES: ModelRates = ModelRates {
    input: 1.0,
    output: 5.0,
    cache_read: 0.1,
    cache_write_5m: 1.25,
    cache_write_1h: 2.0,
};

/// Look up rates for a model ID by family substring.
/// Returns `None` for unknown models so callers can withhold a misleading cost.
pub fn rates_for_model(model: &str) -> Option<ModelRates> {
    let lower = model.to_lowercase();
    if lower.contains("fable") || lower.contains("mythos") {
        Some(FABLE_RATES)
    } else if lower.contains("opus") {
        Some(OPUS_RATES)
    } else if lower.contains("sonnet") {
        Some(SONNET_RATES)
    } else if lower.contains("haiku") {
        Some(HAIKU_RATES)
    } else {
        None
    }
}

/// User-supplied per-model rate overrides from `[rates.<model>]` config.
///
/// Each key is matched as a case-insensitive substring of the model ID, so a new or
/// repriced model can be priced without a code change — `[rates."claude-opus-4-9"]`
/// matches that exact ID, while `[rates.opus]` reprices the whole family. The most
/// specific (longest) matching key wins, and any override beats the built-in table.
#[derive(Debug, Clone, Default)]
pub struct RateOverrides {
    /// `(lowercased key, rates)`, sorted by key length descending for deterministic
    /// "most specific wins" resolution.
    entries: Vec<(String, ModelRates)>,
}

impl RateOverrides {
    /// Build from `(key, rates)` pairs. Keys are lowercased; order is normalized so
    /// the longest matching key always wins regardless of input order.
    pub fn new(pairs: impl IntoIterator<Item = (String, ModelRates)>) -> Self {
        let mut entries: Vec<(String, ModelRates)> = pairs
            .into_iter()
            .map(|(k, v)| (k.to_lowercase(), v))
            .filter(|(k, _)| !k.is_empty())
            .collect();
        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        Self { entries }
    }

    /// Whether any overrides are configured.
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the rates for the first (most specific) key contained in `model_lower`.
    fn lookup(&self, model_lower: &str) -> Option<ModelRates> {
        self.entries
            .iter()
            .find(|(key, _)| model_lower.contains(key.as_str()))
            .map(|(_, rates)| *rates)
    }
}

/// Resolve rates for a model, preferring config overrides over the built-in table.
/// Returns `None` only when neither knows the model (cost is then withheld).
pub fn resolve_rates(model: &str, overrides: &RateOverrides) -> Option<ModelRates> {
    overrides
        .lookup(&model.to_lowercase())
        .or_else(|| rates_for_model(model))
}

/// Process-wide rate overrides, installed once at startup from config. Read only by
/// the real (filesystem-touching) session reader below, which is coverage-excluded;
/// all pure resolution logic takes an explicit `&RateOverrides` and is fully tested.
#[cfg(not(coverage))]
static RATE_OVERRIDES: OnceLock<RateOverrides> = OnceLock::new();

/// Install the process-wide rate overrides. Call once at daemon startup, before the
/// watchdog begins reading usage. Later calls are ignored.
#[cfg(not(coverage))]
pub fn set_rate_overrides(overrides: RateOverrides) {
    let _ = RATE_OVERRIDES.set(overrides);
}

/// No-op under coverage builds (the global is unused there).
#[cfg(coverage)]
pub fn set_rate_overrides(_overrides: RateOverrides) {}

#[cfg(not(coverage))]
fn active_rate_overrides() -> &'static RateOverrides {
    RATE_OVERRIDES.get_or_init(RateOverrides::default)
}

/// Read exact usage for a session command running in `workdir`, spawned at `since`.
///
/// Dispatches to the reader matching the agent in `command`. Returns `None` when no
/// reader matches or no session files are found — callers fall back to output scraping.
pub fn read_exact_usage(
    command: &str,
    workdir: &str,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
    claude_dir: &std::path::Path,
    codex_dir: &std::path::Path,
    rates: &RateOverrides,
) -> Option<ExactUsage> {
    let since = since - TimeDelta::seconds(SINCE_GRACE_SECS);
    match agent_provider_for_command(command)? {
        "claude.ai" => claude::read_usage(claude_dir, workdir, since, rates),
        // Codex reports exact quota rather than a rate-derived cost, so it ignores rates.
        "openai" => codex::read_usage(codex_dir, workdir, since, now),
        _ => None,
    }
}

/// The directory an agent actually ran in, used to locate its session files.
///
/// Worktree sessions run inside the worktree, so prefer `worktree_path` when set;
/// otherwise the workdir. This must match the cwd the agent records, or usage reads as
/// zero (the readers key project files off the cwd) — hence kept as a small, tested unit.
pub fn effective_usage_dir(session: &Session) -> &str {
    session.worktree_path.as_deref().unwrap_or(&session.workdir)
}

/// Read exact usage for a session using the real home-directory agent paths.
///
/// Gated with `cfg(not(coverage))` because it reads the developer's real `~/.claude`
/// and `~/.codex` directories; the inner readers and [`effective_usage_dir`] are covered.
#[cfg(not(coverage))]
pub fn read_exact_usage_for_session(session: &Session) -> Option<ExactUsage> {
    let home = dirs::home_dir()?;
    read_exact_usage(
        &session.command,
        effective_usage_dir(session),
        session.created_at,
        Utc::now(),
        &home.join(".claude"),
        &home.join(".codex"),
        active_rate_overrides(),
    )
}

/// No-op stub under coverage builds (no real filesystem access).
#[cfg(coverage)]
pub fn read_exact_usage_for_session(_session: &Session) -> Option<ExactUsage> {
    None
}

/// Scan all local agent history using the real home-dir paths (`~/.claude`, `~/.codex`).
///
/// `by_worktree` keeps every directory distinct; the default (`false`) collapses git
/// worktrees and subdirectories onto their origin repo via [`scan::canonical_repo`].
///
/// Coverage-excluded (reads the developer's real home dirs); the scan logic itself is
/// covered via temp dirs in `scan` tests. Returns `None` only when the home dir is
/// unknown — the API handler renders that as an empty scan.
#[cfg(not(coverage))]
pub fn scan_local_usage(
    node_name: &str,
    by_worktree: bool,
) -> Option<pulpo_common::api::UsageScanResponse> {
    let home = dirs::home_dir()?;
    let claude_dir = home.join(".claude");
    let codex_dir = home.join(".codex");
    let rates = active_rate_overrides();
    let now = Utc::now();
    let resp = if by_worktree {
        scan::scan_usage(
            &claude_dir,
            &codex_dir,
            rates,
            node_name,
            now,
            |cwd: &str| cwd.to_owned(),
        )
    } else {
        scan::scan_usage(
            &claude_dir,
            &codex_dir,
            rates,
            node_name,
            now,
            scan::canonical_repo,
        )
    };
    Some(resp)
}

/// No-op stub under coverage builds (no real filesystem access).
#[cfg(coverage)]
pub fn scan_local_usage(
    _node_name: &str,
    _by_worktree: bool,
) -> Option<pulpo_common::api::UsageScanResponse> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_rates_for_model_families() {
        assert_eq!(rates_for_model("claude-fable-5").unwrap().input, 10.0);
        assert_eq!(rates_for_model("claude-mythos-5").unwrap().output, 50.0);
        assert_eq!(rates_for_model("claude-opus-4-8").unwrap().input, 5.0);
        assert_eq!(rates_for_model("claude-sonnet-4-6").unwrap().output, 15.0);
        assert_eq!(rates_for_model("claude-haiku-4-5").unwrap().input, 1.0);
    }

    #[test]
    fn test_rates_for_model_unknown() {
        assert!(rates_for_model("gpt-5").is_none());
        assert!(rates_for_model("").is_none());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_resolve_rates_falls_back_to_builtin() {
        let empty = RateOverrides::default();
        assert!(empty.is_empty());
        assert_eq!(resolve_rates("claude-opus-4-8", &empty).unwrap().input, 5.0);
        assert!(resolve_rates("gpt-5", &empty).is_none());
        assert!(resolve_rates("", &empty).is_none());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_resolve_rates_override_beats_builtin_and_most_specific_wins() {
        let rate = |input: f64| ModelRates {
            input,
            output: 0.0,
            cache_read: 0.0,
            cache_write_5m: 0.0,
            cache_write_1h: 0.0,
        };
        let overrides = RateOverrides::new([
            ("opus".to_owned(), rate(1.0)),
            ("claude-opus-4-8".to_owned(), rate(7.0)),
        ]);
        // Longest matching key wins, regardless of insertion order.
        assert_eq!(
            resolve_rates("claude-opus-4-8", &overrides).unwrap().input,
            7.0
        );
        // The family key still applies to other opus models.
        assert_eq!(
            resolve_rates("claude-opus-4-9", &overrides).unwrap().input,
            1.0
        );
        // No matching key and no built-in → withheld.
        assert!(resolve_rates("gpt-5", &overrides).is_none());
    }

    #[test]
    fn test_rate_overrides_new_drops_empty_keys_and_lowercases() {
        let overrides = RateOverrides::new([
            (String::new(), HAIKU_RATES),
            ("GPT-6".to_owned(), HAIKU_RATES),
        ]);
        assert!(!overrides.is_empty());
        // Empty key was dropped; non-empty key matches case-insensitively.
        assert!(resolve_rates("gpt-6-turbo", &overrides).is_some());
    }

    #[test]
    fn test_set_rate_overrides_is_callable() {
        // Smoke test for the public setter (no-op under coverage builds).
        set_rate_overrides(RateOverrides::new([("zzz-smoke".to_owned(), HAIKU_RATES)]));
    }

    #[test]
    fn test_effective_usage_dir_prefers_worktree() {
        // A worktree session's agent runs inside the worktree → read usage from there.
        let session = Session {
            workdir: "/repo".into(),
            worktree_path: Some("/home/u/.pulpo/worktrees/fix".into()),
            ..Default::default()
        };
        assert_eq!(
            effective_usage_dir(&session),
            "/home/u/.pulpo/worktrees/fix"
        );

        // No worktree → the plain workdir.
        let plain = Session {
            workdir: "/repo".into(),
            worktree_path: None,
            ..Default::default()
        };
        assert_eq!(effective_usage_dir(&plain), "/repo");
    }

    #[test]
    fn test_read_exact_usage_unknown_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_exact_usage(
            "cargo test",
            "/tmp/repo",
            Utc::now(),
            Utc::now(),
            tmp.path(),
            tmp.path(),
            &RateOverrides::default(),
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_read_exact_usage_gemini_has_no_reader() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_exact_usage(
            "gemini chat",
            "/tmp/repo",
            Utc::now(),
            Utc::now(),
            tmp.path(),
            tmp.path(),
            &RateOverrides::default(),
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_read_exact_usage_claude_without_files() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_exact_usage(
            "claude -p 'fix'",
            "/tmp/repo",
            Utc::now(),
            Utc::now(),
            tmp.path(),
            tmp.path(),
            &RateOverrides::default(),
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_read_exact_usage_codex_without_files() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_exact_usage(
            "codex exec 'fix'",
            "/tmp/repo",
            Utc::now(),
            Utc::now(),
            tmp.path(),
            tmp.path(),
            &RateOverrides::default(),
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_read_exact_usage_for_session_no_agent_command() {
        let session = Session {
            command: "cargo build".into(),
            ..Default::default()
        };
        assert!(read_exact_usage_for_session(&session).is_none());
    }
}
