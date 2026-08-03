//! Epic 014 Task 1: read-only Git revision/snapshot resolution.
//!
//! Resolves a requested revision to its commit/tree identity, records its
//! parents, and computes honest changed-file evidence without checking the
//! revision out (Task 1 acceptance criterion: "Snapshot inspection performs
//! no mutation, checkout, network call, or dependency installation"). Uses
//! `gix` (the same local `~/Projects/gitoxide/gix` source the rest of the
//! workspace path-depends on) directly, following the patterns already used
//! by `src/git_log.rs` and `crates/daftprompt-indexer/src/code.rs`.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub kind: ChangeKind,
    pub blob_id: Option<String>,
    pub previous_blob_id: Option<String>,
    /// Populated only for `Renamed`/`Copied`: `true` when gix matched the
    /// source and destination by identical content (100% similarity, no
    /// diff needed), `false` when it matched below 100% but still above the
    /// configured similarity threshold. `None` for `Added`/`Deleted`/
    /// `Modified`. Below-threshold matches never reach this variant at all —
    /// they degrade to a plain `Deleted` + `Added` pair, which is `gix`'s
    /// (and this lab's) behavior for "uncertain rename detection degrades to
    /// delete/add".
    pub exact_content_match: Option<bool>,
}

/// What `changed_files` is diffed against.
///
/// A merge commit's changed-file set is inherently ambiguous — there is one
/// diff per parent. This lab records every parent SHA in
/// [`GitSnapshot::parents`] but computes `changed_files` against the first
/// parent only, matching `git log`/`git diff`'s own first-parent default.
/// This is a deliberate, disclosed choice (see manifest.md §6: "parent-
/// relative change sets, first-parent default view"), not an implied claim
/// that the other parent's view doesn't exist.
#[derive(Debug, Clone, Serialize)]
pub enum ChangesBase {
    /// Root commit: diffed against the empty tree.
    EmptyTree,
    /// Ordinary or merge commit: diffed against this parent commit.
    FirstParent(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct GitSnapshot {
    /// Canonicalized repository path (not necessarily the requested one), so
    /// a relative or symlinked `--repo` argument does not silently produce
    /// two identities for the same repository.
    pub repo_path: String,
    pub requested_rev: String,
    pub resolved_commit: String,
    pub short_commit: String,
    pub tree_id: String,
    /// All parent commit SHAs, in Git's own order (first parent first). Zero
    /// entries for a root commit, two or more for a merge commit.
    pub parents: Vec<String>,
    pub is_root_commit: bool,
    pub is_merge_commit: bool,
    pub changes_base: ChangesBase,
    pub changed_files: Vec<ChangedFile>,
    /// Whether `resolved_commit` is the repository's current `HEAD`.
    pub resolved_is_head: bool,
    /// Raw `Repository::is_dirty()` result: whether the *current* working
    /// tree/index differs from *HEAD*. Always relative to HEAD, independent
    /// of `requested_rev`.
    pub head_is_dirty: bool,
    /// The signal Design Constraint 2 actually needs: `true` if the on-disk
    /// working tree cannot be assumed to hold `requested_rev`'s content.
    /// Computed as `head_is_dirty OR !resolved_is_head`. This is
    /// deliberately conservative: this tool never checks a historic
    /// revision out, so whenever `resolved_commit != HEAD` the on-disk
    /// files are, by construction, not that revision's content — even if
    /// some individual file happens to be byte-identical by coincidence. A
    /// prompt/packet built downstream must not read current file content as
    /// if it were `requested_rev`'s content when this is `true`.
    pub worktree_differs_from_revision: bool,
}

pub fn resolve_snapshot(repo_path: &Path, rev: &str) -> anyhow::Result<GitSnapshot> {
    let repo = gix::discover(repo_path)
        .map_err(|e| anyhow::anyhow!("failed to discover Git repository at {}: {}", repo_path.display(), e))?;

    let canonical_repo_path = std::fs::canonicalize(repo_path)
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let resolved_id = repo
        .rev_parse_single(rev)
        .map_err(|e| anyhow::anyhow!("failed to resolve revision '{}': {}", rev, e))?;
    let commit = resolved_id
        .object()?
        .try_into_commit()
        .map_err(|e| anyhow::anyhow!("resolved revision '{}' is not a commit: {}", rev, e))?;

    let resolved_commit = commit.id().to_string();
    let short_commit = commit.id().shorten_or_id().to_string();
    let tree_id = commit.tree_id()?.to_string();

    let parent_ids: Vec<gix::hash::ObjectId> = commit.parent_ids().map(|id| id.detach()).collect();
    let parents: Vec<String> = parent_ids.iter().map(|id| id.to_string()).collect();
    let is_root_commit = parents.is_empty();
    let is_merge_commit = parents.len() > 1;

    let resolved_is_head = repo
        .head_commit()
        .ok()
        .map(|h| h.id() == commit.id())
        .unwrap_or(false);

    // `is_dirty()` requires the `status` feature (enabled on this crate's
    // own `gix` dependency only, see Cargo.toml). Fail safe on error: assume
    // dirty rather than silently reporting a clean tree we could not verify.
    let head_is_dirty = repo.is_dirty().unwrap_or(true);

    let worktree_differs_from_revision = head_is_dirty || !resolved_is_head;

    let (changes_base, changed_files) = diff_against_first_parent(&repo, &commit, parent_ids.first().copied())?;

    Ok(GitSnapshot {
        repo_path: canonical_repo_path,
        requested_rev: rev.to_string(),
        resolved_commit,
        short_commit,
        tree_id,
        parents,
        is_root_commit,
        is_merge_commit,
        changes_base,
        changed_files,
        resolved_is_head,
        head_is_dirty,
        worktree_differs_from_revision,
    })
}

/// Epic 014 Task 2: read a file's content straight from the Git object store
/// at a resolved revision, never from the working tree. This is how graph
/// extraction (`graph_build.rs`) reads epic/instruction Markdown so an
/// extraction over a historic revision never silently substitutes current
/// working-tree text (Design Constraint 2). Returns `Ok(None)` when the path
/// does not exist in the resolved tree, so callers can distinguish "commit
/// has no such file" (a normal, expected case for e.g. a newly added epic)
/// from a hard error.
pub fn read_blob_at_revision(repo_path: &Path, resolved_commit: &str, rel_path: &str) -> anyhow::Result<Option<(String, Vec<u8>)>> {
    let repo = gix::discover(repo_path)
        .map_err(|e| anyhow::anyhow!("failed to discover Git repository at {}: {}", repo_path.display(), e))?;
    let resolved_id = repo
        .rev_parse_single(resolved_commit)
        .map_err(|e| anyhow::anyhow!("failed to resolve revision '{}': {}", resolved_commit, e))?;
    let commit = resolved_id
        .object()?
        .try_into_commit()
        .map_err(|e| anyhow::anyhow!("resolved revision '{}' is not a commit: {}", resolved_commit, e))?;
    let tree = commit.tree()?;

    let tree_id_for_errors = commit.tree_id()?;
    let Some(entry) = tree
        .lookup_entry_by_path(rel_path)
        .map_err(|e| anyhow::anyhow!("failed to look up '{}' in tree {}: {}", rel_path, tree_id_for_errors, e))?
    else {
        return Ok(None);
    };
    if !entry.mode().is_blob() {
        return Ok(None);
    }
    let blob_id = entry.id().to_string();
    let blob = entry
        .object()
        .map_err(|e| anyhow::anyhow!("failed to read blob for '{}' ({}): {}", rel_path, blob_id, e))?
        .try_into_blob()
        .map_err(|e| anyhow::anyhow!("object for '{}' ({}) is not a blob: {}", rel_path, blob_id, e))?;
    Ok(Some((blob_id, blob.data.clone())))
}

fn diff_against_first_parent(
    repo: &gix::Repository,
    commit: &gix::Commit<'_>,
    first_parent_id: Option<gix::hash::ObjectId>,
) -> anyhow::Result<(ChangesBase, Vec<ChangedFile>)> {
    let new_tree = commit.tree()?;

    let (base, old_tree_owned) = match first_parent_id {
        None => (ChangesBase::EmptyTree, None),
        Some(parent_id) => {
            let parent_commit = repo.find_object(parent_id)?.try_into_commit()?;
            let parent_tree = parent_commit.tree()?;
            (ChangesBase::FirstParent(parent_id.to_string()), Some(parent_tree))
        }
    };

    // Explicitly configure rewrite (rename/copy) tracking at Git's own
    // default 50% similarity threshold rather than relying on the local
    // machine's `diff.renames` config, so results are deterministic across
    // environments (Task 1 acceptance criterion: fixtures must be testable
    // without depending on ambient configuration).
    let opts = gix::diff::Options::default().with_rewrites(Some(gix::diff::Rewrites::default()));

    let changes = repo.diff_tree_to_tree(old_tree_owned.as_ref(), Some(&new_tree), opts)?;

    // `gix_diff::tree_with_rewrites` reports tree (directory) entries as well
    // as blob entries — the `relation` field lets a caller reconstruct
    // top-level directory structure, but a directory is not a "file" and
    // reporting it alongside its own changed children would double-report
    // the same content and misrepresent directories as evidence items.
    // Keep only blob/symlink entries, matching the `entry.mode.is_blob()`
    // filter `code.rs`'s tree traversal already applies for the same reason.
    let changed_files = changes
        .into_iter()
        .filter(|c| !change_entry_mode_is_tree(c))
        .map(convert_change)
        .collect();

    Ok((base, changed_files))
}

fn change_entry_mode_is_tree(change: &gix::object::tree::diff::ChangeDetached) -> bool {
    use gix::object::tree::diff::ChangeDetached as Change;
    match change {
        Change::Addition { entry_mode, .. } => entry_mode.is_tree(),
        Change::Deletion { entry_mode, .. } => entry_mode.is_tree(),
        Change::Modification { entry_mode, .. } => entry_mode.is_tree(),
        Change::Rewrite { entry_mode, .. } => entry_mode.is_tree(),
    }
}

fn convert_change(change: gix::object::tree::diff::ChangeDetached) -> ChangedFile {
    use gix::object::tree::diff::ChangeDetached as Change;
    match change {
        Change::Addition { location, id, .. } => ChangedFile {
            path: location.to_string(),
            previous_path: None,
            kind: ChangeKind::Added,
            blob_id: Some(id.to_string()),
            previous_blob_id: None,
            exact_content_match: None,
        },
        Change::Deletion { location, id, .. } => ChangedFile {
            path: location.to_string(),
            previous_path: None,
            kind: ChangeKind::Deleted,
            blob_id: None,
            previous_blob_id: Some(id.to_string()),
            exact_content_match: None,
        },
        Change::Modification {
            location,
            previous_id,
            id,
            ..
        } => ChangedFile {
            path: location.to_string(),
            previous_path: None,
            kind: ChangeKind::Modified,
            blob_id: Some(id.to_string()),
            previous_blob_id: Some(previous_id.to_string()),
            exact_content_match: None,
        },
        Change::Rewrite {
            source_location,
            source_id,
            location,
            id,
            diff,
            copy,
            ..
        } => ChangedFile {
            path: location.to_string(),
            previous_path: Some(source_location.to_string()),
            kind: if copy { ChangeKind::Copied } else { ChangeKind::Renamed },
            blob_id: Some(id.to_string()),
            previous_blob_id: Some(source_id.to_string()),
            exact_content_match: Some(diff.is_none()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .args(args)
            .status()
            .expect("failed to run git");
        assert!(status.success(), "git {:?} failed", args);
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "test@test.com"]);
        git(dir.path(), &["config", "user.name", "Test"]);
        dir
    }

    // ── Synthetic commit-mechanics fixtures ─────────────────────────────
    // Self-contained (no dependency on external clones under ~/Projects),
    // portable, and cover the same five commit-mechanics categories the
    // manifest documents against real repositories (§6): root, merge,
    // rename, deletion, ordinary modification.

    #[test]
    fn root_commit_diffs_against_empty_tree() {
        let repo = init_repo();
        std::fs::write(repo.path().join("LICENSE"), "MIT").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "root"]);

        let snap = resolve_snapshot(repo.path(), "HEAD").unwrap();

        assert!(snap.is_root_commit);
        assert!(!snap.is_merge_commit);
        assert!(snap.parents.is_empty());
        assert!(matches!(snap.changes_base, ChangesBase::EmptyTree));
        assert_eq!(snap.changed_files.len(), 1);
        assert_eq!(snap.changed_files[0].path, "LICENSE");
        assert_eq!(snap.changed_files[0].kind, ChangeKind::Added);
    }

