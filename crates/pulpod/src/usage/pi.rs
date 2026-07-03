//! pi session-file reader (scan only).
//!
//! pi (`@mariozechner/pi-coding-agent`) writes one JSONL session file per session under
//! `<pi_dir>/agent/sessions/--<encoded-cwd>--/<uuid>.jsonl`. The first line is a
//! `session` header carrying the working directory; subsequent `message` entries wrap
//! the conversation, and assistant messages record per-message token usage *and* the
//! exact dollar cost pi computed from its model catalog — no rate table needed on our
//! side. pi is BYOK (API-key) only, so recorded cost is real API spend.

use std::path::Path;

use chrono::{DateTime, Utc};

/// How many leading lines to scan for the `session` header record.
const HEADER_SCAN_LINES: usize = 10;

/// One assistant message's contribution to the usage scan.
pub(crate) struct ScanEntry {
    pub cwd: String,
    pub model: String,
    pub tokens: u64,
    /// Exact cost as recorded by pi. `None` when the record predates cost logging.
    pub cost_usd: Option<f64>,
}

/// Per-message pi token totals and costs across all session files with an assistant
/// message at or after `since`. Messages are filtered individually (a long-lived
/// session contributes only the messages inside the window).
pub(crate) fn scan_sessions(pi_dir: &Path, since: DateTime<Utc>) -> Vec<ScanEntry> {
    let mut out = Vec::new();
    for path in collect_session_files(pi_dir) {
        read_session_file(&path, since, &mut out);
    }
    out
}

