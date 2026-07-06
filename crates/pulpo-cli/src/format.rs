//! Terminal output rendering for the `pulpo` CLI: table and report
//! formatting for sessions, nodes, usage, schedules, secrets, and inks.
//!
//! Pure move from `lib.rs` — no logic changes.

use pulpo_common::api::{
    DimensionRollup, InterventionEventResponse, PeersResponse, ScanRollup, SessionProjection,
    UsageProjectionResponse, UsageScanResponse,
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
                s.status.to_string(),
                format_usage(s),
                format_branch(s),
                s.command.clone(),
            )
        })
        .collect();

    let w_id = 8;
    let w_name = rows.iter().map(|r| r.1.len()).max().unwrap_or(4).max(4);
    let w_status = 8;
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

/// Format the peers response as a table.
pub fn format_nodes(resp: &PeersResponse) -> String {
    let mut lines = vec![format!(
        "{:<20} {:<25} {:<10} {}",
        "NAME", "ADDRESS", "STATUS", "SESSIONS"
    )];
    lines.push(format!(
        "{:<20} {:<25} {:<10} {}",
        resp.local.name, "(local)", "online", "-"
    ));
    for p in &resp.peers {
        let sessions = p
            .session_count
            .map_or_else(|| "-".into(), |c| c.to_string());
        lines.push(format!(
            "{:<20} {:<25} {:<10} {}",
            p.name, p.address, p.status, sessions
        ));
    }
    lines.join("\n")
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

/// Format an optional dollar amount, or "-" when absent. Estimated (output-scraped)
/// costs are prefixed with `~`; exact costs from a structured reader are shown plainly.
fn fmt_cost(c: Option<f64>, exact: bool) -> String {
    c.map_or_else(
        || "-".into(),
        |v| {
            if exact {
                format!("${v:.2}")
            } else {
                format!("~${v:.2}")
            }
        },
    )
}

/// Format a per-hour dollar rate, or "-" when absent.
fn fmt_rate(c: Option<f64>) -> String {
    c.map_or_else(|| "-".into(), |v| format!("${v:.2}/h"))
}

/// Format one session's quota column: exact Codex %, estimated Claude ~%, or "-".
fn fmt_quota(s: &SessionProjection) -> String {
    s.quota_used_percent.map_or_else(
        || {
            s.allowance_used_percent
                .map_or_else(|| "-".into(), |pct| format!("~{pct:.0}%"))
        },
        |pct| format!("{pct:.0}%"),
    )
}

/// Append a labeled cost-rollup section (per-ink / per-repo), most expensive first.
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
            fmt_cost(r.total_cost_usd, r.cost_is_exact),
        ));
    }
}

/// Format the usage projection as session and account tables.
pub fn format_usage_projection(p: &UsageProjectionResponse) -> String {
    if p.sessions.is_empty() {
        return "No sessions with usage data.".into();
    }
    let mut lines = vec![format!(
        "{:<20} {:<8} {:>8} {:>8} {:>9} {:>6}",
        "SESSION", "SOURCE", "TOKENS", "COST", "$/HR", "QUOTA"
    )];
    for s in &p.sessions {
        let source = s
            .usage_source
            .as_deref()
            .map_or("scraped", |src| src.strip_suffix("-jsonl").unwrap_or(src));
        lines.push(format!(
            "{:<20} {:<8} {:>8} {:>8} {:>9} {:>6}",
            truncate(&s.session_name, 20),
            source,
            fmt_tokens(s.total_tokens),
            fmt_cost(s.cost_usd, s.usage_source.is_some()),
            fmt_rate(s.cost_per_hour),
            fmt_quota(s),
        ));
    }

    if !p.accounts.is_empty() {
        lines.push(String::new());
        lines.push("Accounts:".into());
        for a in &p.accounts {
            let who = a
                .email
                .clone()
                .or_else(|| a.provider.clone())
                .unwrap_or_else(|| "unknown".into());
            lines.push(format!(
                "  {:<24} {:<12} {} sessions  {} tokens  {}",
                who,
                a.pool,
                a.session_count,
                fmt_tokens(a.total_tokens),
                fmt_cost(a.total_cost_usd, a.cost_is_exact),
            ));
        }
    }

    append_dimension_rollups(&mut lines, "By ink:", &p.inks);
    append_dimension_rollups(&mut lines, "By repo:", &p.repos);

    lines.join(
        "
",
    )
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
            fmt_cost(row.total_cost_usd, true),
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
        .map(|c| format!("  ({})", fmt_cost(Some(c), true)))
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

/// Format secret entries as a table.
#[cfg_attr(coverage, allow(dead_code))]
pub fn format_secrets(secrets: &[serde_json::Value]) -> String {
    if secrets.is_empty() {
        return "No secrets configured.".into();
    }
    let mut lines = vec![format!("{:<24} {:<24} {}", "NAME", "ENV", "CREATED")];
    for s in secrets {
        let name = s["name"].as_str().unwrap_or("?");
        let env_display = s["env"]
            .as_str()
            .map_or_else(|| name.to_owned(), String::from);
        let created = s["created_at"]
            .as_str()
            .map_or("-", |t| if t.len() >= 16 { &t[..16] } else { t });
        lines.push(format!("{name:<24} {env_display:<24} {created}"));
    }
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
            s.name, branch, s.status, path
        ));
    }
    lines.join("\n")
}

