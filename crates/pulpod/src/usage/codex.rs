//! Codex rollout-file reader.
//!
//! Codex writes one JSONL rollout file per session under
//! `<codex_dir>/sessions/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl`. The leading
//! `session_meta` record carries the working directory; `token_count` events carry
//! cumulative token totals plus the subscription rate-limit snapshot.

use std::path::Path;

use chrono::{DateTime, Datelike, TimeDelta, Utc};

use super::{ExactUsage, QuotaSnapshot, QuotaWindow, SOURCE_CODEX};

/// Upper bound on the number of day-directories walked, so a stale session row
/// can't turn every watchdog tick into a multi-year directory scan.
const MAX_DAYS_SCANNED: u32 = 92;

/// How many leading lines to scan for the `session_meta` record.
const META_SCAN_LINES: usize = 10;

/// Cumulative token totals from a rollout file's last `token_count` event.
#[derive(Debug, Default, Clone, Copy)]
struct FileTotals {
    input: u64,
    cached: u64,
    output: u64,
}

/// Strip trailing slashes so `/repo` and `/repo/` compare equal.
fn normalize_dir(dir: &str) -> &str {
    let trimmed = dir.trim_end_matches('/');
    if trimmed.is_empty() { "/" } else { trimmed }
}

/// Parse a `session_meta` record into `(cwd, started_at)`.
fn parse_session_meta(value: &serde_json::Value) -> Option<(String, DateTime<Utc>)> {
    if value.get("type").and_then(serde_json::Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = value.get("payload")?;
    let cwd = payload.get("cwd")?.as_str()?.to_owned();
    let started_at = payload
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())?
        .with_timezone(&Utc);
    Some((cwd, started_at))
}

/// Parse one rate-limit window object.
fn parse_quota_window(value: &serde_json::Value) -> Option<QuotaWindow> {
    Some(QuotaWindow {
        used_percent: value.get("used_percent")?.as_f64()?,
        window_minutes: value
            .get("window_minutes")
            .and_then(serde_json::Value::as_u64),
        resets_at: value.get("resets_at").and_then(serde_json::Value::as_i64),
    })
}

