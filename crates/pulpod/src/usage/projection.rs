//! Usage projection: burn rate and time-to-wall from the exact totals Phase A keeps fresh.
//!
//! Burn rate is the **session-lifetime average** (cumulative usage ÷ session age), not an
//! instantaneous delta. The watchdog tick is ~10s, so a two-tick delta would be far too
//! noisy to extrapolate into a $/hr figure; the lifetime average is stable and is the
//! right basis for "at this rate, when do I hit the wall."
//!
//! Honesty split (the load-bearing caveat of this whole phase):
//! - **Codex** quota is exact — the agent records `used_percent` + `resets_at`, surfaced
//!   verbatim, no estimation.
//! - **Claude** has no published token allowance, so "% of cap" / time-to-cap are computed
//!   only when the operator configures a `[plans]` allowance, and are labelled estimated.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use pulpo_common::api::{AccountRollup, SessionProjection};
use pulpo_common::session::{Session, meta};

/// Per-hour rate from a cumulative total over an elapsed span.
/// Returns `None` when the span is non-positive (can't divide) — callers render "—".
#[allow(clippy::cast_precision_loss)]
pub fn per_hour(total: f64, elapsed_secs: i64) -> Option<f64> {
    if elapsed_secs <= 0 {
        return None;
    }
    Some(total / (elapsed_secs as f64 / 3600.0))
}

/// Seconds until `current` reaches `threshold` growing at `rate_per_hour`.
/// `None` if the rate is non-positive (never reaches it) or it's already at/over.
#[allow(clippy::cast_possible_truncation)]
pub fn secs_to_threshold(current: f64, threshold: f64, rate_per_hour: f64) -> Option<i64> {
    if rate_per_hour <= 0.0 || current >= threshold {
        return None;
    }
    Some(((threshold - current) / rate_per_hour * 3600.0) as i64)
}

/// Sum of every token dimension we track for a session.
fn total_tokens(session: &Session) -> u64 {
    [
        meta::TOTAL_INPUT_TOKENS,
        meta::TOTAL_OUTPUT_TOKENS,
        meta::CACHE_WRITE_TOKENS,
        meta::CACHE_READ_TOKENS,
    ]
    .iter()
    .filter_map(|key| session.meta_parsed::<u64>(key))
    .sum()
}

/// Build a projection for one session as of `now`.
///
/// `allowance_tokens` is the operator-configured weekly token allowance for this session's
/// plan (`None` disables Claude %-of-cap). Burn rate uses the session's lifetime span.
#[allow(clippy::cast_precision_loss)]
pub fn project_session(
    session: &Session,
    now: DateTime<Utc>,
    allowance_tokens: Option<u64>,
) -> SessionProjection {
    let elapsed_secs = (now - session.created_at).num_seconds();
    let tokens = total_tokens(session);
    let cost_usd = session.meta_parsed::<f64>(meta::SESSION_COST_USD);

    let tokens_per_hour = per_hour(tokens as f64, elapsed_secs);
    let cost_per_hour = cost_usd.and_then(|c| per_hour(c, elapsed_secs));

    // Claude estimated %-of-allowance (only when an allowance is configured).
    let (allowance_used_percent, secs_to_allowance) = match allowance_tokens {
        Some(allowance) if allowance > 0 => {
            let used = tokens as f64 / allowance as f64 * 100.0;
            let to_cap = tokens_per_hour
                .and_then(|rate| secs_to_threshold(tokens as f64, allowance as f64, rate));
            (Some(used), to_cap)
        }
        _ => (None, None),
    };

    SessionProjection {
        session_id: session.id.to_string(),
        session_name: session.name.clone(),
        usage_source: session.meta_str(meta::USAGE_SOURCE).map(str::to_owned),
        auth_provider: session.meta_str(meta::AUTH_PROVIDER).map(str::to_owned),
        auth_plan: session.meta_str(meta::AUTH_PLAN).map(str::to_owned),
        auth_email: session.meta_str(meta::AUTH_EMAIL).map(str::to_owned),
        total_tokens: tokens,
        cost_usd,
        elapsed_secs,
        cost_per_hour,
        tokens_per_hour,
        quota_used_percent: session.meta_parsed::<f64>(meta::QUOTA_PRIMARY_USED_PERCENT),
        quota_resets_at: session.meta_parsed::<i64>(meta::QUOTA_PRIMARY_RESETS_AT),
        allowance_tokens,
        allowance_used_percent,
        secs_to_allowance,
    }
}

/// Account identity key: (provider, plan, email).
type AccountKey = (Option<String>, Option<String>, Option<String>);

