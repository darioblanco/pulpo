//! Terminal output rendering for the `pulpo` CLI: table and report
//! formatting for sessions, usage, and schedules.
//!
//! Pure move from `lib.rs` — no logic changes.

use pulpo_common::api::{
    DimensionRollup, InterventionEventResponse, ScanRollup, UsageScanResponse,
    UsageSessionsResponse,
};
use pulpo_common::session::Session;

/// Format the repo column: `basename@branch +42/-7 ↑3` with diff stats and ahead count.
/// Truncates to 30 chars if needed.
/// Format the branch column: branch name + diff stats + ahead count.
fn format_branch(session: &Session) -> String {
    let branch = session.git_branch.as_deref().unwrap_or("-").to_owned();

    let mut suffix = String::new();
    let ins = session.git_insertions.unwrap_or(0);
    let del = session.git_deletions.unwrap_or(0);
    if ins > 0 || del > 0 {
        suffix = format!(" +{ins}/-{del}");
    }
    if let Some(ahead) = session.git_ahead
        && ahead > 0
    {
        suffix = format!("{suffix} \u{2191}{ahead}");
    }

    format!("{branch}{suffix}")
}

/// Status label, reason-qualified per ADR 0009's five-state model:
///   - `waiting` + `needs_input:<reason>` → `waiting (needs input: <reason>)`
///     (blocked on the human — a harness hook, or a matched scrollback pattern).
///   - `waiting` + `idle` → `waiting (idle)` (turn finished, nothing pending).
///   - `done` + `exited` → `done (exit N)` when `exit_code` is known, else
///     `done (exited)`.
///   - `done` + `stopped` → `done (stopped)` (explicit `pulpo stop`).
///   - `done` + an intervention code → `done (idle timeout)` / `done (budget
///     exceeded)` / `done (memory pressure)`.
///   - `starting`/`working`/`lost`, or any status with no reason: the bare status
///     word.
fn format_status(session: &Session) -> String {
    use pulpo_common::session::{SessionStatus, status_reason};

    let Some(reason) = session.status_reason.as_deref() else {
        return session.status.to_string();
    };

    match session.status {
        SessionStatus::Waiting => {
            if let Some(sub) = status_reason::needs_input_reason(reason) {
                format!("waiting (needs input: {sub})")
            } else {
                format!("waiting ({reason})")
            }
        }
        SessionStatus::Done => {
            let label = match reason {
                status_reason::EXITED => session
                    .exit_code
                    .map_or_else(|| "exited".to_owned(), |code| format!("exit {code}")),
                status_reason::STOPPED => "stopped".to_owned(),
                status_reason::IDLE_TIMEOUT => "idle timeout".to_owned(),
                status_reason::BUDGET_EXCEEDED => "budget exceeded".to_owned(),
                status_reason::MEMORY_PRESSURE => "memory pressure".to_owned(),
                // Forward-compat: an unrecognized `done` reason still needs a label
                // (mirrors the web UI's identical fallback).
                _ => "stopped".to_owned(),
            };
            format!("done ({label})")
        }
        SessionStatus::Starting | SessionStatus::Working | SessionStatus::Lost => {
            session.status.to_string()
        }
    }
}

/// Build a display name with badges: [wt] [PR] [!]
fn format_name(session: &Session) -> String {
    use pulpo_common::session::meta;

    let mut name = session.name.clone();
    if session.worktree_path.is_some() {
        name = format!("{name} [wt]");
    }
    if session.meta_str(meta::PR_URL).is_some() {
        name = format!("{name} [PR]");
    }
    if session.meta_str(meta::ERROR_STATUS).is_some() {
        name = format!("{name} [!]");
    }
    name
}

/// Format token count with K/M suffixes for human readability.
fn format_token_count(n: u64) -> String {
    if n >= 1_000_000 {
        #[allow(clippy::cast_precision_loss)]
        let val = n as f64 / 1_000_000.0;
        format!("{val:.1}M")
    } else if n >= 1_000 {
        #[allow(clippy::cast_precision_loss)]
        let val = n as f64 / 1_000.0;
        format!("{val:.1}K")
    } else {
        n.to_string()
    }
}

/// Format usage column: cost if available, else token count.
fn format_usage(session: &Session) -> String {
    use pulpo_common::session::meta;

    if let Some(cost) = session.meta_parsed::<f64>(meta::SESSION_COST_USD) {
        return format!("${cost:.2}");
    }

    if let Some(tokens) = session.meta_parsed::<u64>(meta::TOTAL_INPUT_TOKENS) {
        return format!("{} tok", format_token_count(tokens));
    }

    "-".into()
}

