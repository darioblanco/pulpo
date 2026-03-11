use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use pulpo_common::culture::{Culture, CultureKind};
use tokio::process::Command;
use tracing::{debug, warn};

/// Git-backed culture repository.
///
/// Stores culture items as Markdown files with YAML frontmatter in a local
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
/// - `culture/` — global culture, not scoped to any repo or ink
#[derive(Clone, Debug)]
pub struct CultureRepo {
    root: PathBuf,
    remote: Option<String>,
}

impl CultureRepo {
    /// Initialise (or open) the culture git repo.
    pub async fn init(data_dir: &str, remote: Option<String>) -> Result<Self> {
        let root = PathBuf::from(data_dir).join("culture");
        std::fs::create_dir_all(&root)
            .with_context(|| format!("create culture dir: {}", root.display()))?;

        // git init (idempotent)
        if !root.join(".git").exists() {
            run_git(&root, &["init"]).await?;
            // Configure for automation
            run_git(&root, &["config", "user.email", "pulpo@localhost"]).await?;
            run_git(&root, &["config", "user.name", "pulpo"]).await?;
        }

        // Bootstrap: write starter AGENTS.md if it doesn't exist
        let global_dir = root.join("culture");
        std::fs::create_dir_all(&global_dir)?;
        let agents_md = global_dir.join(AGENTS_MD_FILENAME);
        if !agents_md.exists() {
            std::fs::write(&agents_md, BOOTSTRAP_TEMPLATE)?;
            // Stage and commit the bootstrap template
            run_git(&root, &["add", "culture/AGENTS.md"]).await?;
            run_git(
                &root,
                &["commit", "-m", "culture: bootstrap AGENTS.md template"],
            )
            .await?;
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
                debug!("culture pull skipped (expected on first use): {e}");
            }
        }

