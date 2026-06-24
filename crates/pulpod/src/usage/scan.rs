//! Read-only usage scan: total agent spend across *all* local history.
//!
//! Unlike [`super::projection`] (which covers pulpo-managed sessions), the scan reads every
//! Claude/Codex session file on the machine and reports spend by agent and by repo — the
//! low-friction "what did my agents cost?" view. It needs no behavior change: it meters
//! sessions you ran however you ran them (raw terminal, another tool, cron), and unifies
//! Claude + Codex into one report — the cross-agent view a single-vendor `/usage` can't give.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use pulpo_common::api::{ScanRollup, UsageScanResponse};

use super::{ExactUsage, RateOverrides, claude, codex};

/// Total tokens across every dimension we track (matches the projection convention).
const fn exact_total_tokens(u: &ExactUsage) -> u64 {
    u.input_tokens + u.output_tokens + u.cache_write_tokens + u.cache_read_tokens
}

/// Scan all local Claude + Codex history into per-agent and per-repo spend rollups.
///
/// `resolve_repo` maps each recorded working directory to the label it's grouped under.
/// Pass [`canonical_repo`] to collapse git worktrees and subdirectories onto their origin
/// repository (the default), or the identity function to keep every directory distinct
/// (`pulpo usage --scan --by-worktree`). Results are memoized so each distinct directory is
/// resolved at most once.
pub fn scan_usage(
    claude_dir: &Path,
    codex_dir: &Path,
    rates: &RateOverrides,
    node_name: &str,
    now: DateTime<Utc>,
    resolve_repo: impl Fn(&str) -> String,
) -> UsageScanResponse {
    // Epoch = no time filter; the scan is all-time.
    let epoch = DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_else(Utc::now);

    // Memoized directory -> group label, so a repo's worktrees only pay one git call each.
    let mut repo_cache: HashMap<String, String> = HashMap::new();
    let mut resolve = |cwd: String| -> String {
        if let Some(label) = repo_cache.get(&cwd) {
            return label.clone();
        }
        let label = resolve_repo(&cwd);
        repo_cache.insert(cwd, label.clone());
        label
    };

    // repo path -> (tokens, cost). Merged across agents.
    let mut by_repo: HashMap<String, (u64, Option<f64>)> = HashMap::new();
    let mut claude_tokens = 0u64;
    let mut claude_cost: Option<f64> = None;

    // Claude: one project directory per repo; label by the recorded cwd when present.
    if let Ok(entries) = std::fs::read_dir(claude_dir.join("projects")) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(d) = claude::read_usage_dir(&dir, epoch, rates) else {
                continue;
            };
            let tokens = exact_total_tokens(&d.usage);
            claude_tokens += tokens;
            if let Some(c) = d.usage.cost_usd {
                claude_cost = Some(claude_cost.unwrap_or(0.0) + c);
            }
            let raw = d.cwd.unwrap_or_else(|| {
                dir.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_owned()
            });
            let repo = resolve(raw);
            let e = by_repo.entry(repo).or_insert((0, None));
            e.0 += tokens;
            if let Some(c) = d.usage.cost_usd {
                e.1 = Some(e.1.unwrap_or(0.0) + c);
            }
        }
    }

    // Codex: per-repo token totals (no per-token cost).
    let codex_by_repo = codex::scan_by_cwd(codex_dir);
    let codex_tokens: u64 = codex_by_repo.values().copied().sum();
    for (raw, tokens) in codex_by_repo {
        let repo = resolve(raw);
        by_repo.entry(repo).or_insert((0, None)).0 += tokens;
    }

    let mut by_repo: Vec<ScanRollup> = by_repo
        .into_iter()
        .map(|(label, (total_tokens, total_cost_usd))| ScanRollup {
            label,
            total_tokens,
            total_cost_usd,
        })
        .collect();
    sort_rollups(&mut by_repo);

    let mut by_agent = Vec::new();
    if claude_tokens > 0 {
        by_agent.push(ScanRollup {
            label: "claude".into(),
            total_tokens: claude_tokens,
            total_cost_usd: claude_cost,
        });
    }
    if codex_tokens > 0 {
        by_agent.push(ScanRollup {
            label: "codex".into(),
            total_tokens: codex_tokens,
            total_cost_usd: None,
        });
    }
    sort_rollups(&mut by_agent);

    UsageScanResponse {
        node_name: node_name.to_owned(),
        generated_at: now.to_rfc3339(),
        total_tokens: claude_tokens + codex_tokens,
        total_cost_usd: claude_cost,
        by_agent,
        by_repo,
    }
}