/// Truncate a string to `max` chars with ellipsis.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        let t: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{t}...")
    }
}

pub fn format_sessions(sessions: &[Session]) -> String {
    if sessions.is_empty() {
        return "No sessions.".into();
    }

    // Compute dynamic column widths from data
    let rows: Vec<(String, String, String, String, String, String)> = sessions
        .iter()
        .map(|s| {
            (
                s.id.to_string()[..8].to_owned(),
                format_name(s),
                format_status(s),
                format_usage(s),
                format_branch(s),
                s.command.clone(),
            )
        })
        .collect();

    let w_id = 8;
    let w_name = rows.iter().map(|r| r.1.len()).max().unwrap_or(4).max(4);
    let w_status = rows.iter().map(|r| r.2.len()).max().unwrap_or(8).max(8);
    let w_usage = rows.iter().map(|r| r.3.len()).max().unwrap_or(5).max(5);
    let w_branch = rows.iter().map(|r| r.4.len()).max().unwrap_or(6).max(6);

    let mut lines = vec![format!(
        "{:<w_id$}  {:<w_name$}  {:<w_status$}  {:<w_usage$}  {:<w_branch$}  {}",
        "ID", "NAME", "STATUS", "USAGE", "BRANCH", "COMMAND"
    )];
    for (id, name, status, usage, branch, cmd) in &rows {
        lines.push(format!(
            "{:<w_id$}  {:<w_name$}  {:<w_status$}  {:<w_usage$}  {:<w_branch$}  {}",
            id,
            truncate(name, w_name),
            status,
            usage,
            truncate(branch, w_branch),
            truncate(cmd, 50)
        ));
    }
    lines.join("\n")
}

/// Trailing hint line for `pulpo ls` when `done` sessions are hidden by default
/// (see `Commands::List`): reports how many were hidden and how to see them.
/// `None` when nothing was hidden — no hint line needed.
pub fn format_hidden_sessions_hint(hidden_count: usize) -> Option<String> {
    if hidden_count == 0 {
        None
    } else {
        Some(format!("{hidden_count} done session(s) hidden — use --all"))
    }
}

/// Format intervention events as a table.
pub fn format_interventions(events: &[InterventionEventResponse]) -> String {
    if events.is_empty() {
        return "No intervention events.".into();
    }
    let mut lines = vec![format!("{:<8} {:<20} {}", "ID", "TIMESTAMP", "REASON")];
    for e in events {
        lines.push(format!("{:<8} {:<20} {}", e.id, e.created_at, e.reason));
    }
    lines.join("\n")
}

/// Compact a token count: `1234` -> `1.2K`, `4_500_000` -> `4.5M`.
#[allow(clippy::cast_precision_loss)]
fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Format an optional dollar amount, or "-" when absent. Every cost in the system comes
/// from a structured usage reader (there is no output-scraping fallback), so this is
/// always exact — no "estimated" marker.
fn fmt_cost(c: Option<f64>) -> String {
    c.map_or_else(|| "-".into(), |v| format!("${v:.2}"))
}

/// Append a labeled cost-rollup section (per-repo), most expensive first.
fn append_dimension_rollups(lines: &mut Vec<String>, heading: &str, rollups: &[DimensionRollup]) {
    if rollups.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(heading.to_owned());
    for r in rollups {
        lines.push(format!(
            "  {:<28} {} sessions  {} tokens  {}",
            truncate(&r.label, 28),
            r.session_count,
            fmt_tokens(r.total_tokens),
            fmt_cost(r.total_cost_usd),
        ));
    }
}

/// Format the exact per-session usage report (pulpo-managed sessions on this node) plus
/// its per-repo rollups.
pub fn format_usage_sessions(r: &UsageSessionsResponse) -> String {
    if r.sessions.is_empty() {
        return "No sessions with usage data.".into();
    }
    let mut lines = vec![format!(
        "{:<20} {:<8} {:>8} {:>8}",
        "SESSION", "SOURCE", "TOKENS", "COST"
    )];
    for s in &r.sessions {
        let source = s
            .usage_source
            .as_deref()
            .map_or("-", |src| src.strip_suffix("-jsonl").unwrap_or(src));
        lines.push(format!(
            "{:<20} {:<8} {:>8} {:>8}",
            truncate(&s.session_name, 20),
            source,
            fmt_tokens(s.total_tokens),
            fmt_cost(s.cost_usd),
        ));
    }

    append_dimension_rollups(&mut lines, "By repo:", &r.repos);

    lines.join("\n")
}

