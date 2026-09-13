//! Per-session exact usage and per-repo/worktree cost rollups for pulpo-managed sessions.
//!
//! Unlike [`super::scan`] (which reads every local agent history file, pulpo-managed or
//! not), this operates on the sessions pulpo itself is tracking — the exact token/cost
//! totals the watchdog keeps fresh in session metadata via the structured usage readers
//! (see `watchdog::metadata::store_exact_usage`). There is no burn-rate/forecasting math
//! here: just what each session has cost so far, and where.

use std::collections::BTreeMap;

use pulpo_common::api::{DimensionRollup, SessionUsage};
use pulpo_common::session::{Session, meta};

/// Sum of every token dimension tracked for a session.
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

/// Build the exact-usage view of one session from its stored metadata.
pub fn session_usage(session: &Session) -> SessionUsage {
    SessionUsage {
        session_id: session.id.to_string(),
        session_name: session.name.clone(),
        workdir: session.workdir.clone(),
        usage_source: session.meta_str(meta::USAGE_SOURCE).map(str::to_owned),
        total_tokens: total_tokens(session),
        cost_usd: session.meta_parsed::<f64>(meta::SESSION_COST_USD),
    }
}

/// Per-repo rollups — grouped by the session's workdir (sessions with no workdir are
/// skipped). Sorted by total cost descending, then label.
pub fn build_repo_rollups(usages: &[SessionUsage]) -> Vec<DimensionRollup> {
    struct Acc {
        session_count: u32,
        total_tokens: u64,
        total_cost_usd: Option<f64>,
    }

    let mut groups: BTreeMap<String, Acc> = BTreeMap::new();
    for u in usages {
        if u.workdir.is_empty() {
            continue;
        }
        let acc = groups.entry(u.workdir.clone()).or_insert_with(|| Acc {
            session_count: 0,
            total_tokens: 0,
            total_cost_usd: None,
        });
        acc.session_count += 1;
        acc.total_tokens += u.total_tokens;
        if let Some(cost) = u.cost_usd {
            acc.total_cost_usd = Some(acc.total_cost_usd.unwrap_or(0.0) + cost);
        }
    }

    let mut rollups: Vec<DimensionRollup> = groups
        .into_iter()
        .map(|(label, a)| DimensionRollup {
            label,
            session_count: a.session_count,
            total_tokens: a.total_tokens,
            total_cost_usd: a.total_cost_usd,
        })
        .collect();
    // Most expensive first; stable tie-break by label.
    rollups.sort_by(|a, b| {
        let (ac, bc) = (
            a.total_cost_usd.unwrap_or(0.0),
            b.total_cost_usd.unwrap_or(0.0),
        );
        bc.partial_cmp(&ac)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.label.cmp(&b.label))
    });
    rollups
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn session_with(meta_pairs: &[(&str, &str)], workdir: &str) -> Session {
        let mut metadata = HashMap::new();
        for (k, v) in meta_pairs {
            metadata.insert((*k).to_owned(), (*v).to_owned());
        }
        Session {
            id: Uuid::nil(),
            name: "sess".into(),
            workdir: workdir.into(),
            metadata: Some(metadata),
            ..Default::default()
        }
    }

    #[test]
    fn test_session_usage_reads_exact_metadata() {
        let session = session_with(
            &[
                (meta::USAGE_SOURCE, "claude-jsonl"),
                (meta::TOTAL_INPUT_TOKENS, "2000"),
                (meta::TOTAL_OUTPUT_TOKENS, "1000"),
                (meta::CACHE_READ_TOKENS, "600"),
                (meta::SESSION_COST_USD, "1.800000"),
            ],
            "/repos/api",
        );
        let usage = session_usage(&session);
        assert_eq!(usage.session_name, "sess");
        assert_eq!(usage.workdir, "/repos/api");
        assert_eq!(usage.usage_source.as_deref(), Some("claude-jsonl"));
        assert_eq!(usage.total_tokens, 3600);
        assert_eq!(usage.cost_usd, Some(1.8));
    }

    #[test]
    fn test_session_usage_no_metadata() {
        let session = session_with(&[], "/repos/api");
        let usage = session_usage(&session);
        assert_eq!(usage.total_tokens, 0);
        assert_eq!(usage.cost_usd, None);
        assert_eq!(usage.usage_source, None);
    }

    fn usage(workdir: &str, tokens: u64, cost: Option<f64>) -> SessionUsage {
        SessionUsage {
            session_id: "id".into(),
            session_name: "s".into(),
            workdir: workdir.into(),
            usage_source: Some("claude-jsonl".into()),
            total_tokens: tokens,
            cost_usd: cost,
        }
    }

    #[test]
    fn test_build_repo_rollups_groups_by_workdir() {
        let rollups = build_repo_rollups(&[
            usage("/repos/api", 50, Some(4.0)),
            usage("/repos/api", 50, Some(1.0)),
            usage("/repos/web", 50, Some(2.0)),
        ]);
        assert_eq!(rollups.len(), 2);
        assert_eq!(rollups[0].label, "/repos/api");
        assert_eq!(rollups[0].session_count, 2);
        assert_eq!(rollups[0].total_tokens, 100);
        assert!((rollups[0].total_cost_usd.unwrap() - 5.0).abs() < 1e-9);
        assert_eq!(rollups[1].label, "/repos/web");
    }

    #[test]
    fn test_build_repo_rollups_skips_empty_workdir() {
        let rollups = build_repo_rollups(&[usage("", 10, Some(1.0))]);
        assert!(rollups.is_empty());
    }

    #[test]
    fn test_build_repo_rollups_empty() {
        assert!(build_repo_rollups(&[]).is_empty());
    }

    #[test]
    fn test_build_repo_rollups_no_cost_sessions() {
        let rollups = build_repo_rollups(&[usage("/repos/api", 10, None)]);
        assert_eq!(rollups.len(), 1);
        assert_eq!(rollups[0].total_cost_usd, None);
    }
}
