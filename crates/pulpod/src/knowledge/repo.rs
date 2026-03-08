use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use pulpo_common::knowledge::{Knowledge, KnowledgeKind};
use tokio::process::Command;
use tracing::{debug, warn};

/// Git-backed knowledge repository.
///
/// Stores knowledge items as Markdown files with YAML frontmatter in a local
/// git repo, optionally synced to a remote. Directory structure:
///
/// ```text
/// <root>/
///   repos/<slug>/
///     summary-<date>-<id>.md
///     failure-<date>-<id>.md
///   inks/<ink>/
///     summary-<date>-<id>.md
///   culture/
///     summary-<date>-<id>.md
/// ```
///
/// - `repos/<slug>/` — scoped to a specific working directory / repository
/// - `inks/<ink>/` — scoped to an ink (but not a specific repo)
/// - `culture/` — global knowledge, not scoped to any repo or ink
#[derive(Clone, Debug)]
pub struct KnowledgeRepo {
    root: PathBuf,
    remote: Option<String>,
}

impl KnowledgeRepo {
    /// Initialise (or open) the knowledge git repo.
    pub async fn init(data_dir: &str, remote: Option<String>) -> Result<Self> {
        let root = PathBuf::from(data_dir).join("knowledge");
        std::fs::create_dir_all(&root)
            .with_context(|| format!("create knowledge dir: {}", root.display()))?;

        // git init (idempotent)
        if !root.join(".git").exists() {
            run_git(&root, &["init"]).await?;
            // Configure for automation
            run_git(&root, &["config", "user.email", "pulpo@localhost"]).await?;
            run_git(&root, &["config", "user.name", "pulpo"]).await?;
        }

        // Set up remote if configured and not already present
        if let Some(url) = &remote {
            let has_origin = run_git(&root, &["remote", "get-url", "origin"])
                .await
                .is_ok();
            if !has_origin {
                run_git(&root, &["remote", "add", "origin", url]).await?;
            }
            // Best-effort pull
            if let Err(e) = run_git(&root, &["pull", "--rebase", "origin", "main"]).await {
                debug!("knowledge pull skipped (expected on first use): {e}");
            }
        }

        Ok(Self { root, remote })
    }

    /// Persist a knowledge item as a Markdown file with YAML frontmatter.
    pub async fn save(&self, knowledge: &Knowledge) -> Result<()> {
        let dir = self.item_dir(knowledge);
        std::fs::create_dir_all(&dir)?;

        let filename = item_filename(knowledge);
        let path = dir.join(&filename);

        let content = serialize_to_markdown(knowledge)?;
        std::fs::write(&path, &content)?;

        // Relative path for git add
        let rel = path
            .strip_prefix(&self.root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        run_git(&self.root, &["add", &rel]).await?;

        let msg = format!("knowledge: {}", knowledge.title);
        run_git(&self.root, &["commit", "-m", &msg]).await?;

        // Fire-and-forget push
        if self.remote.is_some() {
            let root = self.root.clone();
            tokio::spawn(async move {
                if let Err(e) = run_git(&root, &["push", "origin", "main"]).await {
                    warn!("knowledge push failed (will retry next commit): {e}");
                }
            });
        }

        Ok(())
    }

    /// List knowledge items, optionally filtered.
    pub fn list(
        &self,
        session_id: Option<&str>,
        kind: Option<&str>,
        repo: Option<&str>,
        ink: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Knowledge>> {
        let mut items = self.read_all()?;

        // Apply filters
        if let Some(sid) = session_id {
            items.retain(|k| k.session_id.to_string() == sid);
        }
        if let Some(kind_str) = kind {
            items.retain(|k| k.kind.to_string() == kind_str);
        }
        if let Some(r) = repo {
            items.retain(|k| k.scope_repo.as_deref() == Some(r));
        }
        if let Some(i) = ink {
            items.retain(|k| k.scope_ink.as_deref() == Some(i));
        }

        // Sort by created_at DESC
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        if let Some(lim) = limit {
            items.truncate(lim);
        }

        Ok(items)
    }

    /// Query knowledge relevant to a workdir/ink combination for context injection.
    /// Returns items scoped to the repo, the ink, or global, ordered by relevance.
    pub fn query_context(
        &self,
        workdir: Option<&str>,
        ink: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Knowledge>> {
        let mut items = self.read_all()?;

        // Match: scope_repo is None (global) OR matches workdir
        // AND:   scope_ink is None (any) OR matches ink
        items.retain(|k| {
            let repo_match =
                k.scope_repo.is_none() || (workdir.is_some() && k.scope_repo.as_deref() == workdir);
            let ink_match =
                k.scope_ink.is_none() || (ink.is_some() && k.scope_ink.as_deref() == ink);
            repo_match && ink_match
        });

        // Sort by relevance DESC, then created_at DESC
        items.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.created_at.cmp(&a.created_at))
        });

        items.truncate(limit);
        Ok(items)
    }