/// Append a titled section of scan rollups (label / tokens / cost), truncating labels to
/// `width`.
fn append_scan_rollups(lines: &mut Vec<String>, title: &str, rows: &[ScanRollup], width: usize) {
    lines.push(String::new());
    lines.push(title.to_owned());
    for row in rows {
        lines.push(format!(
            "  {:<width$} {:>9} tokens  {}",
            truncate(&row.label, width),
            fmt_tokens(row.total_tokens),
            fmt_cost(row.total_cost_usd),
        ));
    }
}

/// Format the read-only usage *scan* (all local agent history, by agent, model, and repo).
pub fn format_usage_scan(r: &UsageScanResponse) -> String {
    if r.by_agent.is_empty() {
        return "No local agent history found (looked in ~/.claude, ~/.codex, and ~/.pi).".into();
    }
    let total_cost = r
        .total_cost_usd
        .map(|c| format!("  ({})", fmt_cost(Some(c))))
        .unwrap_or_default();
    let window = r
        .window_days
        .map(|d| format!(", last {d}d"))
        .unwrap_or_default();
    let mut lines = vec![format!(
        "Local agent spend on {} — {} tokens{}{}",
        r.node_name,
        fmt_tokens(r.total_tokens),
        total_cost,
        window
    )];
    append_scan_rollups(&mut lines, "By agent:", &r.by_agent, 24);
    append_scan_rollups(&mut lines, "By model:", &r.by_model, 24);
    append_scan_rollups(&mut lines, "By repo:", &r.by_repo, 40);
    lines.join("\n")
}

/// Format worktree sessions as a table.
#[cfg_attr(coverage, allow(dead_code))]
pub fn format_cleanup_message(sessions: u64, worktrees: u64, logs: u64) -> String {
    if sessions == 0 && worktrees == 0 && logs == 0 {
        return "Nothing to clean up.".into();
    }
    let mut parts = Vec::new();
    if sessions > 0 {
        parts.push(format!("{sessions} session(s)"));
    }
    if worktrees > 0 {
        parts.push(format!("{worktrees} worktree(s)"));
    }
    if logs > 0 {
        parts.push(format!("{logs} log file(s)"));
    }
    format!("Cleaned up {}.", parts.join(", "))
}

#[cfg_attr(coverage, allow(dead_code))]
pub fn format_worktree_sessions(sessions: &[&Session]) -> String {
    if sessions.is_empty() {
        return "No worktree sessions.".into();
    }
    let mut lines = vec![format!(
        "{:<20} {:<20} {:<10} {}",
        "NAME", "BRANCH", "STATUS", "PATH"
    )];
    for s in sessions {
        let branch = s.worktree_branch.as_deref().unwrap_or("-");
        let path = s.worktree_path.as_deref().unwrap_or("-");
        lines.push(format!(
            "{:<20} {:<20} {:<10} {}",
            s.name,
            branch,
            format_status(s),
            path
        ));
    }
    lines.join("\n")
}

/// Format a list of schedules as a table.
#[cfg_attr(coverage, allow(dead_code))]
pub fn format_schedules(schedules: &[serde_json::Value]) -> String {
    if schedules.is_empty() {
        return "No schedules.".into();
    }
    let mut lines = vec![format!(
        "{:<20} {:<18} {:<8} {:<24} {}",
        "NAME", "CRON (local)", "ENABLED", "LAST RUN", "NODE"
    )];
    for s in schedules {
        let name = s["name"].as_str().unwrap_or("?");
        let cron = s["cron"].as_str().unwrap_or("?");
        let enabled = if s["enabled"].as_bool().unwrap_or(true) {
            "yes"
        } else {
            "no"
        };
        let last_run = s["last_run_at"]
            .as_str()
            .map_or_else(|| "-".to_owned(), format_local_time);
        lines.push(format!("{name:<20} {cron:<18} {enabled:<8} {last_run:<20}"));
    }
    lines.join("\n")
}