/// Sort rollups most-expensive-first (priced before unpriced), then by tokens, then label.
fn sort_rollups(rollups: &mut [ScanRollup]) {
    rollups.sort_by(|a, b| {
        b.total_cost_usd
            .unwrap_or(-1.0)
            .partial_cmp(&a.total_cost_usd.unwrap_or(-1.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.total_tokens.cmp(&a.total_tokens))
            .then(a.label.cmp(&b.label))
    });
}

/// Resolve a working directory to its canonical repository root, collapsing git worktrees
/// and subdirectories onto the origin repo.
///
/// Every worktree of a repo shares one common git dir (`<origin>/.git`); its parent is the
/// origin root, and any subdirectory resolves there too. Falls back to the input path when
/// `cwd` isn't a git repo or `git` is unavailable, so non-repo directories stay distinct —
/// this is what makes per-repo spend mean "this repo" rather than "this checkout".
///
/// Real `git` invocation, hence coverage-excluded: the merge logic is covered with an
/// injected resolver, and this function is exercised against real worktrees in the
/// (non-coverage) test job.
#[cfg(not(coverage))]
pub(crate) fn canonical_repo(cwd: &str) -> String {
    use std::process::Command;
    let output = Command::new("git")
        .args([
            "-C",
            cwd,
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])
        .output();
    if let Ok(output) = output
        && output.status.success()
    {
        let common = String::from_utf8_lossy(&output.stdout);
        if let Some(root) = Path::new(common.trim()).parent().and_then(Path::to_str)
            && !root.is_empty()
        {
            return root.to_owned();
        }
    }
    cwd.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn claude_record(cwd: Option<&str>, model: &str, input: u64, output: u64) -> String {
        let ts = Utc::now().to_rfc3339();
        let cwd_field = cwd.map(|c| format!(r#""cwd":"{c}","#)).unwrap_or_default();
        format!(
            r#"{{"timestamp":"{ts}",{cwd_field}"requestId":"r1","type":"assistant","message":{{"id":"m1","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":{output},"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
        )
    }

    fn write_claude_project(claude_dir: &Path, dir_name: &str, content: &str) {
        let d = claude_dir.join("projects").join(dir_name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("session.jsonl"), content).unwrap();
    }

    fn codex_rollout(cwd: &str, input: u64, cached: u64, output: u64) -> String {
        let meta = format!(
            r#"{{"timestamp":"2026-06-12T10:00:00Z","type":"session_meta","payload":{{"id":"abc","timestamp":"2026-06-12T10:00:00Z","cwd":"{cwd}","originator":"codex_cli_rs"}}}}"#
        );
        let tc = format!(
            r#"{{"timestamp":"2026-06-12T10:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output},"total_tokens":{}}}}}}}}}"#,
            input + output
        );
        format!("{meta}\n{tc}\n")
    }

    fn write_codex_rollout(codex_dir: &Path, name: &str, content: &str) {
        let d = codex_dir
            .join("sessions")
            .join("2026")
            .join("06")
            .join("12");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(name), content).unwrap();
    }

    #[test]
    fn test_scan_empty_dirs() {
        let claude = tempfile::tempdir().unwrap();
        let codex = tempfile::tempdir().unwrap();
        let r = scan_usage(
            claude.path(),
            codex.path(),
            &RateOverrides::default(),
            "n",
            Utc::now(),
            |s: &str| s.to_owned(),
        );
        assert_eq!(r.total_tokens, 0);
        assert!(r.by_agent.is_empty());
        assert!(r.by_repo.is_empty());
        assert_eq!(r.total_cost_usd, None);
    }

    #[test]
    fn test_scan_merges_agents_by_repo() {
        let claude = tempfile::tempdir().unwrap();
        let codex = tempfile::tempdir().unwrap();
        write_claude_project(
            claude.path(),
            "proj-api",
            &claude_record(Some("/repos/api"), "claude-opus-4-8", 1000, 500),
        );
        write_claude_project(
            claude.path(),
            "proj-web",
            &claude_record(Some("/repos/web"), "claude-opus-4-8", 200, 100),
        );
        // Codex also worked in /repos/api → must merge with Claude's /repos/api.
        write_codex_rollout(
            codex.path(),
            "rollout-2026-06-12-a.jsonl",
            &codex_rollout("/repos/api", 800, 0, 200),
        );

        // Identity resolver: each cwd stays its own row, but Claude+Codex /repos/api still
        // merge (same raw key) — this also exercises the resolver memo's cache-hit path.
        let r = scan_usage(
            claude.path(),
            codex.path(),
            &RateOverrides::default(),
            "node-x",
            Utc::now(),
            |s: &str| s.to_owned(),
        );

        // Two agents present.
        assert_eq!(r.by_agent.len(), 2);
        // 1500 (claude api) + 300 (claude web) + 1000 (codex api).
        assert_eq!(r.total_tokens, 2800);
        // Cost only from Claude (opus $5/$25 per MTok), Codex contributes none.
        // 1000·5 + 500·25 + 200·5 + 100·25 = 21000 micro-dollars.
        let expected = 21_000.0 / 1_000_000.0;
        assert!((r.total_cost_usd.unwrap() - expected).abs() < 1e-9);

        // /repos/api merges both agents: 1500 + 1000 = 2500 tokens, and is the priciest → first.
        assert_eq!(r.by_repo[0].label, "/repos/api");
        assert_eq!(r.by_repo[0].total_tokens, 2500);
        let web = r.by_repo.iter().find(|x| x.label == "/repos/web").unwrap();
        assert_eq!(web.total_tokens, 300);
    }

    #[test]
    fn test_scan_falls_back_to_dir_name_without_cwd() {
        let claude = tempfile::tempdir().unwrap();
        let codex = tempfile::tempdir().unwrap();
        // No cwd in the record → label is the (sanitized) project dir name.
        write_claude_project(
            claude.path(),
            "-Users-x-repo",
            &claude_record(None, "claude-opus-4-8", 10, 5),
        );
        let r = scan_usage(
            claude.path(),
            codex.path(),
            &RateOverrides::default(),
            "n",
            Utc::now(),
            |s: &str| s.to_owned(),
        );
        assert_eq!(r.by_repo.len(), 1);
        assert_eq!(r.by_repo[0].label, "-Users-x-repo");
    }

    #[test]
    fn test_scan_collapses_worktrees_via_resolver() {
        let claude = tempfile::tempdir().unwrap();
        let codex = tempfile::tempdir().unwrap();
        // The repo itself and a worktree of it, as two separate Claude project dirs.
        write_claude_project(
            claude.path(),
            "proj-main",
            &claude_record(Some("/repos/api"), "claude-opus-4-8", 1000, 0),
        );
        write_claude_project(
            claude.path(),
            "proj-wt",
            &claude_record(Some("/repos/api-worktrees/feat"), "claude-opus-4-8", 500, 0),
        );
        // Codex worked in a subdirectory of the same repo.
        write_codex_rollout(
            codex.path(),
            "rollout-2026-06-12-a.jsonl",
            &codex_rollout("/repos/api/src", 200, 0, 0),
        );

        // Resolver standing in for `canonical_repo`: everything under the repo collapses.
        let resolve = |cwd: &str| {
            if cwd.starts_with("/repos/api") {
                "/repos/api".to_owned()
            } else {
                cwd.to_owned()
            }
        };
        let r = scan_usage(
            claude.path(),
            codex.path(),
            &RateOverrides::default(),
            "n",
            Utc::now(),
            resolve,
        );

        // All three checkouts collapse into a single repo row.
        assert_eq!(r.by_repo.len(), 1);
        assert_eq!(r.by_repo[0].label, "/repos/api");
        assert_eq!(r.by_repo[0].total_tokens, 1700);
    }

    /// Exercises the real git-backed resolver in the (non-coverage) test job.
    #[cfg(not(coverage))]
    #[test]
    fn test_canonical_repo_collapses_worktree_and_subdir() {
        use std::process::Command;
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("origin");
        fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str], dir: &Path| {
            let ok = Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?} failed");
        };
        git(&["init", "-q"], &repo);
        git(&["config", "user.email", "t@t"], &repo);
        git(&["config", "user.name", "t"], &repo);
        fs::write(repo.join("f"), "x").unwrap();
        git(&["add", "-A"], &repo);
        git(&["commit", "-qm", "init"], &repo);

        // A subdirectory and a linked worktree both resolve to the same origin root.
        let sub = repo.join("src");
        fs::create_dir_all(&sub).unwrap();
        let wt = tmp.path().join("wt");
        git(&["worktree", "add", "-q", wt.to_str().unwrap()], &repo);

        let root = canonical_repo(repo.to_str().unwrap());
        assert_eq!(canonical_repo(sub.to_str().unwrap()), root);
        assert_eq!(canonical_repo(wt.to_str().unwrap()), root);

        // A non-git directory stays itself.
        let plain = tmp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        assert_eq!(
            canonical_repo(plain.to_str().unwrap()),
            plain.to_str().unwrap()
        );
    }
}