    /// Delete a knowledge item by ID. Returns true if found and deleted.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let path = self.find_by_id(id)?;
        let Some(path) = path else {
            return Ok(false);
        };

        let rel = path
            .strip_prefix(&self.root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        std::fs::remove_file(&path)?;
        run_git(&self.root, &["add", &rel]).await?;
        run_git(
            &self.root,
            &["commit", "-m", &format!("knowledge: delete {id}")],
        )
        .await?;

        self.fire_and_forget_push();
        Ok(true)
    }

    /// Delete all knowledge for a session. Returns count deleted.
    pub async fn delete_by_session(&self, session_id: &str) -> Result<usize> {
        let items = self.read_all()?;
        let to_delete: Vec<_> = items
            .iter()
            .filter(|k| k.session_id.to_string() == session_id)
            .collect();

        if to_delete.is_empty() {
            return Ok(0);
        }

        let count = to_delete.len();
        for item in &to_delete {
            if let Some(path) = self.find_by_id(&item.id.to_string())? {
                std::fs::remove_file(&path)?;
                let rel = path
                    .strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                run_git(&self.root, &["add", &rel]).await?;
            }
        }

        run_git(
            &self.root,
            &[
                "commit",
                "-m",
                &format!("knowledge: delete session {session_id}"),
            ],
        )
        .await?;

        self.fire_and_forget_push();
        Ok(count)
    }

    /// Get a single knowledge item by ID.
    pub fn get_by_id(&self, id: &str) -> Result<Option<Knowledge>> {
        let path = self.find_by_id(id)?;
        let Some(path) = path else {
            return Ok(None);
        };
        let content = std::fs::read_to_string(&path)?;
        Ok(parse_file(&path, &content))
    }