/// Format a map of inks as a table.
#[cfg_attr(coverage, allow(dead_code))]
pub fn format_inks(inks: &serde_json::Map<String, serde_json::Value>) -> String {
    if inks.is_empty() {
        return "No inks configured.".into();
    }
    let mut lines = vec![format!(
        "{:<20} {:<12} {:<30} {}",
        "NAME", "RUNTIME", "COMMAND", "DESCRIPTION"
    )];
    let mut names: Vec<&String> = inks.keys().collect();
    names.sort();
    for name in names {
        let ink = &inks[name];
        let runtime = ink
            .get("runtime")
            .and_then(|v| v.as_str())
            .unwrap_or("tmux");
        let command = ink.get("command").and_then(|v| v.as_str()).unwrap_or("-");
        let desc = ink
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        // Truncate command for display (char-safe to avoid multi-byte panic)
        let cmd_display = if command.chars().count() > 28 {
            let truncated: String = command.chars().take(25).collect();
            format!("{truncated}...")
        } else {
            command.to_owned()
        };
        lines.push(format!("{name:<20} {runtime:<12} {cmd_display:<30} {desc}"));
    }
    lines.join("\n")
}

/// Format a single ink detail view.
#[cfg_attr(coverage, allow(dead_code))]
pub fn format_ink_detail(name: &str, ink: &serde_json::Value) -> String {
    let mut lines = vec![format!("Ink: {name}")];
    if let Some(desc) = ink.get("description").and_then(|v| v.as_str()) {
        lines.push(format!("  Description: {desc}"));
    }
    if let Some(cmd) = ink.get("command").and_then(|v| v.as_str()) {
        lines.push(format!("  Command:     {cmd}"));
    }
    if let Some(runtime) = ink.get("runtime").and_then(|v| v.as_str()) {
        lines.push(format!("  Runtime:     {runtime}"));
    }
    if let Some(secrets) = ink.get("secrets").and_then(|v| v.as_array())
        && !secrets.is_empty()
    {
        let names: Vec<&str> = secrets.iter().filter_map(|s| s.as_str()).collect();
        lines.push(format!("  Secrets:     {}", names.join(", ")));
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
            status: pulpo_common::session::SessionStatus::Active,
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
            status: SessionStatus::Active,
            worktree_path: Some("/home/user/.pulpo/worktrees/fix-auth".into()),
            worktree_branch: Some("fix-auth".into()),
            ..Default::default()
        };
        let sessions = vec![&session];
        let output = format_worktree_sessions(&sessions);
        assert!(output.contains("fix-auth"), "should show name: {output}");
        assert!(output.contains("active"), "should show status: {output}");
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
            status: SessionStatus::Active,
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
            status: SessionStatus::Active,
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("ID"));
        assert!(output.contains("NAME"));
        assert!(output.contains("BRANCH"));
        assert!(output.contains("COMMAND"));
        assert!(output.contains("00000000"));
        assert!(output.contains("my-api"));
        assert!(output.contains("active"));
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
            status: SessionStatus::Active,
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
            status: SessionStatus::Active,
            metadata: Some(meta),
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("[!]"));
    }

    #[test]
    fn test_format_sessions_docker_runtime() {
        use pulpo_common::session::SessionStatus;

        let sessions = vec![Session {
            name: "sandbox-test".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Active,
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
            status: SessionStatus::Ready,
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
            status: SessionStatus::Active,
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
            status: SessionStatus::Active,
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
            status: SessionStatus::Active,
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
            status: SessionStatus::Active,
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(
            !output.contains("[PR]"),
            "should not show PR indicator: {output}"
        );
    }

    #[test]
    fn test_format_nodes() {
        use pulpo_common::node::NodeInfo;
        use pulpo_common::peer::{PeerInfo, PeerSource, PeerStatus};

        let resp = PeersResponse {
            local: NodeInfo {
                name: "mac-mini".into(),
                hostname: "h".into(),
                os: "macos".into(),
                arch: "arm64".into(),
                cpus: 8,
                memory_mb: 16384,
                gpu: None,
            },
            peers: vec![PeerInfo {
                name: "win-pc".into(),
                address: "win-pc:7433".into(),
                status: PeerStatus::Online,
                node_info: None,
                session_count: Some(3),
                source: PeerSource::Configured,
            }],
        };
        let output = format_nodes(&resp);
        assert!(output.contains("mac-mini"));
        assert!(output.contains("(local)"));
        assert!(output.contains("win-pc"));
        assert!(output.contains('3'));
    }

    #[test]
    fn test_format_nodes_no_session_count() {
        use pulpo_common::node::NodeInfo;
        use pulpo_common::peer::{PeerInfo, PeerSource, PeerStatus};

        let resp = PeersResponse {
            local: NodeInfo {
                name: "local".into(),
                hostname: "h".into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                cpus: 4,
                memory_mb: 8192,
                gpu: None,
            },
            peers: vec![PeerInfo {
                name: "peer".into(),
                address: "peer:7433".into(),
                status: PeerStatus::Offline,
                node_info: None,
                session_count: None,
                source: PeerSource::Configured,
            }],
        };
        let output = format_nodes(&resp);
        assert!(output.contains("offline"));
        // No session count → shows "-"
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines[2].contains('-'));
    }

    fn sample_projection() -> SessionProjection {
        SessionProjection {
            session_id: "id".into(),
            session_name: "my-task".into(),
            ink: Some("coder".into()),
            workdir: "/repo".into(),
            usage_source: Some("claude-jsonl".into()),
            auth_provider: Some("claude.ai".into()),
            auth_plan: Some("max".into()),
            auth_email: Some("a@x.com".into()),
            pool: "subscription".into(),
            total_tokens: 1_234_000,
            cost_usd: Some(2.5),
            elapsed_secs: 3600,
            cost_per_hour: Some(2.5),
            tokens_per_hour: Some(1_234_000.0),
            quota_used_percent: None,
            quota_resets_at: None,
            allowance_tokens: Some(100_000_000),
            allowance_used_percent: Some(1.2),
            secs_to_allowance: None,
        }
    }

    #[test]
    fn test_fmt_tokens() {
        assert_eq!(fmt_tokens(500), "500");
        assert_eq!(fmt_tokens(1_500), "1.5K");
        assert_eq!(fmt_tokens(4_500_000), "4.5M");
    }

    #[test]
    fn test_fmt_cost_and_rate() {
        assert_eq!(fmt_cost(None, true), "-");
        assert_eq!(fmt_cost(None, false), "-");
        assert_eq!(fmt_cost(Some(1.234), true), "$1.23"); // exact → plain
        assert_eq!(fmt_cost(Some(1.234), false), "~$1.23"); // scraped → estimated marker
        assert_eq!(fmt_rate(None), "-");
        assert_eq!(fmt_rate(Some(2.0)), "$2.00/h");
    }

    #[test]
    fn test_fmt_quota_codex_exact_claude_estimate_none() {
        let mut s = sample_projection();
        assert_eq!(fmt_quota(&s), "~1%"); // claude estimate marked with ~
        s.allowance_used_percent = None;
        assert_eq!(fmt_quota(&s), "-");
        s.quota_used_percent = Some(42.0);
        assert_eq!(fmt_quota(&s), "42%"); // codex exact, no ~
    }

    #[test]
    fn test_format_usage_projection_empty() {
        let resp = UsageProjectionResponse {
            node_name: "n".into(),
            generated_at: "t".into(),
            sessions: vec![],
            accounts: vec![],
            inks: vec![],
            repos: vec![],
        };
        assert_eq!(
            format_usage_projection(&resp),
            "No sessions with usage data."
        );
    }

    #[test]
    fn test_format_usage_projection_with_sessions_and_accounts() {
        let resp = UsageProjectionResponse {
            node_name: "n".into(),
            generated_at: "t".into(),
            sessions: vec![sample_projection()],
            accounts: vec![pulpo_common::api::AccountRollup {
                provider: Some("claude.ai".into()),
                plan: Some("max".into()),
                email: Some("a@x.com".into()),
                pool: "subscription".into(),
                session_count: 1,
                total_tokens: 1_234_000,
                total_cost_usd: Some(2.5),
                cost_per_hour: Some(2.5),
                max_quota_used_percent: None,
                cost_is_exact: true,
            }],
            inks: vec![],
            repos: vec![],
        };
        let out = format_usage_projection(&resp);
        assert!(out.contains("SESSION"));
        assert!(out.contains("my-task"));
        assert!(out.contains("claude")); // source suffix stripped
        assert!(out.contains("1.2M"));
        assert!(out.contains("$2.50"));
        assert!(!out.contains("~$2.50")); // exact source → no estimate marker
        assert!(out.contains("Accounts:"));
        assert!(out.contains("a@x.com"));
        assert!(out.contains("subscription")); // pool shown
    }

    #[test]
    fn test_format_usage_projection_marks_scraped_cost_estimated() {
        let mut s = sample_projection();
        s.usage_source = None; // scraped → estimated
        let resp = UsageProjectionResponse {
            node_name: "n".into(),
            generated_at: "t".into(),
            sessions: vec![s],
            accounts: vec![pulpo_common::api::AccountRollup {
                provider: Some("gemini".into()),
                plan: None,
                email: None,
                pool: "subscription".into(),
                session_count: 1,
                total_tokens: 1_234_000,
                total_cost_usd: Some(2.5),
                cost_per_hour: Some(2.5),
                max_quota_used_percent: None,
                cost_is_exact: false,
            }],
            inks: vec![],
            repos: vec![],
        };
        let out = format_usage_projection(&resp);
        assert!(out.contains("scraped")); // source column
        assert!(out.contains("~$2.50")); // both session and account cost marked estimated
    }

    #[test]
    fn test_format_usage_projection_shows_ink_and_repo_rollups() {
        let resp = UsageProjectionResponse {
            node_name: "n".into(),
            generated_at: "t".into(),
            sessions: vec![sample_projection()],
            accounts: vec![],
            inks: vec![DimensionRollup {
                label: "nightly".into(),
                session_count: 2,
                total_tokens: 1_000_000,
                total_cost_usd: Some(11.0),
                cost_per_hour: Some(1.0),
                cost_is_exact: true,
            }],
            repos: vec![DimensionRollup {
                label: "/repos/api".into(),
                session_count: 3,
                total_tokens: 2_000_000,
                total_cost_usd: Some(40.0),
                cost_per_hour: None,
                cost_is_exact: false,
            }],
        };
        let out = format_usage_projection(&resp);
        assert!(out.contains("By ink:"));
        assert!(out.contains("nightly"));
        assert!(out.contains("$11.00")); // exact ink cost, no ~
        assert!(out.contains("By repo:"));
        assert!(out.contains("/repos/api"));
        assert!(out.contains("~$40.00")); // scraped repo cost → estimated marker
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
    fn test_format_secrets_empty() {
        let secrets: Vec<serde_json::Value> = vec![];
        assert_eq!(format_secrets(&secrets), "No secrets configured.");
    }

    #[test]
    fn test_format_secrets_with_entries() {
        let secrets = vec![
            serde_json::json!({"name": "GITHUB_TOKEN", "created_at": "2026-03-21T12:00:00Z"}),
            serde_json::json!({"name": "NPM_TOKEN", "created_at": "2026-03-20T10:30:00Z"}),
        ];
        let output = format_secrets(&secrets);
        assert!(output.contains("GITHUB_TOKEN"));
        assert!(output.contains("NPM_TOKEN"));
        assert!(output.contains("NAME"));
        assert!(output.contains("ENV"));
        assert!(output.contains("CREATED"));
    }

    #[test]
    fn test_format_secrets_with_env() {
        let secrets = vec![
            serde_json::json!({"name": "GH_WORK", "env": "GITHUB_TOKEN", "created_at": "2026-03-21T12:00:00Z"}),
            serde_json::json!({"name": "NPM_TOKEN", "created_at": "2026-03-20T10:30:00Z"}),
        ];
        let output = format_secrets(&secrets);
        assert!(output.contains("GH_WORK"));
        assert!(output.contains("GITHUB_TOKEN"));
        assert!(output.contains("NPM_TOKEN"));
    }

    #[test]
    fn test_format_secrets_short_timestamp() {
        let secrets = vec![serde_json::json!({"name": "KEY", "created_at": "now"})];
        let output = format_secrets(&secrets);
        assert!(output.contains("now"));
    }

    #[test]
    fn test_format_sessions_multibyte_command_truncation() {
        use pulpo_common::session::SessionStatus;

        // Command with multi-byte chars exceeding 50 bytes; must not panic
        let sessions = vec![Session {
            name: "test".into(),
            workdir: "/tmp".into(),
            command: "echo '\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}'".into(),
            status: SessionStatus::Active,
            ..Default::default()
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("..."));
    }

    #[test]
    fn test_format_inks_empty() {
        let inks = serde_json::Map::new();
        assert_eq!(format_inks(&inks), "No inks configured.");
    }

    #[test]
    fn test_format_inks_with_entries() {
        let mut inks = serde_json::Map::new();
        inks.insert(
            "coder".into(),
            serde_json::json!({
                "description": "A coder",
                "command": "claude -p 'code'",
                "runtime": "docker"
            }),
        );
        let output = format_inks(&inks);
        assert!(output.contains("coder"));
        assert!(output.contains("docker"));
        assert!(output.contains("A coder"));
    }

    #[test]
    fn test_format_inks_header() {
        let mut inks = serde_json::Map::new();
        inks.insert("test".into(), serde_json::json!({}));
        let output = format_inks(&inks);
        assert!(output.contains("NAME"));
        assert!(output.contains("RUNTIME"));
        assert!(output.contains("COMMAND"));
        assert!(output.contains("DESCRIPTION"));
    }

    #[test]
    fn test_format_inks_long_command_truncated() {
        let mut inks = serde_json::Map::new();
        inks.insert(
            "longcmd".into(),
            serde_json::json!({
                "command": "this is a very long command that exceeds the display limit for the table"
            }),
        );
        let output = format_inks(&inks);
        assert!(output.contains("..."));
    }

    #[test]
    fn test_format_ink_detail() {
        let ink = serde_json::json!({
            "description": "A coder ink",
            "command": "claude -p 'code'",
            "runtime": "docker",
            "secrets": ["GH_TOKEN", "NPM_TOKEN"]
        });
        let output = format_ink_detail("coder", &ink);
        assert!(output.contains("Ink: coder"));
        assert!(output.contains("A coder ink"));
        assert!(output.contains("claude -p 'code'"));
        assert!(output.contains("docker"));
        assert!(output.contains("GH_TOKEN, NPM_TOKEN"));
    }

    #[test]
    fn test_format_ink_detail_minimal() {
        let ink = serde_json::json!({});
        let output = format_ink_detail("bare", &ink);
        assert!(output.contains("Ink: bare"));
        assert!(!output.contains("Description"));
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
    fn test_format_inks_multibyte_command_safe() {
        let mut inks = serde_json::Map::new();
        inks.insert(
            "test".into(),
            serde_json::json!({
                "command": "日本語コマンドですこれは長い文字列で切り捨てテスト"
            }),
        );
        // Should not panic on multi-byte truncation
        let output = format_inks(&inks);
        assert!(output.contains("test"));
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
            status: SessionStatus::Active,
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