/// Format an RFC 3339 timestamp as local time (e.g., "2026-03-29 03:00 CET").
fn format_local_time(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339).map_or_else(
        |_| {
            // Fallback: truncate to ~16 chars (char-safe)
            let truncated: String = rfc3339.chars().take(16).collect();
            truncated
        },
        |dt| {
            let local = dt.with_timezone(&chrono::Local);
            local.format("%Y-%m-%d %H:%M %Z").to_string()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::session::Runtime;

    /// Helper to build a minimal `Session` for `format_branch` tests.
    fn repo_session(workdir: &str, branch: Option<&str>) -> Session {
        Session {
            name: "test".into(),
            workdir: workdir.into(),
            command: "echo".into(),
            status: pulpo_common::session::SessionStatus::Working,
            git_branch: branch.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn test_format_worktree_sessions_empty() {
        let output = format_worktree_sessions(&[]);
        assert_eq!(output, "No worktree sessions.");
    }

    #[test]
    fn test_format_worktree_sessions_with_data() {
        use pulpo_common::session::SessionStatus;

        let session = Session {
            name: "fix-auth".into(),
            workdir: "/tmp/repo".into(),
            command: "claude -p 'fix auth'".into(),
            status: SessionStatus::Working,
            worktree_path: Some("/home/user/.pulpo/worktrees/fix-auth".into()),
            worktree_branch: Some("fix-auth".into()),
            ..Default::default()
        };
        let sessions = vec![&session];
        let output = format_worktree_sessions(&sessions);
        assert!(output.contains("fix-auth"), "should show name: {output}");
        assert!(output.contains("working"), "should show status: {output}");
        assert!(
            output.contains("/home/user/.pulpo/worktrees/fix-auth"),
            "should show path: {output}"
        );
        assert!(output.contains("BRANCH"), "should have header: {output}");
    }

    #[test]
    fn test_format_worktree_sessions_no_branch() {
        use pulpo_common::session::SessionStatus;

        let session = Session {
            name: "old-session".into(),
            workdir: "/tmp".into(),
            command: "echo".into(),
            status: SessionStatus::Working,
            worktree_path: Some("/home/user/.pulpo/worktrees/old-session".into()),
            ..Default::default()
        };
        let sessions = vec![&session];
        let output = format_worktree_sessions(&sessions);
        assert!(
            output.contains('-'),
            "branch should show dash when None: {output}"
        );
    }

    #[test]
    fn test_format_worktree_sessions_renders_needs_input_badge() {
        use pulpo_common::session::SessionStatus;

        let session = Session {
            name: "blocked-wt".into(),
            workdir: "/tmp/repo".into(),
            command: "claude -p fix".into(),
            status: SessionStatus::Waiting,
            status_reason: Some("needs_input:permission".into()),
            worktree_path: Some("/home/user/.pulpo/worktrees/blocked-wt".into()),
            worktree_branch: Some("blocked-wt".into()),
            ..Default::default()
        };
        let sessions = vec![&session];
        let output = format_worktree_sessions(&sessions);
        assert!(
            output.contains("waiting (needs input: permission)"),
            "should render needs-input badge like `pulpo ls`: {output}"
        );
    }

    #[test]
    fn test_format_sessions_empty() {
        assert_eq!(format_sessions(&[]), "No sessions.");
    }

    #[test]
    fn test_format_sessions_with_data() {
        use pulpo_common::session::SessionStatus;

        let sessions = vec![Session {
            name: "my-api".into(),
            workdir: "/tmp/repo".into(),
            command: "claude -p 'Fix the bug'".into(),
            description: Some("Fix the bug".into()),
            status: SessionStatus::Working,
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("ID"));
        assert!(output.contains("NAME"));
        assert!(output.contains("BRANCH"));
        assert!(output.contains("COMMAND"));
        assert!(output.contains("00000000"));
        assert!(output.contains("my-api"));
        assert!(output.contains("working"));
        assert!(output.contains("claude -p 'Fix the bug'"));
    }

    #[test]
    fn test_format_branch_without_branch() {
        let s = repo_session("/home/user/test", None);
        assert_eq!(format_branch(&s), "-");
    }

    #[test]
    fn test_format_branch_with_branch() {
        let s = repo_session("/home/user/pulpo", Some("main"));
        assert_eq!(format_branch(&s), "main");
    }

    #[test]
    fn test_format_branch_with_diff_stats() {
        let mut s = repo_session("/home/user/pulpo", Some("main"));
        s.git_insertions = Some(42);
        s.git_deletions = Some(7);
        let result = format_branch(&s);
        assert_eq!(result, "main +42/-7");
    }

    #[test]
    fn test_format_branch_with_ahead() {
        let mut s = repo_session("/home/user/pulpo", Some("main"));
        s.git_ahead = Some(3);
        let result = format_branch(&s);
        assert!(result.contains("\u{2191}3"));
    }

    #[test]
    fn test_format_branch_zero_diff_hidden() {
        let mut s = repo_session("/home/user/pulpo", None);
        s.git_insertions = Some(0);
        s.git_deletions = Some(0);
        let result = format_branch(&s);
        assert!(!result.contains("+0/-0"));
    }

    #[test]
    fn test_format_branch_zero_ahead_hidden() {
        let mut s = repo_session("/home/user/pulpo", None);
        s.git_ahead = Some(0);
        let result = format_branch(&s);
        assert!(!result.contains('\u{2191}'));
    }

    #[test]
    fn test_format_sessions_with_git_branch() {
        use pulpo_common::session::SessionStatus;

        let sessions = vec![Session {
            name: "my-api".into(),
            workdir: "/tmp/repo".into(),
            command: "echo hello".into(),
            status: SessionStatus::Working,
            git_branch: Some("main".into()),
            git_commit: Some("abc1234".into()),
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("main"), "should show branch: {output}");
    }

    #[test]
    fn test_format_sessions_with_error_status() {
        use pulpo_common::session::SessionStatus;

        let mut meta = std::collections::HashMap::new();
        meta.insert("error_status".into(), "Compile error".into());
        let sessions = vec![Session {
            name: "my-api".into(),
            workdir: "/tmp/repo".into(),
            command: "echo hello".into(),
            status: SessionStatus::Working,
            metadata: Some(meta),
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("[!]"));
    }

    #[test]
    fn test_format_status_needs_input_distinct_from_plain_idle() {
        use pulpo_common::session::SessionStatus;

        let blocked = Session {
            status: SessionStatus::Waiting,
            status_reason: Some("needs_input:permission".into()),
            ..Default::default()
        };
        assert_eq!(format_status(&blocked), "waiting (needs input: permission)");

        let plain_idle = Session {
            status: SessionStatus::Waiting,
            status_reason: Some("idle".into()),
            ..Default::default()
        };
        assert_eq!(format_status(&plain_idle), "waiting (idle)");
    }

    #[test]
    fn test_format_status_no_reason_is_bare_word() {
        // `starting`/`working`/`lost` never carry a `status_reason` in practice, but
        // format defensively either way: with none set, just the bare status word.
        let session = Session {
            status: pulpo_common::session::SessionStatus::Working,
            ..Default::default()
        };
        assert_eq!(format_status(&session), "working");
    }

    #[test]
    fn test_format_status_done_variants() {
        use pulpo_common::session::SessionStatus;

        let exited_with_code = Session {
            status: SessionStatus::Done,
            status_reason: Some("exited".into()),
            exit_code: Some(0),
            ..Default::default()
        };
        assert_eq!(format_status(&exited_with_code), "done (exit 0)");

        let exited_no_code = Session {
            status: SessionStatus::Done,
            status_reason: Some("exited".into()),
            ..Default::default()
        };
        assert_eq!(format_status(&exited_no_code), "done (exited)");

        let stopped = Session {
            status: SessionStatus::Done,
            status_reason: Some("stopped".into()),
            ..Default::default()
        };
        assert_eq!(format_status(&stopped), "done (stopped)");

        let idle_timeout = Session {
            status: SessionStatus::Done,
            status_reason: Some("idle_timeout".into()),
            ..Default::default()
        };
        assert_eq!(format_status(&idle_timeout), "done (idle timeout)");

        let budget_exceeded = Session {
            status: SessionStatus::Done,
            status_reason: Some("budget_exceeded".into()),
            ..Default::default()
        };
        assert_eq!(format_status(&budget_exceeded), "done (budget exceeded)");

        let memory_pressure = Session {
            status: SessionStatus::Done,
            status_reason: Some("memory_pressure".into()),
            ..Default::default()
        };
        assert_eq!(format_status(&memory_pressure), "done (memory pressure)");
    }

    #[test]
    fn test_format_sessions_renders_needs_input_badge() {
        use pulpo_common::session::SessionStatus;

        let sessions = vec![Session {
            name: "blocked-sess".into(),
            workdir: "/tmp/repo".into(),
            command: "claude -p fix".into(),
            status: SessionStatus::Waiting,
            status_reason: Some("needs_input:question".into()),
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(
            output.contains("waiting (needs input: question)"),
            "should render needs-input badge: {output}"
        );
    }

    #[test]
    fn test_format_sessions_docker_runtime() {
        use pulpo_common::session::SessionStatus;

        let sessions = vec![Session {
            name: "sandbox-test".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Working,
            backend_session_id: Some("docker:pulpo-sandbox-test".into()),
            runtime: Runtime::Docker,
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(
            output.contains("sandbox-test"),
            "should show name: {output}"
        );
        assert!(
            output.contains('-'),
            "branch should show dash when None: {output}"
        );
    }

    #[test]
    fn test_format_sessions_long_command_truncated() {
        use pulpo_common::session::SessionStatus;

        let sessions = vec![Session {
            name: "test".into(),
            workdir: "/tmp".into(),
            command:
                "claude -p 'A very long command that exceeds fifty characters in total length here'"
                    .into(),
            status: SessionStatus::Done,
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("..."));
    }

    #[test]
    fn test_format_sessions_worktree_indicator() {
        use pulpo_common::session::SessionStatus;

        let sessions = vec![Session {
            name: "wt-task".into(),
            workdir: "/repo".into(),
            command: "claude".into(),
            status: SessionStatus::Working,
            worktree_path: Some("/home/user/.pulpo/worktrees/wt-task".into()),
            worktree_branch: Some("wt-task".into()),
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(
            output.contains("[wt]"),
            "should show worktree indicator: {output}"
        );
        assert!(output.contains("wt-task [wt]"));
    }

    #[test]
    fn test_format_sessions_pr_indicator() {
        use pulpo_common::session::SessionStatus;
        use std::collections::HashMap;

        let mut meta = HashMap::new();
        meta.insert("pr_url".into(), "https://github.com/a/b/pull/1".into());
        let sessions = vec![Session {
            name: "pr-task".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Working,
            metadata: Some(meta),
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(
            output.contains("[PR]"),
            "should show PR indicator: {output}"
        );
        assert!(output.contains("pr-task [PR]"));
    }

    #[test]
    fn test_format_sessions_worktree_and_pr_indicator() {
        use pulpo_common::session::SessionStatus;
        use std::collections::HashMap;

        let mut meta = HashMap::new();
        meta.insert("pr_url".into(), "https://github.com/a/b/pull/1".into());
        let sessions = vec![Session {
            name: "both-task".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Working,
            metadata: Some(meta),
            worktree_path: Some("/home/user/.pulpo/worktrees/both-task".into()),
            worktree_branch: Some("both-task".into()),
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(
            output.contains("[wt] [PR]"),
            "should show both indicators: {output}"
        );
    }

    #[test]
    fn test_format_sessions_no_pr_without_metadata() {
        use pulpo_common::session::SessionStatus;

        let sessions = vec![Session {
            name: "no-pr".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Working,
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(
            !output.contains("[PR]"),
            "should not show PR indicator: {output}"
        );
    }

    fn sample_usage() -> pulpo_common::api::SessionUsage {
        pulpo_common::api::SessionUsage {
            session_id: "id".into(),
            session_name: "my-task".into(),
            workdir: "/repo".into(),
            usage_source: Some("claude-jsonl".into()),
            total_tokens: 1_234_000,
            cost_usd: Some(2.5),
        }
    }

    #[test]
    fn test_fmt_tokens() {
        assert_eq!(fmt_tokens(500), "500");
        assert_eq!(fmt_tokens(1_500), "1.5K");
        assert_eq!(fmt_tokens(4_500_000), "4.5M");
    }

    #[test]
    fn test_fmt_cost() {
        assert_eq!(fmt_cost(None), "-");
        assert_eq!(fmt_cost(Some(1.234)), "$1.23");
    }

    #[test]
    fn test_format_usage_sessions_empty() {
        let resp = UsageSessionsResponse {
            node_name: "n".into(),
            generated_at: "t".into(),
            sessions: vec![],
            repos: vec![],
        };
        assert_eq!(format_usage_sessions(&resp), "No sessions with usage data.");
    }

    #[test]
    fn test_format_usage_sessions_with_sessions() {
        let resp = UsageSessionsResponse {
            node_name: "n".into(),
            generated_at: "t".into(),
            sessions: vec![sample_usage()],
            repos: vec![],
        };
        let out = format_usage_sessions(&resp);
        assert!(out.contains("SESSION"));
        assert!(out.contains("my-task"));
        assert!(out.contains("claude")); // source suffix stripped
        assert!(out.contains("1.2M"));
        assert!(out.contains("$2.50"));
    }

    #[test]
    fn test_format_usage_sessions_no_usage_source_shown_as_dash() {
        let mut s = sample_usage();
        s.usage_source = None;
        s.total_tokens = 0;
        s.cost_usd = None;
        let resp = UsageSessionsResponse {
            node_name: "n".into(),
            generated_at: "t".into(),
            sessions: vec![s],
            repos: vec![],
        };
        let out = format_usage_sessions(&resp);
        assert!(out.contains(" - "));
    }

    #[test]
    fn test_format_usage_sessions_shows_repo_rollups() {
        let resp = UsageSessionsResponse {
            node_name: "n".into(),
            generated_at: "t".into(),
            sessions: vec![sample_usage()],
            repos: vec![DimensionRollup {
                label: "/repos/api".into(),
                session_count: 3,
                total_tokens: 2_000_000,
                total_cost_usd: Some(40.0),
            }],
        };
        let out = format_usage_sessions(&resp);
        assert!(out.contains("By repo:"));
        assert!(out.contains("/repos/api"));
        assert!(out.contains("$40.00"));
    }

    #[test]
    fn test_format_usage_scan_report() {
        let r = UsageScanResponse {
            node_name: "mac-mini".into(),
            generated_at: "t".into(),
            window_days: Some(7),
            total_tokens: 2_500_000,
            total_cost_usd: Some(12.0),
            by_agent: vec![
                ScanRollup {
                    label: "claude".into(),
                    total_tokens: 1_500_000,
                    total_cost_usd: Some(12.0),
                },
                ScanRollup {
                    label: "codex".into(),
                    total_tokens: 1_000_000,
                    total_cost_usd: None,
                },
            ],
            by_model: vec![ScanRollup {
                label: "claude-opus-4-8".into(),
                total_tokens: 1_500_000,
                total_cost_usd: Some(12.0),
            }],
            by_repo: vec![ScanRollup {
                label: "/repos/api".into(),
                total_tokens: 2_000_000,
                total_cost_usd: Some(10.0),
            }],
        };
        let out = format_usage_scan(&r);
        assert!(out.contains("By agent:"));
        assert!(out.contains("claude"));
        assert!(out.contains("codex"));
        assert!(out.contains("By model:"));
        assert!(out.contains("claude-opus-4-8"));
        assert!(out.contains("By repo:"));
        assert!(out.contains("/repos/api"));
        assert!(out.contains("$12.00")); // total + claude cost
        assert!(out.contains("2.5M")); // total tokens compaction
        assert!(out.contains("last 7d")); // window annotation
    }

    #[test]
    fn test_format_usage_scan_empty() {
        let r = UsageScanResponse {
            node_name: "n".into(),
            generated_at: "t".into(),
            window_days: None,
            total_tokens: 0,
            total_cost_usd: None,
            by_agent: vec![],
            by_model: vec![],
            by_repo: vec![],
        };
        assert!(format_usage_scan(&r).contains("No local agent history"));
    }

    #[test]
    fn test_format_hidden_sessions_hint_none_when_zero() {
        assert_eq!(format_hidden_sessions_hint(0), None);
    }

    #[test]
    fn test_format_hidden_sessions_hint_singular() {
        assert_eq!(
            format_hidden_sessions_hint(1),
            Some("1 done session(s) hidden — use --all".to_owned())
        );
    }

    #[test]
    fn test_format_hidden_sessions_hint_plural() {
        assert_eq!(
            format_hidden_sessions_hint(3),
            Some("3 done session(s) hidden — use --all".to_owned())
        );
    }

    #[test]
    fn test_format_interventions_empty() {
        assert_eq!(format_interventions(&[]), "No intervention events.");
    }

    #[test]
    fn test_format_interventions_with_data() {
        let events = vec![
            InterventionEventResponse {
                id: 1,
                session_id: "sess-1".into(),
                code: None,
                reason: "Memory exceeded threshold".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
            },
            InterventionEventResponse {
                id: 2,
                session_id: "sess-1".into(),
                code: None,
                reason: "Idle for 10 minutes".into(),
                created_at: "2026-01-02T00:00:00Z".into(),
            },
        ];
        let output = format_interventions(&events);
        assert!(output.contains("ID"));
        assert!(output.contains("TIMESTAMP"));
        assert!(output.contains("REASON"));
        assert!(output.contains("Memory exceeded threshold"));
        assert!(output.contains("Idle for 10 minutes"));
        assert!(output.contains("2026-01-01T00:00:00Z"));
    }

    #[test]
    fn test_format_schedules_empty() {
        assert_eq!(format_schedules(&[]), "No schedules.");
    }

    #[test]
    fn test_format_sessions_multibyte_command_truncation() {
        use pulpo_common::session::SessionStatus;

        // Command with multi-byte chars exceeding 50 bytes; must not panic
        let sessions = vec![Session {
            name: "test".into(),
            workdir: "/tmp".into(),
            command: "echo '\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}'".into(),
            status: SessionStatus::Working,
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("..."));
    }

    #[test]
    fn test_format_local_time_valid_utc() {
        let result = format_local_time("2026-03-18T03:00:00Z");
        assert!(result.contains("2026-03-18"));
        // Should contain time and timezone indicator
        assert!(result.contains(':'));
    }

    #[test]
    fn test_format_local_time_valid_with_offset() {
        let result = format_local_time("2026-03-18T03:00:00+02:00");
        assert!(result.contains("2026-03-18"));
    }

    #[test]
    fn test_format_local_time_invalid_truncated() {
        let result = format_local_time("short");
        assert_eq!(result, "short");
    }

    #[test]
    fn test_format_local_time_invalid_long() {
        let result = format_local_time("not-a-valid-rfc3339-timestamp");
        assert_eq!(result.chars().count(), 16);
    }

    #[test]
    fn test_format_local_time_multibyte_safe() {
        // Multi-byte input should not panic
        let result = format_local_time("日本語テストの文字列です");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_format_token_count_small() {
        assert_eq!(format_token_count(999), "999");
    }

    #[test]
    fn test_format_token_count_thousands() {
        assert_eq!(format_token_count(1234), "1.2K");
    }

    #[test]
    fn test_format_token_count_millions() {
        assert_eq!(format_token_count(1_234_567), "1.2M");
    }

    #[test]
    fn test_format_token_count_exact_k() {
        assert_eq!(format_token_count(1000), "1.0K");
    }

    #[test]
    fn test_format_token_count_zero() {
        assert_eq!(format_token_count(0), "0");
    }

    #[test]
    fn test_format_usage_with_cost() {
        let mut session = repo_session("/tmp", None);
        let mut meta = std::collections::HashMap::new();
        meta.insert("session_cost_usd".into(), "0.550000".into());
        meta.insert("total_input_tokens".into(), "10000".into());
        session.metadata = Some(meta);
        // Cost takes priority over tokens
        assert_eq!(format_usage(&session), "$0.55");
    }

    #[test]
    fn test_format_usage_with_tokens_only() {
        let mut session = repo_session("/tmp", None);
        let mut meta = std::collections::HashMap::new();
        meta.insert("total_input_tokens".into(), "12345".into());
        session.metadata = Some(meta);
        assert_eq!(format_usage(&session), "12.3K tok");
    }

    #[test]
    fn test_format_usage_no_data() {
        let session = repo_session("/tmp", None);
        assert_eq!(format_usage(&session), "-");
    }

    #[test]
    fn test_format_sessions_includes_usage_header() {
        use pulpo_common::session::SessionStatus;

        let sessions = vec![Session {
            name: "test".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Working,
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("USAGE"));
    }

    #[test]
    fn test_cleanup_format_none_deleted() {
        assert_eq!(format_cleanup_message(0, 0, 0), "Nothing to clean up.");
    }

    #[test]
    fn test_cleanup_format_sessions_only() {
        assert_eq!(format_cleanup_message(3, 0, 0), "Cleaned up 3 session(s).");
    }

    #[test]
    fn test_cleanup_format_sessions_with_worktrees() {
        assert_eq!(
            format_cleanup_message(2, 2, 0),
            "Cleaned up 2 session(s), 2 worktree(s)."
        );
    }

    #[test]
    fn test_cleanup_format_all_three() {
        assert_eq!(
            format_cleanup_message(5, 3, 4),
            "Cleaned up 5 session(s), 3 worktree(s), 4 log file(s)."
        );
    }

    #[test]
    fn test_cleanup_format_orphans_only_no_dead_sessions() {
        // Orphan sweep can clean worktrees/logs even with zero dead sessions.
        assert_eq!(
            format_cleanup_message(0, 1, 2),
            "Cleaned up 1 worktree(s), 2 log file(s)."
        );
    }
}