    /// Update a knowledge item. Only non-`None` fields are patched.
    pub async fn update(
        &self,
        id: &str,
        title: Option<&str>,
        body: Option<&str>,
        tags: Option<&[String]>,
        relevance: Option<f64>,
    ) -> Result<bool> {
        let path = self.find_by_id(id)?;
        let Some(path) = path else {
            return Ok(false);
        };
        let content = std::fs::read_to_string(&path)?;
        let Some(mut item) = parse_file(&path, &content) else {
            return Ok(false);
        };

        if let Some(t) = title {
            item.title = t.to_owned();
        }
        if let Some(b) = body {
            item.body = b.to_owned();
        }
        if let Some(t) = tags {
            item.tags = t.to_vec();
        }
        if let Some(r) = relevance {
            item.relevance = r;
        }

        let md = serialize_to_markdown(&item)?;
        std::fs::write(&path, md)?;

        let rel = path
            .strip_prefix(&self.root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        run_git(&self.root, &["add", &rel]).await?;
        run_git(
            &self.root,
            &["commit", "-m", &format!("knowledge: update {id}")],
        )
        .await?;

        self.fire_and_forget_push();
        Ok(true)
    }

    /// Explicitly push all local commits to the remote.
    /// Returns an error if no remote is configured or push fails.
    pub async fn push(&self) -> Result<()> {
        let Some(ref _url) = self.remote else {
            bail!("no remote configured for knowledge repo");
        };
        run_git(&self.root, &["push", "origin", "main"]).await?;
        Ok(())
    }

    /// The root path of the knowledge repo.
    pub fn root(&self) -> &Path {
        &self.root
    }

    // ── Private helpers ─────────────────────────────────────────────────

    /// Determine the directory for a knowledge item.
    ///
    /// - `scope_repo` set → `repos/<slug>/`
    /// - `scope_ink` set (no repo) → `inks/<ink>/`
    /// - neither → `culture/`
    fn item_dir(&self, k: &Knowledge) -> PathBuf {
        k.scope_repo.as_ref().map_or_else(
            || {
                k.scope_ink.as_ref().map_or_else(
                    || self.root.join("culture"),
                    |ink| self.root.join("inks").join(ink),
                )
            },
            |repo| self.root.join("repos").join(repo_slug(repo)),
        )
    }

    /// Read all knowledge files from the repo (`.md` and legacy `.json`).
    fn read_all(&self) -> Result<Vec<Knowledge>> {
        let mut items = Vec::new();
        for dir_name in &["repos", "inks", "culture"] {
            let base = self.root.join(dir_name);
            if base.exists() {
                collect_knowledge_files(&base, &mut items)?;
            }
        }
        Ok(items)
    }

    /// Find a knowledge file by its UUID.
    fn find_by_id(&self, id: &str) -> Result<Option<PathBuf>> {
        let all = self.read_all_paths()?;
        for path in all {
            if let Ok(content) = std::fs::read_to_string(&path)
                && let Some(k) = parse_file(&path, &content)
                && k.id.to_string() == id
            {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    /// Collect all knowledge file paths (`.md` and legacy `.json`).
    fn read_all_paths(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for dir_name in &["repos", "inks", "culture"] {
            let base = self.root.join(dir_name);
            if base.exists() {
                collect_knowledge_paths(&base, &mut paths)?;
            }
        }
        Ok(paths)
    }

    fn fire_and_forget_push(&self) {
        if self.remote.is_some() {
            let root = self.root.clone();
            tokio::spawn(async move {
                if let Err(e) = run_git(&root, &["push", "origin", "main"]).await {
                    warn!("knowledge push failed: {e}");
                }
            });
        }
    }
}

// ── Markdown serialization ──────────────────────────────────────────────

/// YAML frontmatter metadata for knowledge files.
#[derive(serde::Serialize, serde::Deserialize)]
struct KnowledgeFrontmatter {
    id: uuid::Uuid,
    session_id: uuid::Uuid,
    kind: KnowledgeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope_repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope_ink: Option<String>,
    title: String,
    tags: Vec<String>,
    relevance: f64,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Serialize a `Knowledge` item to Markdown with YAML frontmatter.
///
/// Format:
/// ```text
/// ---
/// id: <uuid>
/// session_id: <uuid>
/// kind: summary
/// title: "Some finding"
/// tags: [claude, completed]
/// relevance: 0.5
/// created_at: "2026-03-08T12:00:00Z"
/// ---
///
/// Body content here...
/// ```
fn serialize_to_markdown(k: &Knowledge) -> Result<String> {
    let frontmatter = KnowledgeFrontmatter {
        id: k.id,
        session_id: k.session_id,
        kind: k.kind,
        scope_repo: k.scope_repo.clone(),
        scope_ink: k.scope_ink.clone(),
        title: k.title.clone(),
        tags: k.tags.clone(),
        relevance: k.relevance,
        created_at: k.created_at,
    };
    let yaml = serde_yaml::to_string(&frontmatter)?;
    Ok(format!("---\n{yaml}---\n\n{}\n", k.body))
}

/// Parse a Markdown file with YAML frontmatter into a `Knowledge` item.
fn parse_from_markdown(content: &str) -> Result<Knowledge> {
    let content = content.trim();
    if !content.starts_with("---") {
        bail!("missing YAML frontmatter delimiter");
    }

    let after_first = &content[3..];
    let end = after_first
        .find("\n---")
        .context("missing closing frontmatter delimiter")?;
    let yaml_str = &after_first[..end];
    let body_start = end + 4; // skip "\n---"
    let body = after_first[body_start..].trim().to_owned();

    let fm: KnowledgeFrontmatter = serde_yaml::from_str(yaml_str)?;

    Ok(Knowledge {
        id: fm.id,
        session_id: fm.session_id,
        kind: fm.kind,
        scope_repo: fm.scope_repo,
        scope_ink: fm.scope_ink,
        title: fm.title,
        body,
        tags: fm.tags,
        relevance: fm.relevance,
        created_at: fm.created_at,
    })
}

/// Parse a file as either Markdown (`.md`) or legacy JSON (`.json`).
fn parse_file(path: &Path, content: &str) -> Option<Knowledge> {
    if path.extension().is_some_and(|ext| ext == "md") {
        match parse_from_markdown(content) {
            Ok(k) => Some(k),
            Err(e) => {
                warn!("skip invalid knowledge markdown {}: {e}", path.display());
                None
            }
        }
    } else if path.extension().is_some_and(|ext| ext == "json") {
        // Legacy JSON backward compatibility
        match serde_json::from_str::<Knowledge>(content) {
            Ok(k) => Some(k),
            Err(e) => {
                warn!("skip invalid knowledge json {}: {e}", path.display());
                None
            }
        }
    } else {
        None
    }
}

// ── File helpers ────────────────────────────────────────────────────────

/// Run a git command in the given directory.
async fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .with_context(|| format!("git {}", args.join(" ")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
}

/// Derive a filesystem-safe slug from a repo path.
fn repo_slug(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| sanitize_slug(path), |n| n.to_string_lossy().to_string())
}

/// Sanitize a string for use as a filename.
fn sanitize_slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Generate a filename for a knowledge item.
fn item_filename(k: &Knowledge) -> String {
    let kind_str = match k.kind {
        KnowledgeKind::Summary => "summary",
        KnowledgeKind::Failure => "failure",
    };
    let date = k.created_at.format("%Y-%m-%d");
    let id_short = &k.id.to_string()[..8];
    format!("{kind_str}-{date}-{id_short}.md")
}

/// Recursively collect all knowledge files (`.md` and legacy `.json`) and parse them.
fn collect_knowledge_files(dir: &Path, items: &mut Vec<Knowledge>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_knowledge_files(&path, items)?;
        } else if is_knowledge_file(&path) {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    if let Some(k) = parse_file(&path, &content) {
                        items.push(k);
                    }
                }
                Err(e) => warn!("skip unreadable knowledge file {}: {e}", path.display()),
            }
        }
    }
    Ok(())
}

/// Recursively collect all knowledge file paths (`.md` and legacy `.json`).
fn collect_knowledge_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_knowledge_paths(&path, paths)?;
        } else if is_knowledge_file(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

/// Check if a path is a knowledge file (`.md` or legacy `.json`).
fn is_knowledge_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "md" || ext == "json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_knowledge(title: &str, repo: Option<&str>, ink: Option<&str>) -> Knowledge {
        Knowledge {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            kind: KnowledgeKind::Summary,
            scope_repo: repo.map(Into::into),
            scope_ink: ink.map(Into::into),
            title: title.into(),
            body: "Test body".into(),
            tags: vec!["claude".into()],
            relevance: 0.5,
            created_at: Utc::now(),
        }
    }

    fn make_failure(title: &str, repo: Option<&str>) -> Knowledge {
        Knowledge {
            kind: KnowledgeKind::Failure,
            relevance: 0.8,
            ..make_knowledge(title, repo, None)
        }
    }

    // ── Markdown serialization tests ────────────────────────────────────

    #[test]
    fn test_serialize_to_markdown() {
        let k = make_knowledge("test finding", Some("/tmp/repo"), Some("coder"));
        let md = serialize_to_markdown(&k).unwrap();
        assert!(md.starts_with("---\n"));
        assert!(md.contains("title: test finding"));
        assert!(md.contains("kind: summary"));
        assert!(md.contains("\n---\n"));
        assert!(md.contains("Test body"));
    }

    #[test]
    fn test_parse_from_markdown() {
        let k = make_knowledge("roundtrip", Some("/tmp/repo"), Some("coder"));
        let md = serialize_to_markdown(&k).unwrap();
        let parsed = parse_from_markdown(&md).unwrap();
        assert_eq!(parsed.id, k.id);
        assert_eq!(parsed.session_id, k.session_id);
        assert_eq!(parsed.kind, k.kind);
        assert_eq!(parsed.scope_repo, k.scope_repo);
        assert_eq!(parsed.scope_ink, k.scope_ink);
        assert_eq!(parsed.title, k.title);
        assert_eq!(parsed.body, k.body);
        assert_eq!(parsed.tags, k.tags);
        assert!((parsed.relevance - k.relevance).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_from_markdown_no_frontmatter() {
        let err = parse_from_markdown("just body text").unwrap_err();
        assert!(err.to_string().contains("frontmatter"));
    }

    #[test]
    fn test_parse_from_markdown_no_closing_delimiter() {
        let err = parse_from_markdown("---\ntitle: test\nbody text").unwrap_err();
        assert!(err.to_string().contains("closing frontmatter"));
    }

    #[test]
    fn test_serialize_roundtrip_failure_kind() {
        let k = make_failure("fail roundtrip", Some("/tmp"));
        let md = serialize_to_markdown(&k).unwrap();
        let parsed = parse_from_markdown(&md).unwrap();
        assert_eq!(parsed.kind, KnowledgeKind::Failure);
        assert_eq!(parsed.title, "fail roundtrip");
    }

    #[test]
    fn test_serialize_omits_none_scopes() {
        let k = make_knowledge("global", None, None);
        let md = serialize_to_markdown(&k).unwrap();
        assert!(!md.contains("scope_repo"));
        assert!(!md.contains("scope_ink"));
    }

    #[test]
    fn test_parse_file_md() {
        let k = make_knowledge("md test", Some("/tmp"), None);
        let md = serialize_to_markdown(&k).unwrap();
        let path = Path::new("test.md");
        let parsed = parse_file(path, &md);
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap().title, "md test");
    }

    #[test]
    fn test_parse_file_json_legacy() {
        let k = make_knowledge("json test", Some("/tmp"), None);
        let json = serde_json::to_string(&k).unwrap();
        let path = Path::new("test.json");
        let parsed = parse_file(path, &json);
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap().title, "json test");
    }

    #[test]
    fn test_parse_file_unknown_ext() {
        let path = Path::new("test.txt");
        let parsed = parse_file(path, "whatever");
        assert!(parsed.is_none());
    }

    #[test]
    fn test_parse_file_invalid_md() {
        let path = Path::new("bad.md");
        let parsed = parse_file(path, "not valid markdown frontmatter");
        assert!(parsed.is_none());
    }

    #[test]
    fn test_parse_file_invalid_json() {
        let path = Path::new("bad.json");
        let parsed = parse_file(path, "not valid json");
        assert!(parsed.is_none());
    }

    // ── Filename and directory tests ────────────────────────────────────

    #[test]
    fn test_repo_slug() {
        assert_eq!(repo_slug("/Users/dario/Code/pulpo"), "pulpo");
        assert_eq!(repo_slug("/tmp/repo"), "repo");
        assert_eq!(repo_slug("single"), "single");
    }

    #[test]
    fn test_repo_slug_root_path() {
        // Path::new("/").file_name() returns None
        let slug = repo_slug("/");
        assert!(!slug.is_empty());
    }

    #[test]
    fn test_sanitize_slug() {
        assert_eq!(sanitize_slug("hello world!"), "hello-world-");
        assert_eq!(sanitize_slug("my_repo-123"), "my_repo-123");
    }

    #[test]
    fn test_item_filename() {
        let k = make_knowledge("test", Some("/tmp/repo"), None);
        let name = item_filename(&k);
        assert!(name.starts_with("summary-"));
        assert_eq!(std::path::Path::new(&name).extension().unwrap(), "md");
    }

    #[test]
    fn test_item_filename_failure() {
        let k = make_failure("fail", Some("/tmp/repo"));
        let name = item_filename(&k);
        assert!(name.starts_with("failure-"));
        assert_eq!(std::path::Path::new(&name).extension().unwrap(), "md");
    }

    #[test]
    fn test_item_dir_with_scope_repo() {
        let repo = KnowledgeRepo {
            root: PathBuf::from("/data/knowledge"),
            remote: None,
        };
        let k = make_knowledge("test", Some("/tmp/myrepo"), None);
        let dir = repo.item_dir(&k);
        assert_eq!(dir, PathBuf::from("/data/knowledge/repos/myrepo"));
    }

    #[test]
    fn test_item_dir_with_scope_ink_only() {
        let repo = KnowledgeRepo {
            root: PathBuf::from("/data/knowledge"),
            remote: None,
        };
        let k = make_knowledge("test", None, Some("reviewer"));
        let dir = repo.item_dir(&k);
        assert_eq!(dir, PathBuf::from("/data/knowledge/inks/reviewer"));
    }

    #[test]
    fn test_item_dir_culture() {
        let repo = KnowledgeRepo {
            root: PathBuf::from("/data/knowledge"),
            remote: None,
        };
        let k = make_knowledge("test", None, None);
        let dir = repo.item_dir(&k);
        assert_eq!(dir, PathBuf::from("/data/knowledge/culture"));
    }

    #[test]
    fn test_item_dir_repo_takes_precedence_over_ink() {
        let repo = KnowledgeRepo {
            root: PathBuf::from("/data/knowledge"),
            remote: None,
        };
        // When both scope_repo and scope_ink are set, repo wins
        let k = make_knowledge("test", Some("/tmp/myrepo"), Some("coder"));
        let dir = repo.item_dir(&k);
        assert_eq!(dir, PathBuf::from("/data/knowledge/repos/myrepo"));
    }

    #[test]
    fn test_is_knowledge_file() {
        assert!(is_knowledge_file(Path::new("test.md")));
        assert!(is_knowledge_file(Path::new("test.json")));
        assert!(!is_knowledge_file(Path::new("test.txt")));
        assert!(!is_knowledge_file(Path::new("test")));
    }

    // ── Integration tests (git-backed) ──────────────────────────────────

    #[tokio::test]
    async fn test_init_creates_git_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        assert!(repo.root().join(".git").exists());
    }

    #[tokio::test]
    async fn test_init_idempotent() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();
        KnowledgeRepo::init(data_dir, None).await.unwrap();
        // Second init should not error
        KnowledgeRepo::init(data_dir, None).await.unwrap();
    }

    #[tokio::test]
    async fn test_save_and_list() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("finding-1", Some("/tmp/repo"), Some("coder"));
        repo.save(&k).await.unwrap();

        let items = repo.list(None, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "finding-1");
        assert_eq!(items[0].id, k.id);
    }

    #[tokio::test]
    async fn test_save_creates_markdown_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("md-test", Some("/tmp/repo"), None);
        repo.save(&k).await.unwrap();

        // Verify the file is a .md file with YAML frontmatter
        let repo_dir = repo.root().join("repos").join("repo");
        let entries: Vec<_> = std::fs::read_dir(&repo_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 1);
        let path = entries[0].path();
        assert_eq!(path.extension().unwrap(), "md");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("title: md-test"));
        assert!(content.contains("Test body"));
    }

    #[tokio::test]
    async fn test_save_creates_git_commit() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("committed", Some("/tmp/repo"), None);
        repo.save(&k).await.unwrap();

        // Verify git log has a commit
        let log = run_git(repo.root(), &["log", "--oneline"]).await.unwrap();
        assert!(log.contains("knowledge:"));
    }

    #[tokio::test]
    async fn test_list_filter_by_kind() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        repo.save(&make_knowledge("sum", Some("/tmp"), None))
            .await
            .unwrap();
        repo.save(&make_failure("fail", Some("/tmp")))
            .await
            .unwrap();

        let summaries = repo.list(None, Some("summary"), None, None, None).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].title, "sum");

        let failures = repo.list(None, Some("failure"), None, None, None).unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].title, "fail");
    }

    #[tokio::test]
    async fn test_list_filter_by_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        repo.save(&make_knowledge("a", Some("/repo/a"), None))
            .await
            .unwrap();
        repo.save(&make_knowledge("b", Some("/repo/b"), None))
            .await
            .unwrap();

        let items = repo.list(None, None, Some("/repo/a"), None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "a");
    }

    #[tokio::test]
    async fn test_list_filter_by_ink() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        repo.save(&make_knowledge("c", Some("/tmp"), Some("coder")))
            .await
            .unwrap();
        repo.save(&make_knowledge("r", Some("/tmp"), Some("reviewer")))
            .await
            .unwrap();

        let items = repo.list(None, None, None, Some("coder"), None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "c");
    }

    #[tokio::test]
    async fn test_list_filter_by_session() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("target", Some("/tmp"), None);
        let session_id = k.session_id.to_string();
        repo.save(&k).await.unwrap();
        repo.save(&make_knowledge("other", Some("/tmp"), None))
            .await
            .unwrap();

        let items = repo
            .list(Some(&session_id), None, None, None, None)
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "target");
    }

    #[tokio::test]
    async fn test_list_with_limit() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        for i in 0..5 {
            repo.save(&make_knowledge(&format!("item-{i}"), Some("/tmp"), None))
                .await
                .unwrap();
        }

        let items = repo.list(None, None, None, None, Some(2)).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn test_list_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let items = repo.list(None, None, None, None, None).unwrap();
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn test_query_context_returns_relevant() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        repo.save(&make_knowledge("global", None, None))
            .await
            .unwrap();
        repo.save(&make_knowledge("scoped", Some("/my/repo"), Some("coder")))
            .await
            .unwrap();
        repo.save(&make_knowledge("other", Some("/other/repo"), None))
            .await
            .unwrap();

        let items = repo
            .query_context(Some("/my/repo"), Some("coder"), 10)
            .unwrap();
        // Should get global + scoped, not other
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn test_query_context_with_limit() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        for i in 0..10 {
            repo.save(&make_knowledge(&format!("g-{i}"), None, None))
                .await
                .unwrap();
        }

        let items = repo.query_context(None, None, 3).unwrap();
        assert_eq!(items.len(), 3);
    }

    #[tokio::test]
    async fn test_query_context_no_workdir() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        repo.save(&make_knowledge("global", None, None))
            .await
            .unwrap();
        repo.save(&make_knowledge("scoped", Some("/my/repo"), None))
            .await
            .unwrap();

        // Without workdir, only global items match
        let items = repo.query_context(None, None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "global");
    }

    #[tokio::test]
    async fn test_query_context_ordered_by_relevance() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let mut low = make_knowledge("low", None, None);
        low.relevance = 0.2;
        repo.save(&low).await.unwrap();

        let mut high = make_knowledge("high", None, None);
        high.relevance = 0.9;
        repo.save(&high).await.unwrap();

        let items = repo.query_context(None, None, 10).unwrap();
        assert_eq!(items[0].title, "high");
        assert_eq!(items[1].title, "low");
    }

    #[tokio::test]
    async fn test_delete() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("to-delete", Some("/tmp"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let deleted = repo.delete(&id).await.unwrap();
        assert!(deleted);

        let items = repo.list(None, None, None, None, None).unwrap();
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn test_delete_not_found() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let deleted = repo.delete("nonexistent").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_delete_by_session() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let session_id = Uuid::new_v4();
        let k1 = Knowledge {
            session_id,
            ..make_knowledge("s1", Some("/tmp"), None)
        };
        let k2 = Knowledge {
            session_id,
            ..make_knowledge("s2", Some("/tmp"), None)
        };
        let k3 = make_knowledge("other-session", Some("/tmp"), None);

        repo.save(&k1).await.unwrap();
        repo.save(&k2).await.unwrap();
        repo.save(&k3).await.unwrap();

        let count = repo
            .delete_by_session(&session_id.to_string())
            .await
            .unwrap();
        assert_eq!(count, 2);

        let items = repo.list(None, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "other-session");
    }

    #[tokio::test]
    async fn test_delete_by_session_none_found() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let count = repo.delete_by_session("nonexistent").await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_save_culture_knowledge() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("global", None, None);
        repo.save(&k).await.unwrap();

        // Should be in culture/ directory
        let culture_dir = repo.root().join("culture");
        assert!(culture_dir.exists());

        let items = repo.list(None, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[tokio::test]
    async fn test_save_ink_scoped_knowledge() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("ink-only", None, Some("reviewer"));
        repo.save(&k).await.unwrap();

        // Should be in inks/reviewer/ directory
        let ink_dir = repo.root().join("inks").join("reviewer");
        assert!(ink_dir.exists());

        let items = repo.list(None, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[tokio::test]
    async fn test_save_scoped_knowledge() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("scoped", Some("/home/user/myrepo"), Some("coder"));
        repo.save(&k).await.unwrap();

        // Should be in repos/myrepo/ directory (repo takes precedence)
        let repo_dir = repo.root().join("repos").join("myrepo");
        assert!(repo_dir.exists());
    }

    #[tokio::test]
    async fn test_root_accessor() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        assert!(repo.root().exists());
        assert!(repo.root().ends_with("knowledge"));
    }

    #[tokio::test]
    async fn test_delete_creates_git_commit() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("del-commit", Some("/tmp"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();
        repo.delete(&id).await.unwrap();

        let log = run_git(repo.root(), &["log", "--oneline"]).await.unwrap();
        assert!(log.contains("delete"));
    }

    #[test]
    fn test_collect_knowledge_files_ignores_unknown_ext() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path();
        std::fs::write(dir.join("readme.txt"), "not knowledge").unwrap();

        let mut items = Vec::new();
        collect_knowledge_files(dir, &mut items).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_collect_knowledge_files_skips_invalid_md() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path();
        std::fs::write(dir.join("bad.md"), "not valid frontmatter").unwrap();

        let mut items = Vec::new();
        collect_knowledge_files(dir, &mut items).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_collect_knowledge_files_skips_invalid_json() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path();
        std::fs::write(dir.join("bad.json"), "not json at all").unwrap();

        let mut items = Vec::new();
        collect_knowledge_files(dir, &mut items).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_collect_knowledge_files_nonexistent_dir() {
        let mut items = Vec::new();
        let result = collect_knowledge_files(Path::new("/nonexistent"), &mut items);
        assert!(result.is_ok());
        assert!(items.is_empty());
    }

    #[test]
    fn test_collect_knowledge_paths_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        collect_knowledge_paths(tmpdir.path(), &mut paths).unwrap();
        assert!(paths.is_empty());
    }

    #[test]
    fn test_collect_knowledge_paths_nonexistent_dir() {
        let mut paths = Vec::new();
        let result = collect_knowledge_paths(Path::new("/nonexistent"), &mut paths);
        assert!(result.is_ok());
        assert!(paths.is_empty());
    }

    #[test]
    fn test_knowledge_repo_clone() {
        let repo = KnowledgeRepo {
            root: PathBuf::from("/tmp"),
            remote: Some("git@github.com:user/knowledge.git".into()),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = repo.clone();
        assert_eq!(cloned.root, repo.root);
        assert_eq!(cloned.remote, repo.remote);
    }

    #[test]
    fn test_knowledge_repo_debug() {
        let repo = KnowledgeRepo {
            root: PathBuf::from("/tmp"),
            remote: None,
        };
        let debug = format!("{repo:?}");
        assert!(debug.contains("KnowledgeRepo"));
    }

    #[tokio::test]
    async fn test_get_by_id() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("findme", Some("/tmp/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let found = repo.get_by_id(&id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "findme");
    }

    #[tokio::test]
    async fn test_get_by_id_not_found() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let found = repo.get_by_id("nonexistent").unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_update() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("original", Some("/tmp/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let updated = repo
            .update(&id, Some("new title"), Some("new body"), None, Some(0.9))
            .await
            .unwrap();
        assert!(updated);

        let item = repo.get_by_id(&id).unwrap().unwrap();
        assert_eq!(item.title, "new title");
        assert_eq!(item.body, "new body");
        assert!((item.relevance - 0.9).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_update_tags() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("tagged", Some("/tmp/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let tags = vec!["new-tag".to_string(), "another".to_string()];
        repo.update(&id, None, None, Some(&tags), None)
            .await
            .unwrap();

        let item = repo.get_by_id(&id).unwrap().unwrap();
        assert_eq!(item.tags, vec!["new-tag", "another"]);
    }

    #[tokio::test]
    async fn test_update_not_found() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let updated = repo
            .update("nonexistent", Some("title"), None, None, None)
            .await
            .unwrap();
        assert!(!updated);
    }

    #[tokio::test]
    async fn test_update_creates_git_commit() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_knowledge("commit-test", Some("/tmp/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();
        repo.update(&id, Some("changed"), None, None, None)
            .await
            .unwrap();

        let log = run_git(repo.root(), &["log", "--oneline"]).await.unwrap();
        assert!(log.contains("update"));
    }

    #[tokio::test]
    async fn test_push_no_remote() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let err = repo.push().await;
        assert!(err.is_err());
        assert!(
            err.unwrap_err()
                .to_string()
                .contains("no remote configured")
        );
    }

    #[tokio::test]
    async fn test_init_with_remote_no_existing_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        // Remote URL that doesn't exist — init should still succeed (pull fails gracefully)
        let repo = KnowledgeRepo::init(
            tmpdir.path().to_str().unwrap(),
            Some("https://nonexistent.example.com/repo.git".into()),
        )
        .await
        .unwrap();
        assert!(repo.root().join(".git").exists());
    }

    #[tokio::test]
    async fn test_legacy_json_backward_compat() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        // Manually write a legacy JSON file into repos/
        let legacy_dir = repo.root().join("repos").join("legacy-project");
        std::fs::create_dir_all(&legacy_dir).unwrap();

        let k = make_knowledge("legacy item", Some("/tmp/legacy-project"), None);
        let json = serde_json::to_string_pretty(&k).unwrap();
        std::fs::write(legacy_dir.join("summary-2026-01-01-abcd1234.json"), &json).unwrap();

        // read_all should pick up the legacy JSON file
        let items = repo.list(None, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "legacy item");

        // get_by_id should also work
        let found = repo.get_by_id(&k.id.to_string()).unwrap();
        assert!(found.is_some());
    }
}