        Ok(Self { root, remote })
    }

    /// Persist a culture item as a Markdown file with YAML frontmatter.
    /// After saving, recompiles the AGENTS.md for the affected scope.
    pub async fn save(&self, culture: &Culture) -> Result<()> {
        let dir = self.item_dir(culture);
        std::fs::create_dir_all(&dir)?;

        let filename = item_filename(culture);
        let path = dir.join(&filename);

        let content = serialize_to_markdown(culture)?;
        std::fs::write(&path, &content)?;

        // Recompile AGENTS.md for this scope
        self.compile_scope_for(culture)?;

        // Stage both the entry and the updated AGENTS.md
        let scope_dir = scope_dir_name(culture);
        run_git(&self.root, &["add", &scope_dir]).await?;

        let msg = format!("culture: {}", culture.title);
        run_git(&self.root, &["commit", "-m", &msg]).await?;

        // Fire-and-forget push
        if self.remote.is_some() {
            let root = self.root.clone();
            tokio::spawn(async move {
                if let Err(e) = run_git(&root, &["push", "origin", "main"]).await {
                    warn!("culture push failed (will retry next commit): {e}");
                }
            });
        }

        Ok(())
    }

    /// List culture items, optionally filtered.
    pub fn list(
        &self,
        session_id: Option<&str>,
        kind: Option<&str>,
        repo: Option<&str>,
        ink: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Culture>> {
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

    /// Query culture relevant to a workdir/ink combination for context injection.
    /// Returns items scoped to the repo, the ink, or global, ordered by relevance.
    pub fn query_context(
        &self,
        workdir: Option<&str>,
        ink: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Culture>> {
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

    /// Delete a culture item by ID. Returns true if found and deleted.
    /// After deletion, recompiles the AGENTS.md for the affected scope.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let path = self.find_by_id(id)?;
        let Some(path) = path else {
            return Ok(false);
        };

        // Read the item before deleting so we know which scope to recompile
        let content = std::fs::read_to_string(&path)?;
        let item = parse_file(&path, &content);

        std::fs::remove_file(&path)?;

        // Recompile AGENTS.md for the affected scope
        if let Some(ref culture) = item {
            self.compile_scope_for(culture)?;
            let scope_dir = scope_dir_name(culture);
            run_git(&self.root, &["add", &scope_dir]).await?;
        } else {
            let rel = path
                .strip_prefix(&self.root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            run_git(&self.root, &["add", &rel]).await?;
        }

        run_git(
            &self.root,
            &["commit", "-m", &format!("culture: delete {id}")],
        )
        .await?;

        self.fire_and_forget_push();
        Ok(true)
    }

    /// Delete all culture for a session. Returns count deleted.
    /// After deletion, recompiles AGENTS.md for all affected scopes.
    pub async fn delete_by_session(&self, session_id: &str) -> Result<usize> {
        let items = self.read_all()?;
        let to_delete: Vec<_> = items
            .into_iter()
            .filter(|k| k.session_id.to_string() == session_id)
            .collect();

        if to_delete.is_empty() {
            return Ok(0);
        }

        let count = to_delete.len();
        let mut affected_scopes = std::collections::HashSet::new();

        for item in &to_delete {
            if let Some(path) = self.find_by_id(&item.id.to_string())? {
                std::fs::remove_file(&path)?;
            }
            affected_scopes.insert(scope_dir_name(item));
        }

        // Recompile AGENTS.md for all affected scopes
        for scope in &affected_scopes {
            self.compile_agents_md(scope)?;
        }

        // Stage all affected scope directories
        let mut git_args = vec!["add"];
        let scope_refs: Vec<&str> = affected_scopes.iter().map(String::as_str).collect();
        git_args.extend_from_slice(&scope_refs);
        run_git(&self.root, &git_args).await?;

        run_git(
            &self.root,
            &[
                "commit",
                "-m",
                &format!("culture: delete session {session_id}"),
            ],
        )
        .await?;

        self.fire_and_forget_push();
        Ok(count)
    }

    /// Get a single culture item by ID.
    pub fn get_by_id(&self, id: &str) -> Result<Option<Culture>> {
        let path = self.find_by_id(id)?;
        let Some(path) = path else {
            return Ok(None);
        };
        let content = std::fs::read_to_string(&path)?;
        Ok(parse_file(&path, &content))
    }

    /// Update a culture item. Only non-`None` fields are patched.
    /// After updating, recompiles the AGENTS.md for the affected scope.
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

        // Recompile AGENTS.md for this scope
        self.compile_scope_for(&item)?;

        let scope_dir = scope_dir_name(&item);
        run_git(&self.root, &["add", &scope_dir]).await?;
        run_git(
            &self.root,
            &["commit", "-m", &format!("culture: update {id}")],
        )
        .await?;

        self.fire_and_forget_push();
        Ok(true)
    }

    /// Explicitly push all local commits to the remote.
    /// Returns an error if no remote is configured or push fails.
    pub async fn push(&self) -> Result<()> {
        let Some(ref _url) = self.remote else {
            bail!("no remote configured for culture repo");
        };
        run_git(&self.root, &["push", "origin", "main"]).await?;
        Ok(())
    }

    /// The root path of the culture repo.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read the compiled AGENTS.md for a given scope directory.
    /// Returns `None` if the file doesn't exist.
    pub fn read_agents_md(&self, scope_dir: &str) -> Result<Option<String>> {
        let path = self.root.join(scope_dir).join(AGENTS_MD_FILENAME);
        if path.exists() {
            Ok(Some(std::fs::read_to_string(&path)?))
        } else {
            Ok(None)
        }
    }

    /// Compile an AGENTS.md file for a scope directory by collecting all
    /// culture entries in that directory and appending them under
    /// "## Session Learnings".
    ///
    /// For the global scope (`culture/`), the bootstrap template is preserved
    /// above the learnings section. For repo/ink scopes, a minimal header is
    /// generated.
    pub fn compile_agents_md(&self, scope_dir: &str) -> Result<()> {
        let dir = self.root.join(scope_dir);
        if !dir.exists() {
            return Ok(());
        }

        // Collect entries from this specific directory (not recursive into sub-scopes)
        let mut entries = Vec::new();
        if dir.is_dir() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file()
                    && is_culture_file(&path)
                    && let Ok(content) = std::fs::read_to_string(&path)
                    && let Some(k) = parse_file(&path, &content)
                {
                    entries.push(k);
                }
            }
        }

        // Sort by relevance DESC, then created_at DESC
        entries.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.created_at.cmp(&a.created_at))
        });

        let content = build_agents_md_content(scope_dir, &entries);
        let agents_path = dir.join(AGENTS_MD_FILENAME);
        std::fs::write(&agents_path, content)?;

        Ok(())
    }

    /// List all files and directories in the culture repo (excluding `.git`).
    ///
    /// Returns a flat list of `(relative_path, is_dir)` tuples, sorted
    /// alphabetically with directories before files at each level.
    pub fn list_files(&self) -> Result<Vec<(String, bool)>> {
        let mut entries = Vec::new();
        walk_dir(&self.root, &self.root, &mut entries)?;
        // Sort: directories first, then alphabetically
        entries.sort_by(|a, b| match (a.1, b.1) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.cmp(&b.0),
        });
        Ok(entries)
    }

    /// Read a file from the culture repo by relative path.
    ///
    /// Returns the file content, or an error if the path is outside the repo
    /// root, is a directory, or doesn't exist.
    pub fn read_file(&self, relative_path: &str) -> Result<String> {
        let requested = self.root.join(relative_path);
        let canonical = requested
            .canonicalize()
            .with_context(|| format!("file not found: {relative_path}"))?;
        let root_canonical = self
            .root
            .canonicalize()
            .with_context(|| "culture repo root not found")?;
        if !canonical.starts_with(&root_canonical) {
            bail!("path traversal not allowed: {relative_path}");
        }
        if canonical.is_dir() {
            bail!("path is a directory: {relative_path}");
        }
        std::fs::read_to_string(&canonical)
            .with_context(|| format!("failed to read: {relative_path}"))
    }

    /// Compile the AGENTS.md for the scope that a culture item belongs to.
    fn compile_scope_for(&self, culture: &Culture) -> Result<()> {
        let scope_dir = scope_dir_name(culture);
        self.compile_agents_md(&scope_dir)
    }

    // ── Private helpers ─────────────────────────────────────────────────

    /// Determine the directory for a culture item.
    ///
    /// - `scope_repo` set → `repos/<slug>/`
    /// - `scope_ink` set (no repo) → `inks/<ink>/`
    /// - neither → `culture/`
    fn item_dir(&self, k: &Culture) -> PathBuf {
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

    /// Read all culture files from the repo (`.md` and legacy `.json`).
    fn read_all(&self) -> Result<Vec<Culture>> {
        let mut items = Vec::new();
        for dir_name in &["repos", "inks", "culture"] {
            let base = self.root.join(dir_name);
            if base.exists() {
                collect_culture_files(&base, &mut items)?;
            }
        }
        Ok(items)
    }

    /// Find a culture file by its UUID.
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

    /// Collect all culture file paths (`.md` and legacy `.json`).
    fn read_all_paths(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for dir_name in &["repos", "inks", "culture"] {
            let base = self.root.join(dir_name);
            if base.exists() {
                collect_culture_paths(&base, &mut paths)?;
            }
        }
        Ok(paths)
    }

    fn fire_and_forget_push(&self) {
        if self.remote.is_some() {
            let root = self.root.clone();
            tokio::spawn(async move {
                if let Err(e) = run_git(&root, &["push", "origin", "main"]).await {
                    warn!("culture push failed: {e}");
                }
            });
        }
    }
}

