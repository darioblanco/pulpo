use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use pulpo_common::api::SessionIndexEntry;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct SessionIndex {
    entries: Arc<RwLock<HashMap<String, SessionIndexEntry>>>,
    workers: Arc<RwLock<HashMap<String, DateTime<Utc>>>>,
}

impl SessionIndex {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            workers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Insert or update a session entry by `session_id`.
    pub async fn upsert(&self, entry: SessionIndexEntry) {
        let mut entries = self.entries.write().await;
        entries.insert(entry.session_id.clone(), entry);
    }

    /// Remove a session entry by `session_id`.
    pub async fn remove(&self, session_id: &str) {
        let mut entries = self.entries.write().await;
        entries.remove(session_id);
    }

    /// Get a session entry by `session_id`.
    pub async fn get(&self, session_id: &str) -> Option<SessionIndexEntry> {
        let entries = self.entries.read().await;
        entries.get(session_id).cloned()
    }

    /// List all session entries.
    pub async fn list_all(&self) -> Vec<SessionIndexEntry> {
        let entries = self.entries.read().await;
        entries.values().cloned().collect()
    }

    /// List session entries for a specific node.
    pub async fn list_by_node(&self, node_name: &str) -> Vec<SessionIndexEntry> {
        let entries = self.entries.read().await;
        entries
            .values()
            .filter(|e| e.node_name == node_name)
            .cloned()
            .collect()
    }

    /// Update the last-seen timestamp for a worker.
    pub async fn touch_worker(&self, node_name: &str) {
        self.touch_worker_at(node_name, Utc::now()).await;
    }

    /// Update the last-seen timestamp for a worker using an explicit timestamp.
    pub async fn touch_worker_at(&self, node_name: &str, seen_at: DateTime<Utc>) {
        let mut workers = self.workers.write().await;
        workers.insert(node_name.to_owned(), seen_at);
    }

    /// Find workers whose last-seen is older than `timeout`, mark their sessions
    /// as `"lost"`, and return the affected entries.
    pub async fn mark_stale_workers(&self, timeout: Duration) -> Vec<SessionIndexEntry> {
        self.mark_stale_workers_at(Utc::now(), timeout).await
    }

    /// Find workers whose last-seen is older than `timeout`, using the provided time.
    pub async fn mark_stale_workers_at(
        &self,
        now: DateTime<Utc>,
        timeout: Duration,
    ) -> Vec<SessionIndexEntry> {
        let workers = self.workers.read().await;

        let stale_nodes: Vec<String> = workers
            .iter()
            .filter(|(_, last_seen)| {
                now.signed_duration_since(**last_seen)
                    .to_std()
                    .is_ok_and(|age| age > timeout)
            })
            .map(|(name, _)| name.clone())
            .collect();
        drop(workers);

        if stale_nodes.is_empty() {
            return Vec::new();
        }

        let mut affected = Vec::new();
        {
            let mut entries = self.entries.write().await;
            for entry in entries.values_mut() {
                if stale_nodes.contains(&entry.node_name) && entry.status != "lost" {
                    "lost".clone_into(&mut entry.status);
                    affected.push(entry.clone());
                }
            }
        }

        affected
    }

    /// List all known worker names.
    pub async fn worker_names(&self) -> Vec<String> {
        let workers = self.workers.read().await;
        workers.keys().cloned().collect()
    }

    /// Restore a persisted worker heartbeat into the in-memory cache.
    pub async fn restore_worker(&self, node_name: &str, seen_at: DateTime<Utc>) {
        self.touch_worker_at(node_name, seen_at).await;
    }
}