/// Parse a `token_count` event into its cumulative totals and quota snapshot.
/// Either part may be absent (`info` is null on pure rate-limit updates).
fn parse_token_count(value: &serde_json::Value) -> (Option<FileTotals>, Option<QuotaSnapshot>) {
    let Some(payload) = value.get("payload") else {
        return (None, None);
    };
    if payload.get("type").and_then(serde_json::Value::as_str) != Some("token_count") {
        return (None, None);
    }

    let totals = payload
        .get("info")
        .and_then(|info| info.get("total_token_usage"))
        .map(|usage| FileTotals {
            input: usage
                .get("input_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            cached: usage
                .get("cached_input_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            output: usage
                .get("output_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        });

    let quota = payload.get("rate_limits").map(|limits| QuotaSnapshot {
        primary: limits.get("primary").and_then(parse_quota_window),
        secondary: limits.get("secondary").and_then(parse_quota_window),
        plan: limits
            .get("plan_type")
            .and_then(serde_json::Value::as_str)
            .map(String::from),
    });

    (totals, quota)
}

/// Read one rollout file. Returns the file's last cumulative totals and quota
/// snapshot when its `session_meta` matches `workdir` and started at or after
/// `since`; `None` otherwise.
fn read_rollout_file(
    path: &Path,
    workdir: &str,
    since: DateTime<Utc>,
) -> Option<(FileTotals, Option<QuotaSnapshot>)> {
    let rollout = parse_rollout(path)?;
    if normalize_dir(&rollout.cwd) != workdir || rollout.started_at < since {
        return None;
    }
    Some((rollout.totals, rollout.quota))
}

/// One parsed Codex rollout file: its `cwd`, start time, last cumulative totals, and
/// latest quota snapshot — with no workdir/time filtering. [`read_rollout_file`] adds the
/// filter; the usage scan groups by `cwd` instead.
struct Rollout {
    cwd: String,
    started_at: DateTime<Utc>,
    totals: FileTotals,
    quota: Option<QuotaSnapshot>,
}

fn parse_rollout(path: &Path) -> Option<Rollout> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines = content.lines();

    let (cwd, started_at) = lines
        .by_ref()
        .take(META_SCAN_LINES)
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| parse_session_meta(&value))?;

    let mut last_totals = FileTotals::default();
    let mut last_quota = None;
    for line in lines {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let (totals, quota) = parse_token_count(&value);
        if let Some(totals) = totals {
            last_totals = totals;
        }
        if quota.is_some() {
            last_quota = quota;
        }
    }
    Some(Rollout {
        cwd,
        started_at,
        totals: last_totals,
        quota: last_quota,
    })
}

/// Total Codex tokens per repo across *all* rollout files, keyed by normalized `cwd`.
/// Token total matches the `ExactUsage` convention: `(input − cached) + output + cached`.
/// Used by the usage scan (which groups by repo rather than matching one workdir).
pub(crate) fn scan_by_cwd(codex_dir: &Path) -> std::collections::HashMap<String, u64> {
    let mut by_cwd: std::collections::HashMap<String, FileTotals> =
        std::collections::HashMap::new();
    for path in collect_rollout_files(codex_dir) {
        if let Some(rollout) = parse_rollout(&path) {
            let entry = by_cwd
                .entry(normalize_dir(&rollout.cwd).to_owned())
                .or_default();
            // Each rollout file is one agent process; sum their last cumulative totals.
            entry.input += rollout.totals.input;
            entry.cached += rollout.totals.cached;
            entry.output += rollout.totals.output;
        }
    }
    by_cwd
        .into_iter()
        .map(|(cwd, t)| (cwd, t.input.saturating_sub(t.cached) + t.output + t.cached))
        .collect()
}

/// Recursively collect `rollout-*.jsonl` files under `<codex_dir>/sessions`.
fn collect_rollout_files(codex_dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![codex_dir.join("sessions")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("rollout-"))
            {
                out.push(path);
            }
        }
    }
    out
}

/// Read exact usage for a Codex session running in `workdir`, started at `since`.
///
/// Walks the day directories from `since` to `now`, matches rollout files by
/// `session_meta.cwd` and start time, and sums the last cumulative totals of each
/// matching file (one file per agent process; restarts produce new files). The
/// quota snapshot comes from the latest matching file that recorded one.
pub fn read_usage(
    codex_dir: &Path,
    workdir: &str,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<ExactUsage> {
    let sessions_dir = codex_dir.join("sessions");
    let workdir = normalize_dir(workdir);

    let mut totals = FileTotals::default();
    let mut quota = None;
    let mut matched = false;

    let mut date = since.date_naive();
    let end = now.date_naive();
    let mut days_scanned: u32 = 0;
    while date <= end && days_scanned < MAX_DAYS_SCANNED {
        let day_dir = sessions_dir
            .join(format!("{:04}", date.year()))
            .join(format!("{:02}", date.month()))
            .join(format!("{:02}", date.day()));
        if let Ok(entries) = std::fs::read_dir(&day_dir) {
            let mut files: Vec<_> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
                        && path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with("rollout-"))
                })
                .collect();
            files.sort();
            for path in files {
                if let Some((file_totals, file_quota)) = read_rollout_file(&path, workdir, since) {
                    matched = true;
                    totals.input += file_totals.input;
                    totals.cached += file_totals.cached;
                    totals.output += file_totals.output;
                    if file_quota.is_some() {
                        quota = file_quota;
                    }
                }
            }
        }
        date = date.checked_add_signed(TimeDelta::days(1))?;
        days_scanned += 1;
    }

    if !matched {
        return None;
    }
    Some(ExactUsage {
        source: SOURCE_CODEX,
        input_tokens: totals.input.saturating_sub(totals.cached),
        output_tokens: totals.output,
        cache_write_tokens: 0,
        cache_read_tokens: totals.cached,
        // Codex sessions run on OpenAI plans; no reliable per-token rate table here.
        cost_usd: None,
        quota,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn session_meta_line(timestamp: &str, cwd: &str) -> String {
        format!(
            r#"{{"timestamp":"{timestamp}","type":"session_meta","payload":{{"id":"abc","timestamp":"{timestamp}","cwd":"{cwd}","originator":"codex_cli_rs"}}}}"#
        )
    }

    fn token_count_line(input: u64, cached: u64, output: u64) -> String {
        format!(
            r#"{{"timestamp":"2026-06-12T10:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output},"total_tokens":{}}}}},"rate_limits":{{"limit_id":"codex","primary":{{"used_percent":12.5,"window_minutes":300,"resets_at":1775073678}},"secondary":{{"used_percent":3.0,"window_minutes":10080,"resets_at":1775660478}},"plan_type":"plus"}}}}}}"#,
            input + output
        )
    }

    fn write_rollout(codex_dir: &Path, date: DateTime<Utc>, name: &str, content: &str) {
        let day_dir = codex_dir
            .join("sessions")
            .join(format!("{:04}", date.year()))
            .join(format!("{:02}", date.month()))
            .join(format!("{:02}", date.day()));
        fs::create_dir_all(&day_dir).unwrap();
        fs::write(day_dir.join(name), content).unwrap();
    }

    #[test]
    fn test_normalize_dir() {
        assert_eq!(normalize_dir("/repo/"), "/repo");
        assert_eq!(normalize_dir("/repo"), "/repo");
        assert_eq!(normalize_dir("/"), "/");
        assert_eq!(normalize_dir("///"), "/");
    }

    #[test]
    fn test_read_usage_missing_sessions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_usage(tmp.path(), "/repo", Utc::now(), Utc::now()).is_none());
    }

    #[test]
    fn test_read_usage_matches_cwd_and_reads_last_totals() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::hours(1);
        let ts = now.to_rfc3339();
        let content = format!(
            "{}\n{}\n{}\n",
            session_meta_line(&ts, "/repo"),
            token_count_line(1000, 200, 50),
            token_count_line(5000, 1000, 400),
        );
        write_rollout(tmp.path(), now, "rollout-2026-06-12-abc.jsonl", &content);

        let usage = read_usage(tmp.path(), "/repo", since, now).unwrap();
        assert_eq!(usage.source, SOURCE_CODEX);
        // Last cumulative totals win: input excludes cached reads.
        assert_eq!(usage.input_tokens, 4000);
        assert_eq!(usage.cache_read_tokens, 1000);
        assert_eq!(usage.output_tokens, 400);
        assert!(usage.cost_usd.is_none());
    }

    #[test]
    fn test_read_usage_extracts_quota_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::hours(1);
        let ts = now.to_rfc3339();
        let content = format!(
            "{}\n{}\n",
            session_meta_line(&ts, "/repo"),
            token_count_line(100, 0, 10),
        );
        write_rollout(tmp.path(), now, "rollout-2026-06-12-abc.jsonl", &content);

        let usage = read_usage(tmp.path(), "/repo", since, now).unwrap();
        let quota = usage.quota.unwrap();
        let primary = quota.primary.unwrap();
        assert!((primary.used_percent - 12.5).abs() < 1e-9);
        assert_eq!(primary.window_minutes, Some(300));
        assert_eq!(primary.resets_at, Some(1_775_073_678));
        let secondary = quota.secondary.unwrap();
        assert_eq!(secondary.window_minutes, Some(10_080));
        assert_eq!(quota.plan.as_deref(), Some("plus"));
    }

    #[test]
    fn test_read_usage_skips_other_workdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::hours(1);
        let ts = now.to_rfc3339();
        let content = format!(
            "{}\n{}\n",
            session_meta_line(&ts, "/other"),
            token_count_line(100, 0, 10),
        );
        write_rollout(tmp.path(), now, "rollout-2026-06-12-abc.jsonl", &content);

        assert!(read_usage(tmp.path(), "/repo", since, now).is_none());
    }

    #[test]
    fn test_read_usage_skips_sessions_started_before_since() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::hours(1);
        let old_ts = (now - TimeDelta::hours(5)).to_rfc3339();
        let content = format!(
            "{}\n{}\n",
            session_meta_line(&old_ts, "/repo"),
            token_count_line(100, 0, 10),
        );
        write_rollout(tmp.path(), now, "rollout-2026-06-12-abc.jsonl", &content);

        assert!(read_usage(tmp.path(), "/repo", since, now).is_none());
    }

    #[test]
    fn test_read_usage_sums_multiple_matching_files() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::hours(1);
        let ts = now.to_rfc3339();
        write_rollout(
            tmp.path(),
            now,
            "rollout-a.jsonl",
            &format!(
                "{}\n{}\n",
                session_meta_line(&ts, "/repo"),
                token_count_line(1000, 100, 50)
            ),
        );
        write_rollout(
            tmp.path(),
            now,
            "rollout-b.jsonl",
            &format!(
                "{}\n{}\n",
                session_meta_line(&ts, "/repo"),
                token_count_line(2000, 200, 70)
            ),
        );

        let usage = read_usage(tmp.path(), "/repo", since, now).unwrap();
        assert_eq!(usage.input_tokens, 2700);
        assert_eq!(usage.cache_read_tokens, 300);
        assert_eq!(usage.output_tokens, 120);
    }

    #[test]
    fn test_read_usage_walks_multiple_days() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::days(1);
        let yesterday = now - TimeDelta::days(1);
        write_rollout(
            tmp.path(),
            yesterday,
            "rollout-a.jsonl",
            &format!(
                "{}\n{}\n",
                session_meta_line(&yesterday.to_rfc3339(), "/repo"),
                token_count_line(100, 0, 10)
            ),
        );
        write_rollout(
            tmp.path(),
            now,
            "rollout-b.jsonl",
            &format!(
                "{}\n{}\n",
                session_meta_line(&now.to_rfc3339(), "/repo"),
                token_count_line(200, 0, 20)
            ),
        );

        let usage = read_usage(tmp.path(), "/repo", since, now).unwrap();
        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 30);
    }

    #[test]
    fn test_read_usage_matches_trailing_slash_workdir() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::hours(1);
        let ts = now.to_rfc3339();
        let content = format!(
            "{}\n{}\n",
            session_meta_line(&ts, "/repo/"),
            token_count_line(100, 0, 10),
        );
        write_rollout(tmp.path(), now, "rollout-a.jsonl", &content);

        let usage = read_usage(tmp.path(), "/repo/", since, now).unwrap();
        assert_eq!(usage.input_tokens, 100);
    }

    #[test]
    fn test_read_usage_null_info_keeps_quota_only() {
        // Pure rate-limit updates carry info: null — totals stay at zero.
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::hours(1);
        let ts = now.to_rfc3339();
        let null_info = r#"{"timestamp":"2026-06-12T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":1.0,"window_minutes":300,"resets_at":1775073678},"secondary":{"used_percent":0.0,"window_minutes":10080,"resets_at":1775660478},"plan_type":"plus"}}}"#;
        let content = format!("{}\n{null_info}\n", session_meta_line(&ts, "/repo"));
        write_rollout(tmp.path(), now, "rollout-a.jsonl", &content);

        let usage = read_usage(tmp.path(), "/repo", since, now).unwrap();
        assert_eq!(usage.input_tokens, 0);
        assert!(usage.quota.is_some());
    }

    #[test]
    fn test_read_usage_ignores_non_rollout_files() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::hours(1);
        let ts = now.to_rfc3339();
        let content = format!(
            "{}\n{}\n",
            session_meta_line(&ts, "/repo"),
            token_count_line(100, 0, 10),
        );
        write_rollout(tmp.path(), now, "other.jsonl", &content);
        write_rollout(tmp.path(), now, "rollout-a.txt", &content);

        assert!(read_usage(tmp.path(), "/repo", since, now).is_none());
    }

    #[test]
    fn test_read_rollout_file_skips_invalid_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::hours(1);
        let ts = now.to_rfc3339();
        let content = format!(
            "{}\nnot json\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\"}}}}\n{{\"type\":\"event_msg\"}}\n{}\n",
            session_meta_line(&ts, "/repo"),
            token_count_line(100, 0, 10),
        );
        write_rollout(tmp.path(), now, "rollout-a.jsonl", &content);

        let usage = read_usage(tmp.path(), "/repo", since, now).unwrap();
        assert_eq!(usage.input_tokens, 100);
    }

    #[test]
    fn test_read_rollout_file_without_session_meta() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let since = now - TimeDelta::hours(1);
        write_rollout(
            tmp.path(),
            now,
            "rollout-a.jsonl",
            &format!("{}\n", token_count_line(100, 0, 10)),
        );

        assert!(read_usage(tmp.path(), "/repo", since, now).is_none());
    }

    #[test]
    fn test_parse_quota_window_requires_used_percent() {
        let value = serde_json::json!({"window_minutes": 300});
        assert!(parse_quota_window(&value).is_none());
    }

    #[test]
    fn test_parse_session_meta_rejects_bad_timestamp() {
        let value = serde_json::json!({
            "type": "session_meta",
            "payload": {"cwd": "/repo", "timestamp": "not-a-date"}
        });
        assert!(parse_session_meta(&value).is_none());
    }
}
