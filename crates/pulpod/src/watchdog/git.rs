#[cfg(not(coverage))]
use pulpo_common::session::SessionStatus;
#[cfg(not(coverage))]
use tracing::{debug, warn};

use crate::store::Store;

/// Detect and update git branch/commit info for active and idle sessions.
/// Gated with `cfg(not(coverage))` because it requires real git commands.
#[cfg(not(coverage))]
#[allow(clippy::too_many_lines)]
pub(super) async fn update_git_info(store: &Store) {
    let sessions = match store.list_sessions().await {
        Ok(s) => s,
        Err(e) => {
            warn!("Watchdog: failed to list sessions for git info: {e}");
            return;
        }
    };

    let live: Vec<_> = sessions
        .into_iter()
        .filter(|s| s.status == SessionStatus::Active || s.status == SessionStatus::Idle)
        .collect();

    for session in live {
        let effective_dir = session
            .worktree_path
            .as_deref()
            .unwrap_or(&session.workdir)
            .to_owned();
        let session_id = session.id.to_string();
        let old_branch = session.git_branch.clone();
        let old_commit = session.git_commit.clone();

        let old_files_changed = session.git_files_changed;
        let old_insertions = session.git_insertions;
        let old_deletions = session.git_deletions;
        let old_ahead = session.git_ahead;

        let result = tokio::task::spawn_blocking(move || {
            let branch = std::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(&effective_dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
            let commit = std::process::Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .current_dir(&effective_dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());

            let diff_stat = std::process::Command::new("git")
                .args(["diff", "--shortstat", "HEAD"])
                .current_dir(&effective_dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| {
                    let out = String::from_utf8_lossy(&o.stdout).to_string();
                    super::output_patterns::parse_git_shortstat(&out)
                });

            let ahead = std::process::Command::new("git")
                .args(["rev-list", "--count", "@{upstream}..HEAD"])
                .current_dir(&effective_dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .parse::<u32>()
                        .ok()
                });

            (branch, commit, diff_stat, ahead)
        })
        .await;

        match result {
            Ok((branch, commit, diff_stat, ahead)) => {
                if (branch != old_branch || commit != old_commit)
                    && let Err(e) = store
                        .update_session_git_info(&session_id, branch.as_deref(), commit.as_deref())
                        .await
                {
                    warn!("Watchdog: failed to update git info for {session_id}: {e}");
                }

                if let Some((files, ins, del)) = diff_stat
                    && (files != old_files_changed || ins != old_insertions || del != old_deletions)
                    && let Err(e) = store
                        .update_session_git_diff(&session_id, files, ins, del)
                        .await
                {
                    warn!("Watchdog: failed to update git diff for {session_id}: {e}");
                }

                if ahead != old_ahead
                    && let Err(e) = store.update_session_git_ahead(&session_id, ahead).await
                {
                    warn!("Watchdog: failed to update git ahead for {session_id}: {e}");
                }
            }
            Err(e) => {
                debug!("Watchdog: git info task failed for {session_id}: {e}");
            }
        }
    }
}

/// No-op stub under coverage builds (real git commands not available in test).
#[cfg(coverage)]
pub(super) async fn update_git_info(_store: &Store) {}