impl Default for SessionIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn make_entry(session_id: &str, node_name: &str, status: &str) -> SessionIndexEntry {
        SessionIndexEntry {
            session_id: session_id.into(),
            node_name: node_name.into(),
            node_address: Some("10.0.0.1:7433".into()),
            session_name: format!("task-{session_id}"),
            status: status.into(),
            command: Some("claude -p 'build'".into()),
            updated_at: "2026-03-30T12:00:00Z".into(),
        }
    }

    #[tokio::test]
    async fn test_upsert_and_get() {
        let index = SessionIndex::new();
        let entry = make_entry("s1", "worker-1", "active");
        index.upsert(entry.clone()).await;

        let result = index.get("s1").await.unwrap();
        assert_eq!(result.session_id, "s1");
        assert_eq!(result.node_name, "worker-1");
        assert_eq!(result.status, "active");
        assert_eq!(result.session_name, "task-s1");
        assert_eq!(result.command, Some("claude -p 'build'".into()));
        assert_eq!(result.updated_at, "2026-03-30T12:00:00Z");
    }

    #[tokio::test]
    async fn test_upsert_updates_existing() {
        let index = SessionIndex::new();
        index.upsert(make_entry("s1", "worker-1", "active")).await;

        // Update the same session with new values
        let updated = SessionIndexEntry {
            session_id: "s1".into(),
            node_name: "worker-1".into(),
            node_address: Some("10.0.0.2:7433".into()),
            session_name: "renamed-task".into(),
            status: "idle".into(),
            command: None,
            updated_at: "2026-03-30T13:00:00Z".into(),
        };
        index.upsert(updated).await;

        let result = index.get("s1").await.unwrap();
        assert_eq!(result.status, "idle");
        assert_eq!(result.session_name, "renamed-task");
        assert_eq!(result.node_address, Some("10.0.0.2:7433".into()));
        assert!(result.command.is_none());
    }

    #[tokio::test]
    async fn test_remove() {
        let index = SessionIndex::new();
        index.upsert(make_entry("s1", "worker-1", "active")).await;
        index.remove("s1").await;
        assert!(index.get("s1").await.is_none());
    }

    #[tokio::test]
    async fn test_list_all() {
        let index = SessionIndex::new();
        index.upsert(make_entry("s1", "worker-1", "active")).await;
        index.upsert(make_entry("s2", "worker-1", "idle")).await;
        index.upsert(make_entry("s3", "worker-2", "active")).await;

        let all = index.list_all().await;
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn test_list_by_node() {
        let index = SessionIndex::new();
        index.upsert(make_entry("s1", "worker-1", "active")).await;
        index.upsert(make_entry("s2", "worker-1", "idle")).await;
        index.upsert(make_entry("s3", "worker-2", "active")).await;

        let w1 = index.list_by_node("worker-1").await;
        assert_eq!(w1.len(), 2);
        assert!(w1.iter().all(|e| e.node_name == "worker-1"));

        let w2 = index.list_by_node("worker-2").await;
        assert_eq!(w2.len(), 1);
        assert_eq!(w2[0].session_id, "s3");
    }

    #[tokio::test]
    async fn test_touch_and_stale() {
        let index = SessionIndex::new();
        index
            .touch_worker_at("worker-1", ts("2026-03-30T12:00:00Z"))
            .await;
        index.upsert(make_entry("s1", "worker-1", "active")).await;
        index.upsert(make_entry("s2", "worker-1", "idle")).await;

        let affected = index
            .mark_stale_workers_at(ts("2026-03-30T12:02:00Z"), Duration::from_secs(60))
            .await;
        assert_eq!(affected.len(), 2);
        assert!(affected.iter().all(|e| e.status == "lost"));

        // Verify the entries in the index are also updated
        let s1 = index.get("s1").await.unwrap();
        assert_eq!(s1.status, "lost");
    }

    #[tokio::test]
    async fn test_stale_does_not_affect_fresh_workers() {
        let index = SessionIndex::new();
        index
            .touch_worker_at("worker-1", ts("2026-03-30T12:00:00Z"))
            .await;
        index.upsert(make_entry("s1", "worker-1", "active")).await;

        let affected = index
            .mark_stale_workers_at(ts("2026-03-30T12:00:30Z"), Duration::from_secs(60))
            .await;
        assert!(affected.is_empty());

        // Entry should still be active
        let s1 = index.get("s1").await.unwrap();
        assert_eq!(s1.status, "active");
    }

    #[tokio::test]
    async fn test_worker_names() {
        let index = SessionIndex::new();
        index.touch_worker("worker-1").await;
        index.touch_worker("worker-2").await;
        index.touch_worker("worker-3").await;

        let mut names = index.worker_names().await;
        names.sort();
        assert_eq!(names, vec!["worker-1", "worker-2", "worker-3"]);
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let index = SessionIndex::new();
        assert!(index.get("missing").await.is_none());
    }

    #[tokio::test]
    async fn test_remove_nonexistent() {
        let index = SessionIndex::new();
        // Should not panic
        index.remove("missing").await;
    }

    #[tokio::test]
    async fn test_list_all_empty() {
        let index = SessionIndex::new();
        assert!(index.list_all().await.is_empty());
    }

    #[tokio::test]
    async fn test_list_by_node_empty() {
        let index = SessionIndex::new();
        assert!(index.list_by_node("worker-1").await.is_empty());
    }

    #[tokio::test]
    async fn test_worker_names_empty() {
        let index = SessionIndex::new();
        assert!(index.worker_names().await.is_empty());
    }

    #[tokio::test]
    async fn test_default() {
        let index = SessionIndex::default();
        assert!(index.list_all().await.is_empty());
        assert!(index.worker_names().await.is_empty());
    }

    #[tokio::test]
    async fn test_clone_shares_state() {
        let index = SessionIndex::new();
        let cloned = index.clone();
        index.upsert(make_entry("s1", "worker-1", "active")).await;
        // Clone shares the same Arc, so update visible from both
        let result = cloned.get("s1").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().session_id, "s1");
    }

    #[tokio::test]
    async fn test_debug() {
        let index = SessionIndex::new();
        let debug = format!("{index:?}");
        assert!(debug.contains("SessionIndex"));
    }

    #[tokio::test]
    async fn test_mark_stale_skips_already_lost() {
        let index = SessionIndex::new();
        index
            .touch_worker_at("worker-1", ts("2026-03-30T12:00:00Z"))
            .await;
        index.upsert(make_entry("s1", "worker-1", "lost")).await;

        // Already-lost sessions should not appear in the affected list
        let affected = index
            .mark_stale_workers_at(ts("2026-03-30T12:02:00Z"), Duration::from_secs(60))
            .await;
        assert!(affected.is_empty());
    }
}
