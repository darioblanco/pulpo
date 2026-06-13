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
) -> Option<ExactUsage> {
    let since = since - TimeDelta::seconds(SINCE_GRACE_SECS);
    match agent_provider_for_command(command)? {
        "claude.ai" => claude::read_usage(claude_dir, workdir, since),
        "openai" => codex::read_usage(codex_dir, workdir, since, now),
        _ => None,
    }
}

/// Read exact usage for a session using the real home-directory agent paths.
///
/// Gated with `cfg(not(coverage))` because it reads the developer's real `~/.claude`
/// and `~/.codex` directories; the inner readers are fully covered via temp dirs.
#[cfg(not(coverage))]
pub fn read_exact_usage_for_session(session: &Session) -> Option<ExactUsage> {
    let home = dirs::home_dir()?;
    let effective_dir = session.worktree_path.as_deref().unwrap_or(&session.workdir);
    read_exact_usage(
        &session.command,
        effective_dir,
        session.created_at,
        Utc::now(),
        &home.join(".claude"),
        &home.join(".codex"),
    )
}

/// No-op stub under coverage builds (no real filesystem access).
#[cfg(coverage)]
pub fn read_exact_usage_for_session(_session: &Session) -> Option<ExactUsage> {
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
    fn test_read_exact_usage_unknown_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_exact_usage(
            "cargo test",
            "/tmp/repo",
            Utc::now(),
            Utc::now(),
            tmp.path(),
            tmp.path(),
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
