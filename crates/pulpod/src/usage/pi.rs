//! pi session-file reader (scan only).
//!
//! pi (`@mariozechner/pi-coding-agent`) writes one JSONL session file per session under
//! `<pi_dir>/agent/sessions/--<encoded-cwd>--/<uuid>.jsonl`. The first line is a
//! `session` header carrying the working directory; subsequent `message` entries wrap
//! the conversation, and assistant messages record per-message token usage *and* the
//! exact dollar cost pi computed from its model catalog — no rate table needed on our
//! side. pi is BYOK (API-key) only, so recorded cost is real API spend.

use std::collections::HashSet;
use std::path::Path;

use chrono::{DateTime, Utc};

use super::{ScanEntry, collect_jsonl_files, token_field};

/// Per-model pi token totals and costs across all session files, from assistant
/// messages at or after `since`. Messages are filtered individually (a long-lived
/// session contributes only the messages inside the window), then folded per file
/// and model, so one entry covers one model's spend in one session.
///
/// Branching and forking copy prior assistant messages — original entry id,
/// timestamp, usage, and cost included — into a *new* session file, so the shared
/// history is deduplicated across files by `(entry id, timestamp)`: each message's
/// spend counts exactly once no matter how many branches carry it.
pub(crate) fn scan_sessions(pi_dir: &Path, since: DateTime<Utc>) -> Vec<ScanEntry> {
    let mut out = Vec::new();
    let mut seen: HashSet<(String, i64)> = HashSet::new();
    for path in collect_jsonl_files(pi_dir.join("agent").join("sessions"), |_| true) {
        read_session_file(&path, since, &mut seen, &mut out);
    }
    out
}

/// Parse one session file, appending an entry per model with in-window usage.
/// `seen` carries the `(entry id, timestamp)` pairs already counted in other files.
fn read_session_file(
    path: &Path,
    since: DateTime<Utc>,
    seen: &mut HashSet<(String, i64)>,
    out: &mut Vec<ScanEntry>,
) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let mut lines = content.lines();

    // pi buffers entries until the first assistant message arrives, then flushes
    // header-first — so the `session` header is always the first line.
    let Some(cwd) = lines
        .next()
        .and_then(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .and_then(|value| parse_header_cwd(&value))
    else {
        return;
    };

    // Fold in-window assistant messages per model (mid-session switching means a file
    // can hold several, but rarely more than two — a linear scan beats a map here).
    let mut models: Vec<(String, u64, Option<f64>)> = Vec::new();
    for line in lines {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some((model, tokens, cost, ts_millis)) = parse_assistant_message(&value, since) else {
            continue;
        };
        // Skip messages another file already counted (branch/fork copies keep the
        // original entry id and timestamp). Entries without an id are counted as-is.
        if let Some(id) = value.get("id").and_then(serde_json::Value::as_str)
            && !seen.insert((id.to_owned(), ts_millis))
        {
            continue;
        }
        match models.iter_mut().find(|(m, ..)| m == model) {
            Some(agg) => {
                agg.1 += tokens;
                if let Some(c) = cost {
                    agg.2 = Some(agg.2.unwrap_or(0.0) + c);
                }
            }
            None => models.push((model.to_owned(), tokens, cost)),
        }
    }
    for (model, tokens, cost_usd) in models {
        out.push(ScanEntry {
            cwd: cwd.clone(),
            model: Some(model),
            tokens,
            cost_usd,
        });
    }
}