    #[test]
    fn ordinary_modification_reports_previous_and_new_blob() {
        let repo = init_repo();
        std::fs::write(repo.path().join("stat.rs"), "fn stat() {}\n").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "initial"]);

        std::fs::write(repo.path().join("stat.rs"), "fn stat() { /* updated */ }\n").unwrap();
        git(repo.path(), &["commit", "-aq", "-m", "modify"]);

        let snap = resolve_snapshot(repo.path(), "HEAD").unwrap();

        assert!(!snap.is_root_commit);
        assert!(!snap.is_merge_commit);
        assert_eq!(snap.changed_files.len(), 1);
        let change = &snap.changed_files[0];
        assert_eq!(change.path, "stat.rs");
        assert_eq!(change.kind, ChangeKind::Modified);
        assert!(change.blob_id.is_some());
        assert!(change.previous_blob_id.is_some());
        assert_ne!(change.blob_id, change.previous_blob_id);
    }

    #[test]
    fn deletion_closes_current_version_without_blob_id() {
        let repo = init_repo();
        std::fs::write(repo.path().join("CLAUDE.md"), "notes").unwrap();
        std::fs::write(repo.path().join("keep.md"), "keep").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "initial"]);

        std::fs::remove_file(repo.path().join("CLAUDE.md")).unwrap();
        git(repo.path(), &["commit", "-aq", "-m", "Removed CLAUDE.md"]);

        let snap = resolve_snapshot(repo.path(), "HEAD").unwrap();

        assert_eq!(snap.changed_files.len(), 1);
        let change = &snap.changed_files[0];
        assert_eq!(change.path, "CLAUDE.md");
        assert_eq!(change.kind, ChangeKind::Deleted);
        assert!(change.blob_id.is_none());
        assert!(change.previous_blob_id.is_some());
    }

    #[test]
    fn rename_above_threshold_reports_renamed_with_previous_path() {
        let repo = init_repo();
        let original = "export function DBDeveloperPage() {\n  return (\n    <div className=\"page\">\n      <h1>Database Developer</h1>\n      <p>Manage schema, tables, and migrations here.</p>\n      <p>This page intentionally has enough content that a small rename-time edit keeps well above the 50% similarity threshold.</p>\n    </div>\n  );\n}\n";
        std::fs::write(repo.path().join("DBDeveloperPage.tsx"), original).unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "initial"]);

        git(
            repo.path(),
            &["mv", "DBDeveloperPage.tsx", "DatabasePage.tsx"],
        );
        // Small content tweak, matching the real nocodo fixture where the
        // rename commit also edits a couple of lines (99% similarity, not
        // 100%) — the point is to exercise the "renamed but not byte
        // identical" branch, not just a pure `git mv`.
        let renamed = original
            .replace("DBDeveloperPage", "DatabasePage")
            .replace("Database Developer", "Database");
        std::fs::write(repo.path().join("DatabasePage.tsx"), renamed).unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "rename"]);

        let snap = resolve_snapshot(repo.path(), "HEAD").unwrap();

        assert_eq!(snap.changed_files.len(), 1);
        let change = &snap.changed_files[0];
        assert_eq!(change.path, "DatabasePage.tsx");
        assert_eq!(change.previous_path.as_deref(), Some("DBDeveloperPage.tsx"));
        assert_eq!(change.kind, ChangeKind::Renamed);
    }

    #[test]
    fn uncertain_rename_degrades_to_delete_and_add() {
        let repo = init_repo();
        std::fs::write(repo.path().join("a.txt"), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "initial"]);

        // Below the 50% similarity threshold: delete the old file, add a
        // completely unrelated new one. This must NOT be reported as a
        // rename (Task 1 acceptance criterion: "uncertain rename detection
        // degrades to delete/add").
        std::fs::remove_file(repo.path().join("a.txt")).unwrap();
        std::fs::write(repo.path().join("b.txt"), "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").unwrap();
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "-m", "unrelated add/delete"]);

        let snap = resolve_snapshot(repo.path(), "HEAD").unwrap();

        assert_eq!(snap.changed_files.len(), 2);
        assert!(snap
            .changed_files
            .iter()
            .all(|c| c.kind == ChangeKind::Deleted || c.kind == ChangeKind::Added));
        assert!(!snap.changed_files.iter().any(|c| c.kind == ChangeKind::Renamed));
    }

    #[test]
    fn merge_commit_records_all_parents_and_diffs_against_first() {
        let repo = init_repo();
        std::fs::write(repo.path().join("base.txt"), "base").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "base"]);

        git(repo.path(), &["checkout", "-qb", "feature"]);
        std::fs::write(repo.path().join("feature.txt"), "feature").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "feature work"]);

        git(repo.path(), &["checkout", "-q", "-"]);
        std::fs::write(repo.path().join("main.txt"), "main").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "main work"]);

        git(
            repo.path(),
            &["merge", "-q", "--no-ff", "-m", "merge feature", "feature"],
        );

        let snap = resolve_snapshot(repo.path(), "HEAD").unwrap();

        assert!(snap.is_merge_commit);
        assert_eq!(snap.parents.len(), 2);
        // First parent is whichever branch HEAD pointed at before the
        // merge (the "main" line here); its diff should show only the
        // feature-branch content newly merged in.
        assert!(matches!(snap.changes_base, ChangesBase::FirstParent(_)));
        assert!(snap.changed_files.iter().any(|c| c.path == "feature.txt"));
        assert!(!snap.changed_files.iter().any(|c| c.path == "main.txt"));
    }

    #[test]
    fn historic_revision_is_flagged_as_not_represented_by_worktree() {
        let repo = init_repo();
        std::fs::write(repo.path().join("a.txt"), "one").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "first"]);
        let first_sha = String::from_utf8(
            Command::new("git")
                .current_dir(repo.path())
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();

        std::fs::write(repo.path().join("b.txt"), "two").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "second"]);

        // Clean worktree, but the requested revision is not HEAD.
        let snap = resolve_snapshot(repo.path(), &first_sha).unwrap();

        assert!(!snap.resolved_is_head);
        assert!(!snap.head_is_dirty, "worktree should be clean at HEAD");
        assert!(
            snap.worktree_differs_from_revision,
            "worktree must never be silently treated as historic revision content"
        );
    }

    #[test]
    fn dirty_worktree_at_head_is_flagged() {
        let repo = init_repo();
        std::fs::write(repo.path().join("a.txt"), "one").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "first"]);

        // Uncommitted modification.
        std::fs::write(repo.path().join("a.txt"), "one modified, uncommitted").unwrap();

        let snap = resolve_snapshot(repo.path(), "HEAD").unwrap();

        assert!(snap.resolved_is_head);
        assert!(snap.head_is_dirty);
        assert!(snap.worktree_differs_from_revision);
    }

    #[test]
    fn read_blob_at_revision_reads_committed_content_not_worktree() {
        let repo = init_repo();
        std::fs::write(repo.path().join("guide.md"), "version one\n").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "first"]);
        let first_sha = String::from_utf8(
            Command::new("git")
                .current_dir(repo.path())
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();

        // Change on disk without committing.
        std::fs::write(repo.path().join("guide.md"), "version two, uncommitted\n").unwrap();

        let (blob_id, data) = read_blob_at_revision(repo.path(), &first_sha, "guide.md").unwrap().unwrap();
        assert_eq!(String::from_utf8(data).unwrap(), "version one\n");
        assert!(!blob_id.is_empty());

        let missing = read_blob_at_revision(repo.path(), &first_sha, "does-not-exist.md").unwrap();
        assert!(missing.is_none());
    }

    // ── Real-history manifest fixtures (epics/research/task-zero-lab/manifest.md §6) ──
    //
    // These exercise the exact concrete commits the manifest and Task 1's
    // acceptance criteria call out, against the local clones under
    // `~/Projects/`. They degrade to a skipped no-op (not a failure) when a
    // clone is not present, so `cargo test --workspace` stays portable and
    // credential/network-free, per Task 1's "tests use local fixtures and
    // pass without credentials" criterion.

    fn projects_repo(name: &str) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let path = home.join("Projects").join(name);
        if path.join(".git").exists() {
            Some(path)
        } else {
            None
        }
    }

    #[test]
    fn manifest_akar_root_commit_311e7d6() {
        let Some(repo) = projects_repo("akar") else {
            eprintln!("skip manifest_akar_root_commit_311e7d6: ~/Projects/akar not present");
            return;
        };
        let snap = resolve_snapshot(&repo, "311e7d6").unwrap();
        assert!(snap.is_root_commit, "311e7d6 must be akar's root commit");
        assert!(matches!(snap.changes_base, ChangesBase::EmptyTree));
        assert!(snap
            .changed_files
            .iter()
            .any(|c| c.path == "LICENSE" && c.kind == ChangeKind::Added));
    }

    #[test]
    fn manifest_dwata_merge_commit_f14ac5c() {
        let Some(repo) = projects_repo("dwata") else {
            eprintln!("skip manifest_dwata_merge_commit_f14ac5c: ~/Projects/dwata not present");
            return;
        };
        let snap = resolve_snapshot(&repo, "f14ac5c").unwrap();
        assert!(snap.is_merge_commit);
        assert_eq!(snap.parents.len(), 2);
        assert!(snap
            .parents
            .iter()
            .any(|p| p.starts_with("90f8e7f9")));
        assert!(snap.parents.iter().any(|p| p.starts_with("070cbe83")));
    }

    #[test]
    fn manifest_nocodo_rename_commit_18bb8b4() {
        let Some(repo) = projects_repo("nocodo") else {
            eprintln!("skip manifest_nocodo_rename_commit_18bb8b4: ~/Projects/nocodo not present");
            return;
        };
        let snap = resolve_snapshot(&repo, "18bb8b4").unwrap();
        let rename = snap
            .changed_files
            .iter()
            .find(|c| c.path == "admin-gui/src/pages/DatabasePage.tsx")
            .expect("expected DatabasePage.tsx rename in changed_files");
        assert_eq!(rename.kind, ChangeKind::Renamed);
        assert_eq!(
            rename.previous_path.as_deref(),
            Some("admin-gui/src/pages/DBDeveloperPage.tsx")
        );
    }

    #[test]
    fn manifest_akar_deletion_commit_3e17af8() {
        let Some(repo) = projects_repo("akar") else {
            eprintln!("skip manifest_akar_deletion_commit_3e17af8: ~/Projects/akar not present");
            return;
        };
        let snap = resolve_snapshot(&repo, "3e17af8").unwrap();
        let deletion = snap
            .changed_files
            .iter()
            .find(|c| c.path == "CLAUDE.md")
            .expect("expected CLAUDE.md deletion in changed_files");
        assert_eq!(deletion.kind, ChangeKind::Deleted);
        assert!(deletion.blob_id.is_none());
        assert!(deletion.previous_blob_id.is_some());
    }

    #[test]
    fn manifest_akar_modification_commit_13d7692() {
        let Some(repo) = projects_repo("akar") else {
            eprintln!("skip manifest_akar_modification_commit_13d7692: ~/Projects/akar not present");
            return;
        };
        let snap = resolve_snapshot(&repo, "13d7692").unwrap();
        let paths: Vec<&str> = snap.changed_files.iter().map(|c| c.path.as_str()).collect();
        assert!(paths.contains(&"crates/akar-components/src/stat.rs"));
        assert!(paths.contains(&"examples/demo-rust/src/main.rs"));
        assert!(snap
            .changed_files
            .iter()
            .all(|c| c.kind == ChangeKind::Modified));
    }
}
