use std::sync::Arc;
use std::time::Duration;

use pulpo_common::event::{CultureEvent, PulpoEvent};
use tokio::sync::{RwLock, broadcast, watch};
use tracing::{info, warn};

use super::repo::CultureRepo;

/// Shared sync status updated by the background loop.
#[derive(Debug, Clone)]
pub struct SyncStatus {
    pub enabled: bool,
    pub last_sync: Option<String>,
    pub last_error: Option<String>,
    pub pending_commits: usize,
    pub total_syncs: u64,
}

impl SyncStatus {
    pub const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            last_sync: None,
            last_error: None,
            pending_commits: 0,
            total_syncs: 0,
        }
    }
}

/// Background loop that periodically pulls from the culture remote.
///
/// Follows the same `tokio::select!` + `watch` shutdown pattern used by the
/// watchdog and discovery loops.
#[cfg(not(coverage))]
pub async fn run_culture_sync_loop(
    repo: CultureRepo,
    interval: Duration,
    sync_scopes: Option<Vec<String>>,
    node_name: String,
    event_tx: broadcast::Sender<PulpoEvent>,
    sync_status: Arc<RwLock<SyncStatus>>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(interval);
    // Skip the first immediate tick — the daemon already does a pull on init
    tick.tick().await;

    loop {
        tokio::select! {
            _ = tick.tick() => {
                do_sync(
                    &repo,
                    sync_scopes.as_deref(),
                    &node_name,
                    &event_tx,
                    &sync_status,
                )
                .await;
            }
            _ = shutdown_rx.changed() => {
                info!("Culture sync loop shutting down");
                return;
            }
        }
    }
}

/// Stub for coverage builds.
#[cfg(coverage)]
pub async fn run_culture_sync_loop(
    _repo: CultureRepo,
    _interval: Duration,
    _sync_scopes: Option<Vec<String>>,
    _node_name: String,
    _event_tx: broadcast::Sender<PulpoEvent>,
    _sync_status: Arc<RwLock<SyncStatus>>,
    _shutdown_rx: watch::Receiver<bool>,
) {
}