/// Extract the working directory from a `session` header record.
fn parse_header_cwd(value: &serde_json::Value) -> Option<String> {
    if value.get("type").and_then(serde_json::Value::as_str) != Some("session") {
        return None;
    }
    value
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// Parse a `message` entry wrapping an assistant message into
/// `(model, tokens, cost, timestamp-millis)`. Returns `None` for non-message entries,
/// non-assistant roles, and messages before `since` (assistant timestamps are epoch
/// milliseconds). Cost is `None` when the record predates pi's cost logging.
fn parse_assistant_message(
    value: &serde_json::Value,
    since: DateTime<Utc>,
) -> Option<(&str, u64, Option<f64>, i64)> {
    if value.get("type").and_then(serde_json::Value::as_str) != Some("message") {
        return None;
    }
    let message = value.get("message")?;
    if message.get("role").and_then(serde_json::Value::as_str) != Some("assistant") {
        return None;
    }
    let ts_millis = message
        .get("timestamp")
        .and_then(serde_json::Value::as_i64)?;
    let ts = DateTime::<Utc>::from_timestamp_millis(ts_millis)?;
    if ts < since {
        return None;
    }
    let model = message.get("model").and_then(serde_json::Value::as_str)?;
    let usage = message.get("usage")?;
    let tokens = token_field(usage, "input")
        + token_field(usage, "output")
        + token_field(usage, "cacheRead")
        + token_field(usage, "cacheWrite");
    let cost = usage
        .get("cost")
        .and_then(|c| c.get("total"))
        .and_then(serde_json::Value::as_f64);
    Some((model, tokens, cost, ts_millis))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use std::fs;

    fn header_line(cwd: &str) -> String {
        format!(
            r#"{{"type":"session","version":3,"id":"0197-abc","timestamp":"2026-06-12T10:00:00.000Z","cwd":"{cwd}"}}"#
        )
    }

    fn user_line() -> String {
        r#"{"type":"message","id":"aaaa1111","parentId":null,"message":{"role":"user","content":"hi","timestamp":1765533600000}}"#.to_owned()
    }

    fn assistant_line(
        id: &str,
        model: &str,
        ts: DateTime<Utc>,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
        cost_total: f64,
    ) -> String {
        format!(
            r#"{{"type":"message","id":"{id}","parentId":"aaaa1111","message":{{"role":"assistant","content":[{{"type":"text","text":"ok"}}],"api":"anthropic-messages","provider":"anthropic","model":"{model}","usage":{{"input":{input},"output":{output},"cacheRead":{cache_read},"cacheWrite":{cache_write},"totalTokens":{},"cost":{{"input":0.0,"output":0.0,"cacheRead":0.0,"cacheWrite":0.0,"total":{cost_total}}}}},"stopReason":"stop","timestamp":{}}}}}"#,
            input + output + cache_read + cache_write,
            ts.timestamp_millis(),
        )
    }

    fn write_session(pi_dir: &Path, dir_name: &str, file_name: &str, content: &str) {
        let d = pi_dir.join("agent").join("sessions").join(dir_name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(file_name), content).unwrap();
    }

    #[test]
    fn test_scan_sessions_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(scan_sessions(tmp.path(), Utc::now()).is_empty());
    }

    #[test]
    fn test_scan_sessions_reads_assistant_usage_and_cost() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let content = format!(
            "{}\n{}\n{}\n",
            header_line("/repos/api"),
            user_line(),
            assistant_line("b1", "claude-sonnet-4-5", now, 1000, 200, 5000, 300, 0.0325),
        );
        write_session(tmp.path(), "--repos-api--", "0197-abc.jsonl", &content);

        let entries = scan_sessions(tmp.path(), now - TimeDelta::hours(1));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cwd, "/repos/api");
        assert_eq!(entries[0].model.as_deref(), Some("claude-sonnet-4-5"));
        // input + output + cacheRead + cacheWrite.
        assert_eq!(entries[0].tokens, 6500);
        assert!((entries[0].cost_usd.unwrap() - 0.0325).abs() < 1e-12);
    }

    #[test]
    fn test_scan_sessions_filters_messages_by_since_and_folds_per_model() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let old = now - TimeDelta::days(10);
        // One old and one recent assistant message in the same session file:
        // only the recent one is inside a 3-day window.
        let content = format!(
            "{}\n{}\n{}\n",
            header_line("/repos/api"),
            assistant_line("b1", "claude-sonnet-4-5", old, 100, 10, 0, 0, 0.001),
            assistant_line("b2", "claude-sonnet-4-5", now, 200, 20, 0, 0, 0.002),
        );
        write_session(tmp.path(), "--repos-api--", "s.jsonl", &content);

        let entries = scan_sessions(tmp.path(), now - TimeDelta::days(3));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tokens, 220);
        assert!((entries[0].cost_usd.unwrap() - 0.002).abs() < 1e-12);

        // All-time (epoch): both messages fold into one per-model entry.
        let all = scan_sessions(tmp.path(), DateTime::<Utc>::from_timestamp(0, 0).unwrap());
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].tokens, 330);
        assert!((all[0].cost_usd.unwrap() - 0.003).abs() < 1e-12);
    }

    #[test]
    fn test_scan_sessions_multiple_models_per_file() {
        // Mid-session model switching: each model gets its own entry.
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let content = format!(
            "{}\n{}\n{}\n",
            header_line("/repos/api"),
            assistant_line("b1", "claude-sonnet-4-5", now, 100, 10, 0, 0, 0.001),
            assistant_line("b2", "gemini-3-pro", now, 200, 20, 0, 0, 0.002),
        );
        write_session(tmp.path(), "--repos-api--", "s.jsonl", &content);

        let entries = scan_sessions(tmp.path(), now - TimeDelta::hours(1));
        assert_eq!(entries.len(), 2);
        let models: Vec<Option<&str>> = entries.iter().map(|e| e.model.as_deref()).collect();
        assert!(models.contains(&Some("claude-sonnet-4-5")));
        assert!(models.contains(&Some("gemini-3-pro")));
    }

    #[test]
    fn test_scan_sessions_dedupes_branched_history_across_files() {
        // Branch/fork: pi copies the path-to-branch-point into a NEW session file,
        // original entry ids and timestamps included. The shared prefix must count once.
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let shared = assistant_line("b1", "claude-sonnet-4-5", now, 1000, 100, 0, 0, 0.60);
        let original = format!(
            "{}\n{shared}\n{}\n",
            header_line("/repos/api"),
            assistant_line("b2", "claude-sonnet-4-5", now, 500, 50, 0, 0, 0.40),
        );
        // The branched file carries the same shared prefix plus its own new message.
        let branched = format!(
            "{}\n{shared}\n{}\n",
            header_line("/repos/api"),
            assistant_line("b3", "claude-sonnet-4-5", now, 200, 20, 0, 0, 0.10),
        );
        write_session(tmp.path(), "--repos-api--", "orig.jsonl", &original);
        write_session(tmp.path(), "--repos-api--", "branch.jsonl", &branched);

        let entries = scan_sessions(tmp.path(), now - TimeDelta::hours(1));
        let tokens: u64 = entries.iter().map(|e| e.tokens).sum();
        let cost: f64 = entries.iter().filter_map(|e| e.cost_usd).sum();
        // 1100 (shared, once) + 550 + 220 — not 2970.
        assert_eq!(tokens, 1870);
        // 0.60 (shared, once) + 0.40 + 0.10 — not 1.70.
        assert!((cost - 1.10).abs() < 1e-9);
    }

    #[test]
    fn test_scan_sessions_missing_cost_counts_tokens() {
        // Records predating cost logging: tokens still count, cost stays None.
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let no_cost = format!(
            r#"{{"type":"message","id":"cccc3333","parentId":null,"message":{{"role":"assistant","content":[],"model":"local-qwen","usage":{{"input":50,"output":5,"cacheRead":0,"cacheWrite":0,"totalTokens":55}},"stopReason":"stop","timestamp":{}}}}}"#,
            now.timestamp_millis()
        );
        let content = format!("{}\n{no_cost}\n", header_line("/repos/api"));
        write_session(tmp.path(), "--repos-api--", "s.jsonl", &content);

        let entries = scan_sessions(tmp.path(), now - TimeDelta::hours(1));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tokens, 55);
        assert!(entries[0].cost_usd.is_none());
    }

    #[test]
    fn test_scan_sessions_skips_files_without_leading_header() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        // Assistant message first, header missing: pi always flushes the header as
        // line one, so anything else means the file isn't a pi session.
        let content = format!(
            "{}\n",
            assistant_line("b1", "claude-sonnet-4-5", now, 100, 10, 0, 0, 0.001)
        );
        write_session(tmp.path(), "--repos-api--", "s.jsonl", &content);

        assert!(scan_sessions(tmp.path(), now - TimeDelta::hours(1)).is_empty());
    }

    #[test]
    fn test_scan_sessions_skips_invalid_lines_and_non_jsonl_files() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let content = format!(
            "{}\nnot json\n{}\n{}\n",
            header_line("/repos/api"),
            r#"{"type":"message","id":"x","message":{"role":"toolResult","timestamp":1}}"#,
            assistant_line("b1", "claude-sonnet-4-5", now, 100, 10, 0, 0, 0.001),
        );
        write_session(tmp.path(), "--repos-api--", "s.jsonl", &content);
        // A stray non-jsonl file in the same dir is ignored.
        write_session(tmp.path(), "--repos-api--", "notes.txt", &content);

        let entries = scan_sessions(tmp.path(), now - TimeDelta::hours(1));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tokens, 110);
    }

    #[test]
    fn test_parse_assistant_message_rejects_bad_records() {
        let since = Utc::now() - TimeDelta::hours(1);
        let ms = Utc::now().timestamp_millis();
        // Not a message entry (pi also writes model_change / compaction / etc.).
        let v: serde_json::Value = serde_json::json!({"type": "model_change"});
        assert!(parse_assistant_message(&v, since).is_none());
        // Message without a model.
        let v = serde_json::json!({
            "type": "message",
            "message": {"role": "assistant", "usage": {"input": 1}, "timestamp": ms}
        });
        assert!(parse_assistant_message(&v, since).is_none());
        // Message without usage.
        let v = serde_json::json!({
            "type": "message",
            "message": {"role": "assistant", "model": "m", "timestamp": ms}
        });
        assert!(parse_assistant_message(&v, since).is_none());
        // Message without a timestamp.
        let v = serde_json::json!({
            "type": "message",
            "message": {"role": "assistant", "model": "m", "usage": {"input": 1}}
        });
        assert!(parse_assistant_message(&v, since).is_none());
    }

    #[test]
    fn test_parse_header_cwd_requires_session_type_and_cwd() {
        assert!(parse_header_cwd(&serde_json::json!({"type": "message"})).is_none());
        assert!(parse_header_cwd(&serde_json::json!({"type": "session"})).is_none());
        assert_eq!(
            parse_header_cwd(&serde_json::json!({"type": "session", "cwd": "/x"})),
            Some("/x".to_owned())
        );
    }
}
