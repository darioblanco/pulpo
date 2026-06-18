//! Claude Code transcript reader.
//!
//! Claude Code writes one JSONL transcript per session under
//! `<claude_dir>/projects/<sanitized-workdir>/<session-uuid>.jsonl`. Each assistant
//! record carries a `message.usage` block with exact token counts and the model ID.

use std::collections::HashSet;
use std::path::Path;

use chrono::{DateTime, Utc};

use super::{ExactUsage, RateOverrides, SOURCE_CLAUDE, resolve_rates};

/// Sanitize a working directory into Claude Code's project-directory name.
/// Claude Code replaces every non-alphanumeric character with `-`
/// (`/Users/dario/.pulpo` becomes `-Users-dario--pulpo`).
pub fn sanitize_workdir(workdir: &str) -> String {
    workdir
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Running totals while parsing transcript records.
#[derive(Debug, Default)]
struct Totals {
    input: u64,
    output: u64,
    cache_write: u64,
    cache_read: u64,
    cost_usd: f64,
    unknown_model: bool,
    records: u64,
}

/// Read a `u64` token count from a usage field, defaulting to 0.
fn token_field(usage: &serde_json::Value, key: &str) -> u64 {
    usage
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}

/// Parse one transcript line and fold its usage into `totals`.
///
/// Lines are skipped when they are not JSON, predate `since`, carry no
/// `message.usage`, or repeat an already-seen `message.id` + `requestId` pair
/// (streaming writes the same usage on multiple records).
fn apply_transcript_line(
    line: &str,
    since: DateTime<Utc>,
    seen: &mut HashSet<String>,
    totals: &mut Totals,
    rates: &RateOverrides,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let Some(timestamp) = value
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
    else {
        return;
    };
    if timestamp.with_timezone(&Utc) < since {
        return;
    }
    let Some(message) = value.get("message") else {
        return;
    };
    let Some(usage) = message.get("usage") else {
        return;
    };

    let message_id = message.get("id").and_then(serde_json::Value::as_str);
    let request_id = value.get("requestId").and_then(serde_json::Value::as_str);
    if let (Some(mid), Some(rid)) = (message_id, request_id)
        && !seen.insert(format!("{mid}:{rid}"))
    {
        return;
    }

    let input = token_field(usage, "input_tokens");
    let output = token_field(usage, "output_tokens");
    let cache_read = token_field(usage, "cache_read_input_tokens");
    // Prefer the TTL breakdown (priced differently) over the flat total.
    let (five_min, one_hour) = usage.get("cache_creation").map_or_else(
        || (token_field(usage, "cache_creation_input_tokens"), 0),
        |breakdown| {
            (
                token_field(breakdown, "ephemeral_5m_input_tokens"),
                token_field(breakdown, "ephemeral_1h_input_tokens"),
            )
        },
    );

    totals.input += input;
    totals.output += output;
    totals.cache_read += cache_read;
    totals.cache_write += five_min + one_hour;
    totals.records += 1;

    let model = message
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    #[allow(clippy::cast_precision_loss)]
    match resolve_rates(model, rates) {
        Some(rates) => {
            totals.cost_usd += (input as f64).mul_add(
                rates.input,
                (output as f64).mul_add(
                    rates.output,
                    (cache_read as f64).mul_add(
                        rates.cache_read,
                        (five_min as f64).mul_add(
                            rates.cache_write_5m,
                            (one_hour as f64) * rates.cache_write_1h,
                        ),
                    ),
                ),
            ) / 1_000_000.0;
        }
        None => {
            if input + output + cache_read + five_min + one_hour > 0 {
                totals.unknown_model = true;
            }
        }
    }
}

/// Read exact usage for a Claude Code session running in `workdir`, started at `since`.
///
/// Sums usage records with timestamps at or after `since` across every transcript in
/// the project directory whose mtime is at or after `since`. Returns `None` when the
/// directory does not exist or no matching records are found.
pub fn read_usage(
    claude_dir: &Path,
    workdir: &str,
    since: DateTime<Utc>,
    rates: &RateOverrides,
) -> Option<ExactUsage> {
    let project_dir = claude_dir.join("projects").join(sanitize_workdir(workdir));
    let entries = std::fs::read_dir(&project_dir).ok()?;

    let mut totals = Totals::default();
    let mut seen = HashSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        if let Ok(file_meta) = entry.metadata()
            && let Ok(modified) = file_meta.modified()
        {
            let modified: DateTime<Utc> = modified.into();
            if modified < since {
                continue;
            }
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in content.lines() {
            apply_transcript_line(line, since, &mut seen, &mut totals, rates);
        }
    }

    if totals.records == 0 {
        return None;
    }
    Some(ExactUsage {
        source: SOURCE_CLAUDE,
        input_tokens: totals.input,
        output_tokens: totals.output,
        cache_write_tokens: totals.cache_write,
        cache_read_tokens: totals.cache_read,
        cost_usd: (!totals.unknown_model).then_some(totals.cost_usd),
        quota: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use std::fs;

    fn transcript_line(timestamp: &str, message_id: &str, request_id: &str, model: &str) -> String {
        format!(
            r#"{{"timestamp":"{timestamp}","requestId":"{request_id}","type":"assistant","message":{{"id":"{message_id}","model":"{model}","usage":{{"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":2000,"cache_creation_input_tokens":300,"cache_creation":{{"ephemeral_5m_input_tokens":300,"ephemeral_1h_input_tokens":0}}}}}}}}"#
        )
    }

    fn write_project_file(claude_dir: &Path, workdir: &str, name: &str, content: &str) {
        let project_dir = claude_dir.join("projects").join(sanitize_workdir(workdir));
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(project_dir.join(name), content).unwrap();
    }

    #[test]
    fn test_sanitize_workdir_plain_path() {
        assert_eq!(
            sanitize_workdir("/Users/dario/Code/darioblanco/pulpo"),
            "-Users-dario-Code-darioblanco-pulpo"
        );
    }

    #[test]
    fn test_sanitize_workdir_dots_become_dashes() {
        assert_eq!(
            sanitize_workdir("/Users/dario/.pulpo/worktrees/fix-1"),
            "-Users-dario--pulpo-worktrees-fix-1"
        );
    }

    #[test]
    fn test_sanitize_workdir_underscores_and_spaces() {
        assert_eq!(sanitize_workdir("/tmp/my_repo v2"), "-tmp-my-repo-v2");
    }

    #[test]
    fn test_read_usage_missing_project_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            read_usage(
                tmp.path(),
                "/tmp/repo",
                Utc::now(),
                &RateOverrides::default()
            )
            .is_none()
        );
    }

    #[test]
    fn test_read_usage_sums_records_and_computes_cost() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        let content = format!(
            "{}\n{}\n",
            transcript_line(&ts, "msg_1", "req_1", "claude-fable-5"),
            transcript_line(&ts, "msg_2", "req_2", "claude-fable-5"),
        );
        write_project_file(tmp.path(), "/tmp/repo", "abc.jsonl", &content);

        let usage = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        assert_eq!(usage.source, SOURCE_CLAUDE);
        assert_eq!(usage.input_tokens, 2000);
        assert_eq!(usage.output_tokens, 1000);
        assert_eq!(usage.cache_read_tokens, 4000);
        assert_eq!(usage.cache_write_tokens, 600);
        // 2 records × (1000×$10 + 500×$50 + 2000×$1 + 300×$12.5) / 1M
        let expected = 2.0 * (10_000.0 + 25_000.0 + 2_000.0 + 3_750.0) / 1_000_000.0;
        assert!((usage.cost_usd.unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn test_read_usage_sums_across_multiple_files_in_project_dir() {
        // The reader sums every transcript in the project dir. This is correct for one
        // session whose history spans multiple files — and is also the documented
        // over-count: a second agent run in the same workdir during the window is
        // attributed here too (session→file mapping is dir+time, not per-session-id).
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        write_project_file(
            tmp.path(),
            "/tmp/repo",
            "first.jsonl",
            &transcript_line(&ts, "m1", "r1", "claude-opus-4-8"),
        );
        write_project_file(
            tmp.path(),
            "/tmp/repo",
            "second.jsonl",
            &transcript_line(&ts, "m2", "r2", "claude-opus-4-8"),
        );

        let usage = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        // Both files counted: 2 × 1000 input, 2 × 500 output.
        assert_eq!(usage.input_tokens, 2000);
        assert_eq!(usage.output_tokens, 1000);
    }

    #[test]
    fn test_read_usage_dedupes_repeated_message_and_request_id() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        let line = transcript_line(&ts, "msg_1", "req_1", "claude-opus-4-8");
        write_project_file(
            tmp.path(),
            "/tmp/repo",
            "abc.jsonl",
            &format!("{line}\n{line}\n"),
        );

        let usage = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
    }

    #[test]
    fn test_read_usage_skips_records_before_since() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let old_ts = (Utc::now() - TimeDelta::hours(5)).to_rfc3339();
        let new_ts = Utc::now().to_rfc3339();
        let content = format!(
            "{}\n{}\n",
            transcript_line(&old_ts, "msg_old", "req_old", "claude-fable-5"),
            transcript_line(&new_ts, "msg_new", "req_new", "claude-fable-5"),
        );
        write_project_file(tmp.path(), "/tmp/repo", "abc.jsonl", &content);

        let usage = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        assert_eq!(usage.input_tokens, 1000);
    }

    #[test]
    fn test_read_usage_all_records_too_old_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let old_ts = (Utc::now() - TimeDelta::hours(5)).to_rfc3339();
        let content = transcript_line(&old_ts, "msg_old", "req_old", "claude-fable-5");
        write_project_file(tmp.path(), "/tmp/repo", "abc.jsonl", &content);

        let since = Utc::now() - TimeDelta::hours(1);
        assert!(read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).is_none());
    }

    #[test]
    fn test_read_usage_unknown_model_withholds_cost() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        let content = format!(
            "{}\n{}\n",
            transcript_line(&ts, "msg_1", "req_1", "claude-fable-5"),
            transcript_line(&ts, "msg_2", "req_2", "experimental-model"),
        );
        write_project_file(tmp.path(), "/tmp/repo", "abc.jsonl", &content);

        let usage = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        assert_eq!(usage.input_tokens, 2000);
        assert!(usage.cost_usd.is_none());
    }

    #[test]
    fn test_read_usage_config_override_prices_unknown_model() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        let content = transcript_line(&ts, "msg_1", "req_1", "brand-new-model");
        write_project_file(tmp.path(), "/tmp/repo", "abc.jsonl", &content);

        // No override → unknown model, cost withheld.
        let bare = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        assert!(bare.cost_usd.is_none());

        // With a [rates.brand-new-model] override, cost is computed (no code change).
        let overrides = RateOverrides::new([(
            "brand-new-model".to_owned(),
            crate::usage::ModelRates {
                input: 2.0,
                output: 8.0,
                cache_read: 0.0,
                cache_write_5m: 0.0,
                cache_write_1h: 0.0,
            },
        )]);
        let usage = read_usage(tmp.path(), "/tmp/repo", since, &overrides).unwrap();
        // input 1000×$2 + output 500×$8 (cache priced at $0) = 6000 / 1M
        let expected = (1000.0f64).mul_add(2.0, 500.0 * 8.0) / 1_000_000.0;
        assert!((usage.cost_usd.unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn test_read_usage_config_override_reprices_known_model() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        let content = transcript_line(&ts, "msg_1", "req_1", "claude-opus-4-8");
        write_project_file(tmp.path(), "/tmp/repo", "abc.jsonl", &content);

        // Exact-ID override beats the built-in opus rate.
        let overrides = RateOverrides::new([(
            "claude-opus-4-8".to_owned(),
            crate::usage::ModelRates {
                input: 99.0,
                output: 0.0,
                cache_read: 0.0,
                cache_write_5m: 0.0,
                cache_write_1h: 0.0,
            },
        )]);
        let usage = read_usage(tmp.path(), "/tmp/repo", since, &overrides).unwrap();
        let expected = 1000.0 * 99.0 / 1_000_000.0;
        assert!((usage.cost_usd.unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn test_read_usage_prices_1h_cache_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        let line = format!(
            r#"{{"timestamp":"{ts}","requestId":"req_1","message":{{"id":"msg_1","model":"claude-fable-5","usage":{{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":1000000,"cache_creation":{{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":1000000}}}}}}}}"#
        );
        write_project_file(tmp.path(), "/tmp/repo", "abc.jsonl", &line);

        let usage = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        assert_eq!(usage.cache_write_tokens, 1_000_000);
        assert!((usage.cost_usd.unwrap() - 20.0).abs() < 1e-9);
    }

    #[test]
    fn test_read_usage_flat_cache_creation_without_breakdown() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        let line = format!(
            r#"{{"timestamp":"{ts}","requestId":"req_1","message":{{"id":"msg_1","model":"claude-haiku-4-5","usage":{{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":400}}}}}}"#
        );
        write_project_file(tmp.path(), "/tmp/repo", "abc.jsonl", &line);

        let usage = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        assert_eq!(usage.cache_write_tokens, 400);
        assert_eq!(usage.cache_read_tokens, 0);
    }

    #[test]
    fn test_read_usage_skips_invalid_and_irrelevant_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        let content = format!(
            "not json\n{{\"timestamp\":\"{ts}\",\"type\":\"user\"}}\n{{\"timestamp\":\"bad-ts\"}}\n{{\"no_timestamp\":true}}\n{{\"timestamp\":\"{ts}\",\"message\":{{\"id\":\"m\"}}}}\n{}\n",
            transcript_line(&ts, "msg_1", "req_1", "claude-sonnet-4-6"),
        );
        write_project_file(tmp.path(), "/tmp/repo", "abc.jsonl", &content);

        let usage = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        assert_eq!(usage.input_tokens, 1000);
    }

    #[test]
    fn test_read_usage_counts_records_without_ids() {
        // Records missing message.id/requestId can't be deduped — both count.
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        let line = format!(
            r#"{{"timestamp":"{ts}","message":{{"model":"claude-opus-4-8","usage":{{"input_tokens":10,"output_tokens":5}}}}}}"#
        );
        write_project_file(
            tmp.path(),
            "/tmp/repo",
            "abc.jsonl",
            &format!("{line}\n{line}\n"),
        );

        let usage = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        assert_eq!(usage.input_tokens, 20);
    }

    #[test]
    fn test_read_usage_ignores_non_jsonl_files() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        write_project_file(
            tmp.path(),
            "/tmp/repo",
            "notes.txt",
            &transcript_line(&ts, "msg_1", "req_1", "claude-fable-5"),
        );

        assert!(read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).is_none());
    }

    #[test]
    fn test_read_usage_skips_files_untouched_since_spawn() {
        let tmp = tempfile::tempdir().unwrap();
        let ts = Utc::now().to_rfc3339();
        write_project_file(
            tmp.path(),
            "/tmp/repo",
            "old.jsonl",
            &transcript_line(&ts, "msg_1", "req_1", "claude-fable-5"),
        );
        let file_path = tmp
            .path()
            .join("projects")
            .join(sanitize_workdir("/tmp/repo"))
            .join("old.jsonl");
        let old_mtime = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let file = fs::File::options().write(true).open(&file_path).unwrap();
        file.set_modified(old_mtime).unwrap();

        let since = Utc::now() - TimeDelta::hours(1);
        assert!(read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).is_none());
    }

    #[test]
    fn test_read_usage_sums_across_multiple_files() {
        let tmp = tempfile::tempdir().unwrap();
        let since = Utc::now() - TimeDelta::hours(1);
        let ts = Utc::now().to_rfc3339();
        write_project_file(
            tmp.path(),
            "/tmp/repo",
            "a.jsonl",
            &transcript_line(&ts, "msg_1", "req_1", "claude-fable-5"),
        );
        write_project_file(
            tmp.path(),
            "/tmp/repo",
            "b.jsonl",
            &transcript_line(&ts, "msg_2", "req_2", "claude-fable-5"),
        );

        let usage = read_usage(tmp.path(), "/tmp/repo", since, &RateOverrides::default()).unwrap();
        assert_eq!(usage.input_tokens, 2000);
    }
}