/// Execute a single sync cycle: pull + optional scope filter + event emission.
async fn do_sync(
    repo: &CultureRepo,
    sync_scopes: Option<&[String]>,
    node_name: &str,
    event_tx: &broadcast::Sender<PulpoEvent>,
    sync_status: &Arc<RwLock<SyncStatus>>,
) {
    match repo.pull().await {
        Ok(result) => {
            if result.updated {
                // Apply scope filter after pull
                if let Err(e) = repo.filter_scopes(sync_scopes).await {
                    warn!("Culture scope filtering failed: {e}");
                }

                info!(conflicts = result.conflicts, "Culture synced from remote");

                let _ = event_tx.send(PulpoEvent::Culture(CultureEvent {
                    action: "synced".into(),
                    count: 1,
                    node_name: node_name.to_owned(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                }));
            }

            let pending = repo.pending_commit_count().await;
            let mut status = sync_status.write().await;
            status.last_sync = Some(chrono::Utc::now().to_rfc3339());
            status.last_error = None;
            status.pending_commits = pending;
            status.total_syncs += 1;
        }
        Err(e) => {
            warn!("Culture sync failed: {e}");
            let pending = repo.pending_commit_count().await;
            let mut status = sync_status.write().await;
            status.last_error = Some(e.to_string());
            status.pending_commits = pending;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_status_new_enabled() {
        let status = SyncStatus::new(true);
        assert!(status.enabled);
        assert!(status.last_sync.is_none());
        assert!(status.last_error.is_none());
        assert_eq!(status.pending_commits, 0);
        assert_eq!(status.total_syncs, 0);
    }

    #[test]
    fn test_sync_status_new_disabled() {
        let status = SyncStatus::new(false);
        assert!(!status.enabled);
    }

    #[test]
    fn test_sync_status_clone() {
        let status = SyncStatus::new(true);
        let cloned = status.clone();
        assert_eq!(status.enabled, cloned.enabled);
        assert_eq!(status.total_syncs, cloned.total_syncs);
    }

    #[test]
    fn test_sync_status_debug() {
        let status = SyncStatus::new(true);
        let debug = format!("{status:?}");
        assert!(debug.contains("SyncStatus"));
        assert!(debug.contains("enabled: true"));
    }

    #[tokio::test]
    async fn test_do_sync_no_remote() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let (event_tx, _) = broadcast::channel(16);
        let sync_status = Arc::new(RwLock::new(SyncStatus::new(false)));

        do_sync(&repo, None, "test-node", &event_tx, &sync_status).await;

        // Should record an error (no remote configured)
        assert!(sync_status.read().await.last_error.is_some());
    }

    #[tokio::test]
    async fn test_do_sync_with_remote_fetch_fails() {
        let tmpdir = tempfile::tempdir().unwrap();
        // Use a bogus remote that will fail on fetch
        let repo = CultureRepo::init(
            tmpdir.path().to_str().unwrap(),
            Some("/nonexistent/remote.git".into()),
        )
        .await
        .unwrap();
        let (event_tx, _) = broadcast::channel(16);
        let sync_status = Arc::new(RwLock::new(SyncStatus::new(true)));

        do_sync(&repo, None, "test-node", &event_tx, &sync_status).await;

        assert!(sync_status.read().await.last_error.is_some());
    }

    #[tokio::test]
    async fn test_do_sync_successful_pull() {
        // Create a bare remote
        let remote_dir = tempfile::tempdir().unwrap();
        let remote_path = remote_dir.path().to_str().unwrap();

        tokio::process::Command::new("git")
            .args(["init", "--bare"])
            .current_dir(remote_path)
            .output()
            .await
            .unwrap();

        // Init repo A without remote to avoid fire-and-forget push races
        let dir_a = tempfile::tempdir().unwrap();
        let repo_a = setup_remote_and_repo(remote_path, dir_a.path().to_str().unwrap()).await;

        // Init repo B (clone from remote)
        let dir_b = tempfile::tempdir().unwrap();
        let repo_b =
            CultureRepo::init(dir_b.path().to_str().unwrap(), Some(remote_path.to_owned()))
                .await
                .unwrap();

        // Make a change in repo A and push manually
        let culture = pulpo_common::culture::Culture {
            id: uuid::Uuid::new_v4(),
            session_id: uuid::Uuid::new_v4(),
            kind: pulpo_common::culture::CultureKind::Summary,
            scope_repo: None,
            scope_ink: None,
            title: "sync test".into(),
            body: "body".into(),
            tags: vec![],
            relevance: 0.5,
            created_at: chrono::Utc::now(),
            last_referenced_at: None,
        };
        repo_a.save(&culture).await.unwrap();
        git_push(&repo_a).await;

        // Sync repo B
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let sync_status = Arc::new(RwLock::new(SyncStatus::new(true)));

        do_sync(&repo_b, None, "node-b", &event_tx, &sync_status).await;

        assert!(sync_status.read().await.last_error.is_none());
        assert!(sync_status.read().await.last_sync.is_some());
        assert_eq!(sync_status.read().await.total_syncs, 1);

        // Check that an event was emitted
        let event = event_rx.try_recv().unwrap();
        assert!(matches!(
            event,
            PulpoEvent::Culture(ref ce) if ce.action == "synced" && ce.node_name == "node-b"
        ));

        // Verify repo B has the new culture
        let items = repo_b.list(None, None, None, None, None).unwrap();
        assert!(!items.is_empty());
        assert!(items.iter().any(|k| k.title == "sync test"));
    }

    /// Helper: init a bare remote, create repo A (no remote to avoid push races),
    /// add the remote manually, then push.
    async fn setup_remote_and_repo(remote_path: &str, data_dir: &str) -> CultureRepo {
        let repo = CultureRepo::init(data_dir, None).await.unwrap();

        // Add remote manually (CultureRepo::init skips remote setup when None)
        tokio::process::Command::new("git")
            .args(["remote", "add", "origin", remote_path])
            .current_dir(repo.root())
            .output()
            .await
            .unwrap();
        // Push initial commit
        tokio::process::Command::new("git")
            .args(["push", "-u", "origin", "main"])
            .current_dir(repo.root())
            .output()
            .await
            .unwrap();
        repo
    }

    /// Helper: force push from a repo to its remote.
    async fn git_push(repo: &CultureRepo) {
        tokio::process::Command::new("git")
            .args(["push", "origin", "main"])
            .current_dir(repo.root())
            .output()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_do_sync_with_conflict_resolution() {
        // Create a bare remote
        let remote_dir = tempfile::tempdir().unwrap();
        let remote_path = remote_dir.path().to_str().unwrap();

        tokio::process::Command::new("git")
            .args(["init", "--bare"])
            .current_dir(remote_path)
            .output()
            .await
            .unwrap();

        // Init repo A without remote (avoids fire-and-forget push races)
        let dir_a = tempfile::tempdir().unwrap();
        let repo_a = setup_remote_and_repo(remote_path, dir_a.path().to_str().unwrap()).await;

        // Init repo B from remote
        let dir_b = tempfile::tempdir().unwrap();
        let repo_b =
            CultureRepo::init(dir_b.path().to_str().unwrap(), Some(remote_path.to_owned()))
                .await
                .unwrap();

        // Repo A: save a culture item (no fire-and-forget push — no remote on repo_a)
        let culture_a = pulpo_common::culture::Culture {
            id: uuid::Uuid::new_v4(),
            session_id: uuid::Uuid::new_v4(),
            kind: pulpo_common::culture::CultureKind::Summary,
            scope_repo: None,
            scope_ink: None,
            title: "from-a".into(),
            body: "body-a".into(),
            tags: vec![],
            relevance: 0.5,
            created_at: chrono::Utc::now(),
            last_referenced_at: None,
        };
        repo_a.save(&culture_a).await.unwrap();
        git_push(&repo_a).await;

        // Repo B: save a different culture item (creates diverged history)
        let culture_b = pulpo_common::culture::Culture {
            id: uuid::Uuid::new_v4(),
            session_id: uuid::Uuid::new_v4(),
            kind: pulpo_common::culture::CultureKind::Summary,
            scope_repo: None,
            scope_ink: None,
            title: "from-b".into(),
            body: "body-b".into(),
            tags: vec![],
            relevance: 0.5,
            created_at: chrono::Utc::now(),
            last_referenced_at: None,
        };
        repo_b.save(&culture_b).await.unwrap();

        // Pull should resolve the conflict
        let (event_tx, _) = broadcast::channel(16);
        let sync_status = Arc::new(RwLock::new(SyncStatus::new(true)));

        do_sync(&repo_b, None, "node-b", &event_tx, &sync_status).await;

        // Either rebase succeeded or merge fallback worked — no error
        assert!(sync_status.read().await.last_error.is_none());
        assert!(sync_status.read().await.last_sync.is_some());
    }

    #[tokio::test]
    async fn test_do_sync_no_updates_available() {
        // Create a bare remote
        let remote_dir = tempfile::tempdir().unwrap();
        let remote_path = remote_dir.path().to_str().unwrap();

        tokio::process::Command::new("git")
            .args(["init", "--bare"])
            .current_dir(remote_path)
            .output()
            .await
            .unwrap();

        // Init without remote to avoid races, then add remote and push
        let dir = tempfile::tempdir().unwrap();
        let _initial = setup_remote_and_repo(remote_path, dir.path().to_str().unwrap()).await;
        // Re-init with remote for the pull test
        let repo = CultureRepo::init(dir.path().to_str().unwrap(), Some(remote_path.to_owned()))
            .await
            .unwrap();

        let (event_tx, mut event_rx) = broadcast::channel(16);
        let sync_status = Arc::new(RwLock::new(SyncStatus::new(true)));

        do_sync(&repo, None, "test-node", &event_tx, &sync_status).await;

        assert!(sync_status.read().await.last_error.is_none());
        assert!(sync_status.read().await.last_sync.is_some());
        assert_eq!(sync_status.read().await.total_syncs, 1);

        // No event should be emitted (nothing updated)
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_do_sync_with_scope_filter() {
        // Create a bare remote
        let remote_dir = tempfile::tempdir().unwrap();
        let remote_path = remote_dir.path().to_str().unwrap();

        tokio::process::Command::new("git")
            .args(["init", "--bare"])
            .current_dir(remote_path)
            .output()
            .await
            .unwrap();

        // Init repo A without remote to avoid fire-and-forget push races
        let dir_a = tempfile::tempdir().unwrap();
        let repo_a = setup_remote_and_repo(remote_path, dir_a.path().to_str().unwrap()).await;

        // Init repo B from remote
        let dir_b = tempfile::tempdir().unwrap();
        let repo_b =
            CultureRepo::init(dir_b.path().to_str().unwrap(), Some(remote_path.to_owned()))
                .await
                .unwrap();

        // Repo A saves culture in two scopes (no fire-and-forget since no remote)
        let culture_global = pulpo_common::culture::Culture {
            id: uuid::Uuid::new_v4(),
            session_id: uuid::Uuid::new_v4(),
            kind: pulpo_common::culture::CultureKind::Summary,
            scope_repo: None,
            scope_ink: None,
            title: "global-item".into(),
            body: "global".into(),
            tags: vec![],
            relevance: 0.5,
            created_at: chrono::Utc::now(),
            last_referenced_at: None,
        };
        repo_a.save(&culture_global).await.unwrap();

        let culture_repo = pulpo_common::culture::Culture {
            id: uuid::Uuid::new_v4(),
            session_id: uuid::Uuid::new_v4(),
            kind: pulpo_common::culture::CultureKind::Summary,
            scope_repo: Some("/my/repo".into()),
            scope_ink: None,
            title: "repo-item".into(),
            body: "scoped".into(),
            tags: vec![],
            relevance: 0.5,
            created_at: chrono::Utc::now(),
            last_referenced_at: None,
        };
        repo_a.save(&culture_repo).await.unwrap();
        git_push(&repo_a).await;

        // Sync repo B with scope filter — only allow "culture/"
        let scopes = vec!["culture".into()];
        let (event_tx, _) = broadcast::channel(16);
        let sync_status = Arc::new(RwLock::new(SyncStatus::new(true)));

        do_sync(&repo_b, Some(&scopes), "node-b", &event_tx, &sync_status).await;

        assert!(sync_status.read().await.last_error.is_none());
    }

    #[cfg(coverage)]
    #[tokio::test]
    async fn test_run_culture_sync_loop_coverage_stub() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let (event_tx, _) = broadcast::channel(16);
        let sync_status = Arc::new(RwLock::new(SyncStatus::new(false)));
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        run_culture_sync_loop(
            repo,
            Duration::from_secs(60),
            None,
            "test".into(),
            event_tx,
            sync_status,
            shutdown_rx,
        )
        .await;
    }
}