// ── Markdown serialization ──────────────────────────────────────────────

/// YAML frontmatter metadata for culture files.
#[derive(serde::Serialize, serde::Deserialize)]
struct CultureFrontmatter {
    id: uuid::Uuid,
    session_id: uuid::Uuid,
    kind: CultureKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope_repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope_ink: Option<String>,
    title: String,
    tags: Vec<String>,
    relevance: f64,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Serialize a `Culture` item to Markdown with YAML frontmatter.
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
fn serialize_to_markdown(k: &Culture) -> Result<String> {
    let frontmatter = CultureFrontmatter {
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

/// Parse a Markdown file with YAML frontmatter into a `Culture` item.
fn parse_from_markdown(content: &str) -> Result<Culture> {
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

    let fm: CultureFrontmatter = serde_yaml::from_str(yaml_str)?;

    Ok(Culture {
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

/// Recursively walk a directory, collecting `(relative_path, is_dir)`.
/// Skips `.git` directories.
fn walk_dir(dir: &Path, root: &Path, out: &mut Vec<(String, bool)>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut dir_entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    dir_entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in dir_entries {
        let path = entry.path();
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        let is_dir = path.is_dir();
        out.push((rel, is_dir));
        if is_dir {
            walk_dir(&path, root, out)?;
        }
    }
    Ok(())
}

/// Parse a file as either Markdown (`.md`) or legacy JSON (`.json`).
fn parse_file(path: &Path, content: &str) -> Option<Culture> {
    if path.extension().is_some_and(|ext| ext == "md") {
        match parse_from_markdown(content) {
            Ok(k) => Some(k),
            Err(e) => {
                warn!("skip invalid culture markdown {}: {e}", path.display());
                None
            }
        }
    } else if path.extension().is_some_and(|ext| ext == "json") {
        // Legacy JSON backward compatibility
        match serde_json::from_str::<Culture>(content) {
            Ok(k) => Some(k),
            Err(e) => {
                warn!("skip invalid culture json {}: {e}", path.display());
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

/// Build the content for a compiled AGENTS.md file.
///
/// For the global scope (`culture/`), preserves the bootstrap template header.
/// For repo/ink scopes, generates a minimal header. In both cases, entries are
/// appended under a "## Session Learnings" section.
fn build_agents_md_content(scope_dir: &str, entries: &[Culture]) -> String {
    use std::fmt::Write;

    let mut out = String::new();

    if scope_dir == "culture" {
        // Global scope: use the bootstrap template (which already ends with
        // "## Session Learnings")
        out.push_str(BOOTSTRAP_TEMPLATE);
    } else {
        // Scoped: generate a minimal header
        #[allow(clippy::option_if_let_else)]
        let scope_label = if let Some(slug) = scope_dir.strip_prefix("repos/") {
            format!("Repository: {slug}")
        } else if let Some(ink) = scope_dir.strip_prefix("inks/") {
            format!("Ink: {ink}")
        } else {
            "Culture".to_owned()
        };
        let _ = write!(out, "# {scope_label}\n\n## Session Learnings\n\n");
    }

    if entries.is_empty() {
        out.push_str("<!-- No learnings yet -->\n");
    } else {
        for entry in entries {
            let kind_tag = match entry.kind {
                CultureKind::Summary => "summary",
                CultureKind::Failure => "failure",
            };
            let _ = write!(out, "### [{kind_tag}] {}\n\n", entry.title);
            if !entry.body.is_empty() {
                out.push_str(&entry.body);
                out.push('\n');
            }
            out.push('\n');
        }
    }

    out
}

/// Get the scope directory name (relative to root) for a culture item.
fn scope_dir_name(k: &Culture) -> String {
    k.scope_repo.as_ref().map_or_else(
        || {
            k.scope_ink
                .as_ref()
                .map_or_else(|| "culture".to_owned(), |ink| format!("inks/{ink}"))
        },
        |repo| format!("repos/{}", repo_slug(repo)),
    )
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

/// Generate a filename for a culture item.
fn item_filename(k: &Culture) -> String {
    let kind_str = match k.kind {
        CultureKind::Summary => "summary",
        CultureKind::Failure => "failure",
    };
    let date = k.created_at.format("%Y-%m-%d");
    let id_short = &k.id.to_string()[..8];
    format!("{kind_str}-{date}-{id_short}.md")
}

/// Recursively collect all culture files (`.md` and legacy `.json`) and parse them.
fn collect_culture_files(dir: &Path, items: &mut Vec<Culture>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_culture_files(&path, items)?;
        } else if is_culture_file(&path) {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    if let Some(k) = parse_file(&path, &content) {
                        items.push(k);
                    }
                }
                Err(e) => warn!("skip unreadable culture file {}: {e}", path.display()),
            }
        }
    }
    Ok(())
}

/// Recursively collect all culture file paths (`.md` and legacy `.json`).
fn collect_culture_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_culture_paths(&path, paths)?;
        } else if is_culture_file(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

/// The filename used for compiled AGENTS.md files in each scope directory.
const AGENTS_MD_FILENAME: &str = "AGENTS.md";

/// Check if a path is a culture entry file (`.md` or legacy `.json`),
/// excluding compiled AGENTS.md files which are generated artifacts.
fn is_culture_file(path: &Path) -> bool {
    let is_agents_md = path.file_name().is_some_and(|n| n == AGENTS_MD_FILENAME);
    !is_agents_md
        && path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "json")
}

/// Starter AGENTS.md template with community-validated sections.
/// Written to `culture/AGENTS.md` (global scope) on first init.
const BOOTSTRAP_TEMPLATE: &str = "\
# Culture

Shared learnings accumulated by Pulpo agent sessions. This file is automatically
maintained — agents contribute learnings after each session, and Pulpo validates
and merges them here.

## Commands

<!-- Build, test, lint, and dev commands discovered by agents -->

## Testing

<!-- Testing conventions, frameworks, and gotchas -->

## Architecture

<!-- Key modules, their relationships, and design decisions -->

## Code Style

<!-- Formatting, naming, and patterns to follow or avoid -->

## Git Workflow

<!-- Branching, commit conventions, and PR requirements -->

## Boundaries

<!-- Files/dirs not to modify, security considerations, constraints -->

## Session Learnings

<!-- Entries below are added automatically by Pulpo from agent sessions -->
";

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_culture(title: &str, repo: Option<&str>, ink: Option<&str>) -> Culture {
        Culture {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            kind: CultureKind::Summary,
            scope_repo: repo.map(Into::into),
            scope_ink: ink.map(Into::into),
            title: title.into(),
            body: "Test body".into(),
            tags: vec!["claude".into()],
            relevance: 0.5,
            created_at: Utc::now(),
        }
    }

    fn make_failure(title: &str, repo: Option<&str>) -> Culture {
        Culture {
            kind: CultureKind::Failure,
            relevance: 0.8,
            ..make_culture(title, repo, None)
        }
    }

    // ── Markdown serialization tests ────────────────────────────────────

    #[test]
    fn test_serialize_to_markdown() {
        let k = make_culture("test finding", Some("/tmp/repo"), Some("coder"));
        let md = serialize_to_markdown(&k).unwrap();
        assert!(md.starts_with("---\n"));
        assert!(md.contains("title: test finding"));
        assert!(md.contains("kind: summary"));
        assert!(md.contains("\n---\n"));
        assert!(md.contains("Test body"));
    }

    #[test]
    fn test_parse_from_markdown() {
        let k = make_culture("roundtrip", Some("/tmp/repo"), Some("coder"));
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
        assert_eq!(parsed.kind, CultureKind::Failure);
        assert_eq!(parsed.title, "fail roundtrip");
    }

    #[test]
    fn test_serialize_omits_none_scopes() {
        let k = make_culture("global", None, None);
        let md = serialize_to_markdown(&k).unwrap();
        assert!(!md.contains("scope_repo"));
        assert!(!md.contains("scope_ink"));
    }

    #[test]
    fn test_parse_file_md() {
        let k = make_culture("md test", Some("/tmp"), None);
        let md = serialize_to_markdown(&k).unwrap();
        let path = Path::new("test.md");
        let parsed = parse_file(path, &md);
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap().title, "md test");
    }

    #[test]
    fn test_parse_file_json_legacy() {
        let k = make_culture("json test", Some("/tmp"), None);
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
        let k = make_culture("test", Some("/tmp/repo"), None);
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
        let repo = CultureRepo {
            root: PathBuf::from("/data/culture"),
            remote: None,
        };
        let k = make_culture("test", Some("/tmp/myrepo"), None);
        let dir = repo.item_dir(&k);
        assert_eq!(dir, PathBuf::from("/data/culture/repos/myrepo"));
    }

    #[test]
    fn test_item_dir_with_scope_ink_only() {
        let repo = CultureRepo {
            root: PathBuf::from("/data/culture"),
            remote: None,
        };
        let k = make_culture("test", None, Some("reviewer"));
        let dir = repo.item_dir(&k);
        assert_eq!(dir, PathBuf::from("/data/culture/inks/reviewer"));
    }

    #[test]
    fn test_item_dir_culture() {
        let repo = CultureRepo {
            root: PathBuf::from("/data/culture"),
            remote: None,
        };
        let k = make_culture("test", None, None);
        let dir = repo.item_dir(&k);
        assert_eq!(dir, PathBuf::from("/data/culture/culture"));
    }

    #[test]
    fn test_item_dir_repo_takes_precedence_over_ink() {
        let repo = CultureRepo {
            root: PathBuf::from("/data/culture"),
            remote: None,
        };
        // When both scope_repo and scope_ink are set, repo wins
        let k = make_culture("test", Some("/tmp/myrepo"), Some("coder"));
        let dir = repo.item_dir(&k);
        assert_eq!(dir, PathBuf::from("/data/culture/repos/myrepo"));
    }

    #[test]
    fn test_is_culture_file() {
        assert!(is_culture_file(Path::new("test.md")));
        assert!(is_culture_file(Path::new("test.json")));
        assert!(!is_culture_file(Path::new("test.txt")));
        assert!(!is_culture_file(Path::new("test")));
    }

    // ── Integration tests (git-backed) ──────────────────────────────────

    #[tokio::test]
    async fn test_init_creates_git_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        assert!(repo.root().join(".git").exists());
    }

    #[tokio::test]
    async fn test_init_idempotent() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();
        CultureRepo::init(data_dir, None).await.unwrap();
        // Second init should not error
        CultureRepo::init(data_dir, None).await.unwrap();
    }

    #[tokio::test]
    async fn test_save_and_list() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("finding-1", Some("/tmp/repo"), Some("coder"));
        repo.save(&k).await.unwrap();

        let items = repo.list(None, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "finding-1");
        assert_eq!(items[0].id, k.id);
    }

    #[tokio::test]
    async fn test_save_creates_markdown_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("md-test", Some("/tmp/repo"), None);
        repo.save(&k).await.unwrap();

        // Verify the directory contains the entry file + compiled AGENTS.md
        let repo_dir = repo.root().join("repos").join("repo");
        let entries: Vec<_> = std::fs::read_dir(&repo_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 2); // entry .md + AGENTS.md

        // Find the entry file (not AGENTS.md)
        let entry_path = entries
            .iter()
            .map(std::fs::DirEntry::path)
            .find(|p| p.file_name().unwrap() != "AGENTS.md")
            .unwrap();
        assert_eq!(entry_path.extension().unwrap(), "md");

        let content = std::fs::read_to_string(&entry_path).unwrap();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("title: md-test"));
        assert!(content.contains("Test body"));
    }

    #[tokio::test]
    async fn test_save_creates_git_commit() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("committed", Some("/tmp/repo"), None);
        repo.save(&k).await.unwrap();

        // Verify git log has a commit
        let log = run_git(repo.root(), &["log", "--oneline"]).await.unwrap();
        assert!(log.contains("culture:"));
    }

    #[tokio::test]
    async fn test_list_filter_by_kind() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        repo.save(&make_culture("sum", Some("/tmp"), None))
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
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        repo.save(&make_culture("a", Some("/repo/a"), None))
            .await
            .unwrap();
        repo.save(&make_culture("b", Some("/repo/b"), None))
            .await
            .unwrap();

        let items = repo.list(None, None, Some("/repo/a"), None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "a");
    }

    #[tokio::test]
    async fn test_list_filter_by_ink() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        repo.save(&make_culture("c", Some("/tmp"), Some("coder")))
            .await
            .unwrap();
        repo.save(&make_culture("r", Some("/tmp"), Some("reviewer")))
            .await
            .unwrap();

        let items = repo.list(None, None, None, Some("coder"), None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "c");
    }

    #[tokio::test]
    async fn test_list_filter_by_session() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("target", Some("/tmp"), None);
        let session_id = k.session_id.to_string();
        repo.save(&k).await.unwrap();
        repo.save(&make_culture("other", Some("/tmp"), None))
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
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        for i in 0..5 {
            repo.save(&make_culture(&format!("item-{i}"), Some("/tmp"), None))
                .await
                .unwrap();
        }

        let items = repo.list(None, None, None, None, Some(2)).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn test_list_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let items = repo.list(None, None, None, None, None).unwrap();
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn test_query_context_returns_relevant() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        repo.save(&make_culture("global", None, None))
            .await
            .unwrap();
        repo.save(&make_culture("scoped", Some("/my/repo"), Some("coder")))
            .await
            .unwrap();
        repo.save(&make_culture("other", Some("/other/repo"), None))
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
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        for i in 0..10 {
            repo.save(&make_culture(&format!("g-{i}"), None, None))
                .await
                .unwrap();
        }

        let items = repo.query_context(None, None, 3).unwrap();
        assert_eq!(items.len(), 3);
    }

    #[tokio::test]
    async fn test_query_context_no_workdir() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        repo.save(&make_culture("global", None, None))
            .await
            .unwrap();
        repo.save(&make_culture("scoped", Some("/my/repo"), None))
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
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let mut low = make_culture("low", None, None);
        low.relevance = 0.2;
        repo.save(&low).await.unwrap();

        let mut high = make_culture("high", None, None);
        high.relevance = 0.9;
        repo.save(&high).await.unwrap();

        let items = repo.query_context(None, None, 10).unwrap();
        assert_eq!(items[0].title, "high");
        assert_eq!(items[1].title, "low");
    }

    #[tokio::test]
    async fn test_delete() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("to-delete", Some("/tmp"), None);
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
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let deleted = repo.delete("nonexistent").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_delete_by_session() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let session_id = Uuid::new_v4();
        let k1 = Culture {
            session_id,
            ..make_culture("s1", Some("/tmp"), None)
        };
        let k2 = Culture {
            session_id,
            ..make_culture("s2", Some("/tmp"), None)
        };
        let k3 = make_culture("other-session", Some("/tmp"), None);

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
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let count = repo.delete_by_session("nonexistent").await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_save_culture_culture() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("global", None, None);
        repo.save(&k).await.unwrap();

        // Should be in culture/ directory
        let culture_dir = repo.root().join("culture");
        assert!(culture_dir.exists());

        let items = repo.list(None, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[tokio::test]
    async fn test_save_ink_scoped_culture() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("ink-only", None, Some("reviewer"));
        repo.save(&k).await.unwrap();

        // Should be in inks/reviewer/ directory
        let ink_dir = repo.root().join("inks").join("reviewer");
        assert!(ink_dir.exists());

        let items = repo.list(None, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[tokio::test]
    async fn test_save_scoped_culture() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("scoped", Some("/home/user/myrepo"), Some("coder"));
        repo.save(&k).await.unwrap();

        // Should be in repos/myrepo/ directory (repo takes precedence)
        let repo_dir = repo.root().join("repos").join("myrepo");
        assert!(repo_dir.exists());
    }

    #[tokio::test]
    async fn test_root_accessor() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        assert!(repo.root().exists());
        assert!(repo.root().ends_with("culture"));
    }

    #[tokio::test]
    async fn test_delete_creates_git_commit() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("del-commit", Some("/tmp"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();
        repo.delete(&id).await.unwrap();

        let log = run_git(repo.root(), &["log", "--oneline"]).await.unwrap();
        assert!(log.contains("delete"));
    }

    #[test]
    fn test_collect_culture_files_ignores_unknown_ext() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path();
        std::fs::write(dir.join("readme.txt"), "not culture").unwrap();

        let mut items = Vec::new();
        collect_culture_files(dir, &mut items).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_collect_culture_files_skips_invalid_md() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path();
        std::fs::write(dir.join("bad.md"), "not valid frontmatter").unwrap();

        let mut items = Vec::new();
        collect_culture_files(dir, &mut items).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_collect_culture_files_skips_invalid_json() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path();
        std::fs::write(dir.join("bad.json"), "not json at all").unwrap();

        let mut items = Vec::new();
        collect_culture_files(dir, &mut items).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_collect_culture_files_nonexistent_dir() {
        let mut items = Vec::new();
        let result = collect_culture_files(Path::new("/nonexistent"), &mut items);
        assert!(result.is_ok());
        assert!(items.is_empty());
    }

    #[test]
    fn test_collect_culture_paths_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        collect_culture_paths(tmpdir.path(), &mut paths).unwrap();
        assert!(paths.is_empty());
    }

    #[test]
    fn test_collect_culture_paths_nonexistent_dir() {
        let mut paths = Vec::new();
        let result = collect_culture_paths(Path::new("/nonexistent"), &mut paths);
        assert!(result.is_ok());
        assert!(paths.is_empty());
    }

    #[test]
    fn test_culture_repo_clone() {
        let repo = CultureRepo {
            root: PathBuf::from("/tmp"),
            remote: Some("git@github.com:user/culture.git".into()),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = repo.clone();
        assert_eq!(cloned.root, repo.root);
        assert_eq!(cloned.remote, repo.remote);
    }

    #[test]
    fn test_culture_repo_debug() {
        let repo = CultureRepo {
            root: PathBuf::from("/tmp"),
            remote: None,
        };
        let debug = format!("{repo:?}");
        assert!(debug.contains("CultureRepo"));
    }

    #[tokio::test]
    async fn test_get_by_id() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("findme", Some("/tmp/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let found = repo.get_by_id(&id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "findme");
    }

    #[tokio::test]
    async fn test_get_by_id_not_found() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let found = repo.get_by_id("nonexistent").unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_update() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("original", Some("/tmp/repo"), None);
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
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("tagged", Some("/tmp/repo"), None);
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
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
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
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("commit-test", Some("/tmp/repo"), None);
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
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
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
        let repo = CultureRepo::init(
            tmpdir.path().to_str().unwrap(),
            Some("https://nonexistent.example.com/repo.git".into()),
        )
        .await
        .unwrap();
        assert!(repo.root().join(".git").exists());
    }

    // ── AGENTS.md bootstrap tests ──────────────────────────────────────

    #[tokio::test]
    async fn test_init_creates_bootstrap_agents_md() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let agents_md = repo.root().join("culture").join("AGENTS.md");
        assert!(agents_md.exists());

        let content = std::fs::read_to_string(&agents_md).unwrap();
        assert!(content.contains("# Culture"));
        assert!(content.contains("## Commands"));
        assert!(content.contains("## Testing"));
        assert!(content.contains("## Architecture"));
        assert!(content.contains("## Code Style"));
        assert!(content.contains("## Git Workflow"));
        assert!(content.contains("## Boundaries"));
        assert!(content.contains("## Session Learnings"));
    }

    #[tokio::test]
    async fn test_init_bootstrap_is_committed() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let log = run_git(repo.root(), &["log", "--oneline"]).await.unwrap();
        assert!(log.contains("bootstrap AGENTS.md"));
    }

    #[tokio::test]
    async fn test_init_does_not_overwrite_existing_agents_md() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();

        // First init creates the bootstrap
        let repo = CultureRepo::init(data_dir, None).await.unwrap();

        // Manually modify the AGENTS.md
        let agents_md = repo.root().join("culture").join("AGENTS.md");
        std::fs::write(&agents_md, "# Custom culture content\n").unwrap();

        // Second init should NOT overwrite
        CultureRepo::init(data_dir, None).await.unwrap();

        let content = std::fs::read_to_string(&agents_md).unwrap();
        assert_eq!(content, "# Custom culture content\n");
    }

    #[tokio::test]
    async fn test_agents_md_not_parsed_as_culture_entry() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        // AGENTS.md exists but should NOT appear in list
        let items = repo.list(None, None, None, None, None).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_is_culture_file_excludes_agents_md() {
        assert!(!is_culture_file(Path::new("culture/AGENTS.md")));
        assert!(!is_culture_file(Path::new(
            "/data/culture/repos/pulpo/AGENTS.md"
        )));
        // Regular .md files still match
        assert!(is_culture_file(Path::new("summary-2026-03-08-abcd1234.md")));
    }

    #[tokio::test]
    async fn test_read_agents_md_global() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let content = repo.read_agents_md("culture").unwrap();
        assert!(content.is_some());
        assert!(content.unwrap().contains("# Culture"));
    }

    #[tokio::test]
    async fn test_read_agents_md_nonexistent_scope() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let content = repo.read_agents_md("repos/nonexistent").unwrap();
        assert!(content.is_none());
    }

    // ── AGENTS.md compilation tests ─────────────────────────────────────

    #[tokio::test]
    async fn test_save_updates_agents_md() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("useful finding", None, None);
        repo.save(&k).await.unwrap();

        let content = repo.read_agents_md("culture").unwrap().unwrap();
        assert!(content.contains("# Culture"));
        assert!(content.contains("## Session Learnings"));
        assert!(content.contains("### [summary] useful finding"));
        assert!(content.contains("Test body"));
    }

    #[tokio::test]
    async fn test_save_updates_repo_scoped_agents_md() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("repo finding", Some("/tmp/myrepo"), None);
        repo.save(&k).await.unwrap();

        let content = repo.read_agents_md("repos/myrepo").unwrap().unwrap();
        assert!(content.contains("# Repository: myrepo"));
        assert!(content.contains("### [summary] repo finding"));
    }

    #[tokio::test]
    async fn test_save_updates_ink_scoped_agents_md() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("ink finding", None, Some("reviewer"));
        repo.save(&k).await.unwrap();

        let content = repo.read_agents_md("inks/reviewer").unwrap().unwrap();
        assert!(content.contains("# Ink: reviewer"));
        assert!(content.contains("### [summary] ink finding"));
    }

    #[tokio::test]
    async fn test_delete_recompiles_agents_md() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("will delete", None, None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        // Verify it's in the compiled AGENTS.md
        let content = repo.read_agents_md("culture").unwrap().unwrap();
        assert!(content.contains("will delete"));

        repo.delete(&id).await.unwrap();

        // After delete, AGENTS.md should no longer contain the entry
        let content = repo.read_agents_md("culture").unwrap().unwrap();
        assert!(!content.contains("will delete"));
        assert!(content.contains("No learnings yet"));
    }

    #[tokio::test]
    async fn test_update_recompiles_agents_md() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_culture("old title", None, None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        repo.update(&id, Some("new title"), None, None, None)
            .await
            .unwrap();

        let content = repo.read_agents_md("culture").unwrap().unwrap();
        assert!(content.contains("new title"));
        assert!(!content.contains("old title"));
    }

    #[tokio::test]
    async fn test_compile_agents_md_failure_kind() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let k = make_failure("crash bug", Some("/tmp/repo"));
        repo.save(&k).await.unwrap();

        let content = repo.read_agents_md("repos/repo").unwrap().unwrap();
        assert!(content.contains("### [failure] crash bug"));
    }

    #[tokio::test]
    async fn test_compile_agents_md_sorted_by_relevance() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let mut low = make_culture("low priority", None, None);
        low.relevance = 0.2;
        repo.save(&low).await.unwrap();

        let mut high = make_culture("high priority", None, None);
        high.relevance = 0.9;
        repo.save(&high).await.unwrap();

        let content = repo.read_agents_md("culture").unwrap().unwrap();
        let high_pos = content.find("high priority").unwrap();
        let low_pos = content.find("low priority").unwrap();
        assert!(high_pos < low_pos, "high relevance should appear first");
    }

    #[test]
    fn test_build_agents_md_content_global_empty() {
        let content = build_agents_md_content("culture", &[]);
        assert!(content.contains("# Culture"));
        assert!(content.contains("## Session Learnings"));
        assert!(content.contains("No learnings yet"));
    }

    #[test]
    fn test_build_agents_md_content_repo_scope() {
        let content = build_agents_md_content("repos/myrepo", &[]);
        assert!(content.contains("# Repository: myrepo"));
        assert!(content.contains("## Session Learnings"));
    }

    #[test]
    fn test_build_agents_md_content_ink_scope() {
        let content = build_agents_md_content("inks/coder", &[]);
        assert!(content.contains("# Ink: coder"));
    }

    #[test]
    fn test_build_agents_md_content_with_entries() {
        let entries = vec![
            make_culture("first", None, None),
            make_failure("second", None),
        ];
        let content = build_agents_md_content("culture", &entries);
        assert!(content.contains("### [summary] first"));
        assert!(content.contains("### [failure] second"));
        assert!(!content.contains("No learnings yet"));
    }

    #[test]
    fn test_scope_dir_name_global() {
        let k = make_culture("test", None, None);
        assert_eq!(scope_dir_name(&k), "culture");
    }

    #[test]
    fn test_scope_dir_name_repo() {
        let k = make_culture("test", Some("/tmp/myrepo"), None);
        assert_eq!(scope_dir_name(&k), "repos/myrepo");
    }

    #[test]
    fn test_scope_dir_name_ink() {
        let k = make_culture("test", None, Some("coder"));
        assert_eq!(scope_dir_name(&k), "inks/coder");
    }

    #[test]
    fn test_compile_agents_md_nonexistent_dir() {
        let repo = CultureRepo {
            root: PathBuf::from("/nonexistent/path"),
            remote: None,
        };
        // Should return Ok, not error
        let result = repo.compile_agents_md("repos/missing");
        assert!(result.is_ok());
    }

    #[test]
    fn test_bootstrap_template_content() {
        assert!(BOOTSTRAP_TEMPLATE.starts_with("# Culture"));
        assert!(BOOTSTRAP_TEMPLATE.contains("## Session Learnings"));
        // Should not be empty
        assert!(BOOTSTRAP_TEMPLATE.len() > 100);
    }

    #[tokio::test]
    async fn test_legacy_json_backward_compat() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        // Manually write a legacy JSON file into repos/
        let legacy_dir = repo.root().join("repos").join("legacy-project");
        std::fs::create_dir_all(&legacy_dir).unwrap();

        let k = make_culture("legacy item", Some("/tmp/legacy-project"), None);
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

    #[tokio::test]
    async fn test_list_files_initial() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let files = repo.list_files().unwrap();
        // Should contain at least the culture/ dir and culture/AGENTS.md
        let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"culture"), "should list culture dir");
        assert!(
            paths.contains(&"culture/AGENTS.md"),
            "should list culture/AGENTS.md"
        );
        // .git should be excluded
        assert!(!paths.iter().any(|p| p.contains(".git")));
    }

    #[tokio::test]
    async fn test_list_files_with_entries() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        repo.save(&make_culture("entry-1", Some("/my/repo"), None))
            .await
            .unwrap();
        let files = repo.list_files().unwrap();
        let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"repos"), "should list repos dir");
        // Should have a repos/<slug>/ dir and files in it
        assert!(
            paths
                .iter()
                .any(|p| p.starts_with("repos/") && p.ends_with("AGENTS.md")),
            "should have compiled AGENTS.md in repos scope"
        );
    }

    #[tokio::test]
    async fn test_list_files_dirs_before_files() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let files = repo.list_files().unwrap();
        // All directories should come before all files
        let first_file_idx = files.iter().position(|(_, is_dir)| !is_dir);
        let last_dir_idx = files.iter().rposition(|(_, is_dir)| *is_dir);
        if let (Some(first_file), Some(last_dir)) = (first_file_idx, last_dir_idx) {
            assert!(
                last_dir < first_file,
                "dirs should come before files in sorted order"
            );
        }
    }

    #[tokio::test]
    async fn test_read_file_existing() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let content = repo.read_file("culture/AGENTS.md").unwrap();
        assert!(content.contains("# Culture"));
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let result = repo.read_file("nonexistent.md");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file_path_traversal() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let result = repo.read_file("../../etc/passwd");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file_directory() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let result = repo.read_file("culture");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("directory"));
    }
}