/// Group per-session projections into per-account rollups (provider + plan + email).
///
/// Cost fields aggregate only the sessions that have a cost (Codex contributes tokens and
/// quota but no cost); `total_cost_usd` is `None` when no session in the group had one.
#[allow(clippy::cast_possible_truncation)]
pub fn build_rollups(projections: &[SessionProjection]) -> Vec<AccountRollup> {
    struct Acc {
        provider: Option<String>,
        plan: Option<String>,
        email: Option<String>,
        session_count: u32,
        total_tokens: u64,
        total_cost_usd: Option<f64>,
        cost_per_hour: Option<f64>,
        max_quota_used_percent: Option<f64>,
    }

    let mut groups: BTreeMap<AccountKey, Acc> = BTreeMap::new();

    for p in projections {
        let key = (
            p.auth_provider.clone(),
            p.auth_plan.clone(),
            p.auth_email.clone(),
        );
        let acc = groups.entry(key).or_insert_with(|| Acc {
            provider: p.auth_provider.clone(),
            plan: p.auth_plan.clone(),
            email: p.auth_email.clone(),
            session_count: 0,
            total_tokens: 0,
            total_cost_usd: None,
            cost_per_hour: None,
            max_quota_used_percent: None,
        });
        acc.session_count += 1;
        acc.total_tokens += p.total_tokens;
        if let Some(cost) = p.cost_usd {
            acc.total_cost_usd = Some(acc.total_cost_usd.unwrap_or(0.0) + cost);
        }
        if let Some(rate) = p.cost_per_hour {
            acc.cost_per_hour = Some(acc.cost_per_hour.unwrap_or(0.0) + rate);
        }
        if let Some(pct) = p.quota_used_percent {
            acc.max_quota_used_percent =
                Some(acc.max_quota_used_percent.map_or(pct, |m| m.max(pct)));
        }
    }

    groups
        .into_values()
        .map(|a| AccountRollup {
            provider: a.provider,
            plan: a.plan,
            email: a.email,
            session_count: a.session_count,
            total_tokens: a.total_tokens,
            total_cost_usd: a.total_cost_usd,
            cost_per_hour: a.cost_per_hour,
            max_quota_used_percent: a.max_quota_used_percent,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use pulpo_common::session::SessionStatus;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn session_with(meta_pairs: &[(&str, &str)], age: TimeDelta) -> (Session, DateTime<Utc>) {
        let now = DateTime::parse_from_rfc3339("2026-06-13T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut metadata = HashMap::new();
        for (k, v) in meta_pairs {
            metadata.insert((*k).to_owned(), (*v).to_owned());
        }
        let session = Session {
            id: Uuid::nil(),
            name: "proj".into(),
            status: SessionStatus::Active,
            created_at: now - age,
            metadata: Some(metadata),
            ..Default::default()
        };
        (session, now)
    }

    #[test]
    fn test_per_hour_basic() {
        // 10 units over 30 min → 20/hr
        assert_eq!(per_hour(10.0, 1800), Some(20.0));
    }

    #[test]
    fn test_per_hour_zero_and_negative_elapsed() {
        assert_eq!(per_hour(10.0, 0), None);
        assert_eq!(per_hour(10.0, -5), None);
    }

    #[test]
    fn test_secs_to_threshold_basic() {
        // at 50, target 100, rate 50/hr → 1h = 3600s
        assert_eq!(secs_to_threshold(50.0, 100.0, 50.0), Some(3600));
    }

    #[test]
    fn test_secs_to_threshold_already_past() {
        assert_eq!(secs_to_threshold(120.0, 100.0, 50.0), None);
    }

    #[test]
    fn test_secs_to_threshold_zero_rate() {
        assert_eq!(secs_to_threshold(10.0, 100.0, 0.0), None);
    }

    #[test]
    fn test_project_claude_cost_and_token_rate() {
        // 1h old; 3600 tokens, $1.80 → 3600 tok/hr, $1.80/hr
        let (session, now) = session_with(
            &[
                (meta::USAGE_SOURCE, "claude-jsonl"),
                (meta::TOTAL_INPUT_TOKENS, "2000"),
                (meta::TOTAL_OUTPUT_TOKENS, "1000"),
                (meta::CACHE_READ_TOKENS, "600"),
                (meta::SESSION_COST_USD, "1.800000"),
                (meta::AUTH_PROVIDER, "claude.ai"),
                (meta::AUTH_PLAN, "max"),
            ],
            TimeDelta::hours(1),
        );
        let p = project_session(&session, now, None);
        assert_eq!(p.total_tokens, 3600);
        assert_eq!(p.cost_usd, Some(1.8));
        assert_eq!(p.tokens_per_hour, Some(3600.0));
        assert_eq!(p.cost_per_hour, Some(1.8));
        // No allowance configured → no Claude %-of-cap.
        assert_eq!(p.allowance_used_percent, None);
        assert_eq!(p.secs_to_allowance, None);
    }

    #[test]
    fn test_project_claude_with_allowance() {
        // 3600 tokens over 1h, allowance 36000 → 10% used, 9h to cap (32400s)
        let (session, now) = session_with(
            &[
                (meta::TOTAL_INPUT_TOKENS, "3600"),
                (meta::SESSION_COST_USD, "1.0"),
            ],
            TimeDelta::hours(1),
        );
        let p = project_session(&session, now, Some(36_000));
        assert_eq!(p.allowance_tokens, Some(36_000));
        assert!((p.allowance_used_percent.unwrap() - 10.0).abs() < 1e-9);
        assert_eq!(p.secs_to_allowance, Some(32_400));
    }

    #[test]
    fn test_project_codex_quota_passthrough_no_cost() {
        let (session, now) = session_with(
            &[
                (meta::USAGE_SOURCE, "codex-jsonl"),
                (meta::TOTAL_INPUT_TOKENS, "5000"),
                (meta::TOTAL_OUTPUT_TOKENS, "1000"),
                (meta::QUOTA_PRIMARY_USED_PERCENT, "42.5"),
                (meta::QUOTA_PRIMARY_RESETS_AT, "1775073678"),
                (meta::AUTH_PROVIDER, "openai"),
            ],
            TimeDelta::hours(2),
        );
        let p = project_session(&session, now, None);
        assert_eq!(p.total_tokens, 6000);
        assert_eq!(p.cost_usd, None);
        assert_eq!(p.cost_per_hour, None);
        assert_eq!(p.tokens_per_hour, Some(3000.0));
        assert_eq!(p.quota_used_percent, Some(42.5));
        assert_eq!(p.quota_resets_at, Some(1_775_073_678));
    }

    #[test]
    fn test_project_brand_new_session_no_rate() {
        // age 0 → rates None, but totals still reported
        let (session, now) = session_with(&[(meta::TOTAL_INPUT_TOKENS, "100")], TimeDelta::zero());
        let p = project_session(&session, now, Some(1000));
        assert_eq!(p.total_tokens, 100);
        assert_eq!(p.tokens_per_hour, None);
        assert_eq!(p.cost_per_hour, None);
        // %-used is still computable without a rate; time-to-cap is not.
        assert!((p.allowance_used_percent.unwrap() - 10.0).abs() < 1e-9);
        assert_eq!(p.secs_to_allowance, None);
    }

    #[test]
    fn test_project_no_usage_metadata() {
        let (session, now) = session_with(&[], TimeDelta::hours(1));
        let p = project_session(&session, now, None);
        assert_eq!(p.total_tokens, 0);
        assert_eq!(p.cost_usd, None);
        assert_eq!(p.tokens_per_hour, Some(0.0));
        assert_eq!(p.usage_source, None);
    }

    #[allow(clippy::cast_precision_loss)]
    fn proj(
        provider: &str,
        email: &str,
        tokens: u64,
        cost: Option<f64>,
        quota: Option<f64>,
    ) -> SessionProjection {
        SessionProjection {
            session_id: "id".into(),
            session_name: "s".into(),
            usage_source: None,
            auth_provider: Some(provider.into()),
            auth_plan: Some("max".into()),
            auth_email: Some(email.into()),
            total_tokens: tokens,
            cost_usd: cost,
            elapsed_secs: 3600,
            cost_per_hour: cost,
            tokens_per_hour: Some(tokens as f64),
            quota_used_percent: quota,
            quota_resets_at: None,
            allowance_tokens: None,
            allowance_used_percent: None,
            secs_to_allowance: None,
        }
    }

    #[test]
    fn test_build_rollups_groups_by_account() {
        let rollups = build_rollups(&[
            proj("claude.ai", "a@x.com", 100, Some(1.0), None),
            proj("claude.ai", "a@x.com", 200, Some(2.0), None),
            proj("openai", "b@y.com", 50, None, Some(30.0)),
        ]);
        assert_eq!(rollups.len(), 2);
        let claude = rollups
            .iter()
            .find(|r| r.email.as_deref() == Some("a@x.com"))
            .unwrap();
        assert_eq!(claude.session_count, 2);
        assert_eq!(claude.total_tokens, 300);
        assert!((claude.total_cost_usd.unwrap() - 3.0).abs() < 1e-9);
        assert!((claude.cost_per_hour.unwrap() - 3.0).abs() < 1e-9);
        assert_eq!(claude.max_quota_used_percent, None);

        let codex = rollups
            .iter()
            .find(|r| r.email.as_deref() == Some("b@y.com"))
            .unwrap();
        assert_eq!(codex.session_count, 1);
        assert_eq!(codex.total_cost_usd, None);
        assert_eq!(codex.max_quota_used_percent, Some(30.0));
    }

    #[test]
    fn test_build_rollups_max_quota_across_sessions() {
        let rollups = build_rollups(&[
            proj("openai", "b@y.com", 10, None, Some(12.0)),
            proj("openai", "b@y.com", 10, None, Some(47.0)),
            proj("openai", "b@y.com", 10, None, None),
        ]);
        assert_eq!(rollups.len(), 1);
        assert_eq!(rollups[0].max_quota_used_percent, Some(47.0));
        assert_eq!(rollups[0].session_count, 3);
    }

    #[test]
    fn test_build_rollups_empty() {
        assert!(build_rollups(&[]).is_empty());
    }
}