/// Parse one session file, appending an entry per in-window assistant message.
fn read_session_file(path: &Path, since: DateTime<Utc>, out: &mut Vec<ScanEntry>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let mut lines = content.lines();

    // The header is written first (pi buffers until the first assistant message, then
    // flushes header-first), but tolerate leading garbage like the Codex reader does.
    let Some(cwd) = lines
        .by_ref()
        .take(HEADER_SCAN_LINES)
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| parse_header_cwd(&value))
    else {
        return;
    };

    for line in lines {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(entry) = parse_assistant_entry(&value, &cwd, since) {
            out.push(entry);
        }
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

/// Parse a `message` entry wrapping an assistant message into a scan entry.
/// Returns `None` for non-message entries, non-assistant roles, and messages
/// before `since` (assistant timestamps are epoch milliseconds).
fn parse_assistant_entry(
    value: &serde_json::Value,
    cwd: &str,
    since: DateTime<Utc>,
) -> Option<ScanEntry> {
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
    let tok = |key: &str| {
        usage
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    Some(ScanEntry {
        cwd: cwd.to_owned(),
        model: model.to_owned(),
        tokens: tok("input") + tok("output") + tok("cacheRead") + tok("cacheWrite"),
        cost_usd: usage
            .get("cost")
            .and_then(|c| c.get("total"))
            .and_then(serde_json::Value::as_f64),
    })
}

/// Recursively collect `*.jsonl` session files under `<pi_dir>/agent/sessions`.
fn collect_session_files(pi_dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![pi_dir.join("agent").join("sessions")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                out.push(path);
            }
        }
    }
    out
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
        model: &str,
        ts: DateTime<Utc>,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
        cost_total: f64,
    ) -> String {
        format!(
            r#"{{"type":"message","id":"bbbb2222","parentId":"aaaa1111","message":{{"role":"assistant","content":[{{"type":"text","text":"ok"}}],"api":"anthropic-messages","provider":"anthropic","model":"{model}","usage":{{"input":{input},"output":{output},"cacheRead":{cache_read},"cacheWrite":{cache_write},"totalTokens":{},"cost":{{"input":0.0,"output":0.0,"cacheRead":0.0,"cacheWrite":0.0,"total":{cost_total}}}}},"stopReason":"stop","timestamp":{}}}}}"#,
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
            assistant_line("claude-sonnet-4-5", now, 1000, 200, 5000, 300, 0.0325),
        );
        write_session(tmp.path(), "--repos-api--", "0197-abc.jsonl", &content);

        let entries = scan_sessions(tmp.path(), now - TimeDelta::hours(1));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cwd, "/repos/api");
        assert_eq!(entries[0].model, "claude-sonnet-4-5");
        // input + output + cacheRead + cacheWrite.
        assert_eq!(entries[0].tokens, 6500);
        assert!((entries[0].cost_usd.unwrap() - 0.0325).abs() < 1e-12);
    }

    #[test]
    fn test_scan_sessions_filters_messages_by_since() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let old = now - TimeDelta::days(10);
        // One old and one recent assistant message in the same session file:
        // only the recent one is inside a 3-day window.
        let content = format!(
            "{}\n{}\n{}\n",
            header_line("/repos/api"),
            assistant_line("claude-sonnet-4-5", old, 100, 10, 0, 0, 0.001),
            assistant_line("claude-sonnet-4-5", now, 200, 20, 0, 0, 0.002),
        );
        write_session(tmp.path(), "--repos-api--", "s.jsonl", &content);

        let entries = scan_sessions(tmp.path(), now - TimeDelta::days(3));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tokens, 220);

        // All-time (epoch) picks up both.
        let all = scan_sessions(tmp.path(), DateTime::<Utc>::from_timestamp(0, 0).unwrap());
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_scan_sessions_multiple_models_per_file() {
        // Mid-session model switching: each assistant message keeps its own model.
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let content = format!(
            "{}\n{}\n{}\n",
            header_line("/repos/api"),
            assistant_line("claude-sonnet-4-5", now, 100, 10, 0, 0, 0.001),
            assistant_line("gemini-3-pro", now, 200, 20, 0, 0, 0.002),
        );
        write_session(tmp.path(), "--repos-api--", "s.jsonl", &content);

        let entries = scan_sessions(tmp.path(), now - TimeDelta::hours(1));
        assert_eq!(entries.len(), 2);
        let models: Vec<&str> = entries.iter().map(|e| e.model.as_str()).collect();
        assert!(models.contains(&"claude-sonnet-4-5"));
        assert!(models.contains(&"gemini-3-pro"));
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
    fn test_scan_sessions_skips_files_without_header() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let content = format!(
            "{}\n",
            assistant_line("claude-sonnet-4-5", now, 100, 10, 0, 0, 0.001)
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
            assistant_line("claude-sonnet-4-5", now, 100, 10, 0, 0, 0.001),
        );
        write_session(tmp.path(), "--repos-api--", "s.jsonl", &content);
        // A stray non-jsonl file in the same dir is ignored.
        write_session(tmp.path(), "--repos-api--", "notes.txt", &content);

        let entries = scan_sessions(tmp.path(), now - TimeDelta::hours(1));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tokens, 110);
    }

    #[test]
    fn test_parse_assistant_entry_rejects_bad_records() {
        let since = Utc::now() - TimeDelta::hours(1);
        let ms = Utc::now().timestamp_millis();
        // Not a message entry.
        let v: serde_json::Value = serde_json::json!({"type": "compaction"});
        assert!(parse_assistant_entry(&v, "/r", since).is_none());
        // Message without a model.
        let v = serde_json::json!({
            "type": "message",
            "message": {"role": "assistant", "usage": {"input": 1}, "timestamp": ms}
        });
        assert!(parse_assistant_entry(&v, "/r", since).is_none());
        // Message without usage.
        let v = serde_json::json!({
            "type": "message",
            "message": {"role": "assistant", "model": "m", "timestamp": ms}
        });
        assert!(parse_assistant_entry(&v, "/r", since).is_none());
        // Message without a timestamp.
        let v = serde_json::json!({
            "type": "message",
            "message": {"role": "assistant", "model": "m", "usage": {"input": 1}}
        });
        assert!(parse_assistant_entry(&v, "/r", since).is_none());
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
