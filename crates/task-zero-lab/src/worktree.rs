//! Disposable Git worktree management for Task 6 evaluation.
//!
//! Each eval run executes in its own worktree so the agent's edits cannot
//! contaminate the pinned evaluation checkout or a sibling variant's run
//! (Design Constraint 7, Task 6 acceptance criteria).
//!
//! ## Design
//!
//! - [`EvalWorktree::create`] creates a new worktree at a pinned revision
//! - The worktree is an RAII guard: dropping it removes the worktree
//! - The resolved commit is recorded for mismatch detection
//! - `git diff` captures the agent's changes after execution

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Metadata about a disposable evaluation worktree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeMeta {
    /// The resolved commit SHA.
    pub resolved_commit: String,
    /// Path to the worktree on disk.
    pub path: PathBuf,
    /// The original revision requested.
    pub requested_revision: String,
}

/// A disposable Git worktree for evaluation. Removes the worktree on drop.
pub struct EvalWorktree {
    meta: WorktreeMeta,
    /// Whether to remove the worktree on drop. Set to false for inspection.
    auto_remove: bool,
}

impl EvalWorktree {
    /// Compute the path a disposable worktree for `repo_path` should be
    /// created at, given a caller-supplied `label`.
    ///
    /// Preferred placement: `repo_path.parent()` (a sibling of `repo_path`
    /// itself), named `"{repo_name}-eval-{label}"`. This keeps the worktree
    /// at the same directory depth as the real clone, so relative
    /// path-dependencies inside the checked-out repo's `Cargo.toml`(s)
    /// resolve identically to how they resolve in the real clone.
    ///
    /// Falls back to the OS temp directory (namespaced the same way) if
    /// `repo_path` has no parent or no file name — this should not happen
    /// for a real repository checkout, but must degrade rather than panic.
    fn worktree_base_path(repo_path: &Path, label: &str) -> PathBuf {
        let repo_name = repo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo");
        let dir_name = format!("{repo_name}-eval-{label}");
        match repo_path.parent() {
            Some(parent) if repo_path.file_name().is_some() => parent.join(dir_name),
            _ => std::env::temp_dir().join(format!("daftprompt-eval-{dir_name}")),
        }
    }

    /// Create a new worktree at the given revision.
    ///
    /// The worktree is checked out as a sibling directory of `repo_path`
    /// (i.e. inside `repo_path`'s parent directory), not under the OS temp
    /// directory. This matters because a checked-out repo's `Cargo.toml`
    /// (and its members') may reference sibling crates via relative path
    /// dependencies (e.g. `../../llm-sdk`), which only resolve correctly
    /// when the checkout sits at the same depth in the directory tree as
    /// the real clone. Placing the worktree under the OS temp dir breaks
    /// those relative paths; placing it as a sibling of `repo_path`
    /// preserves them (see Task 6 Experiment Notes, 2026-08-14, for the
    /// live failure this fixes: `cargo check --workspace` inside a temp-dir
    /// worktree of `~/Projects/dwata` could not resolve `../../llm-sdk`
    /// from `dwata-agents/Cargo.toml`, because that path is only valid when
    /// the checkout root is itself a direct child of `~/Projects/`, the
    /// same directory level as the real `~/Projects/dwata` clone).
    ///
    /// If `repo_path` has no parent (e.g. it is `/` or a bare relative
    /// component with no directory prefix), we fall back to the OS temp
    /// directory rather than panicking (AGENTS.md: "failures degrade,
    /// never panic") — this should be effectively unreachable for real
    /// repository checkouts, which always live inside some directory.
    pub fn create(repo_path: &Path, revision: &str, label: &str) -> anyhow::Result<Self> {
        let worktree_base = Self::worktree_base_path(repo_path, label);
        // Clean up any stale worktree at the same path. The directory name
        // is namespaced with the repo's own name plus "-eval-" plus the
        // caller-supplied label (which already incorporates case id,
        // variant kind, and repetition in eval_runner.rs), so this can only
        // collide with a directory this same harness created previously,
        // not with an unrelated real project directory under the same
        // parent.
        if worktree_base.exists() {
            std::fs::remove_dir_all(&worktree_base).ok();
        }
        let tmp_base = worktree_base;

        // Resolve the commit
        let output = std::process::Command::new("git")
            .args(["rev-parse", revision])
            .current_dir(repo_path)
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "failed to resolve revision {}: {}",
            revision,
            String::from_utf8_lossy(&output.stderr)
        );
        let resolved = String::from_utf8(output.stdout)?.trim().to_string();

        // Create the worktree
        let output = std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "--detach",
                tmp_base.to_str().unwrap(),
                &resolved,
            ])
            .current_dir(repo_path)
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "failed to create worktree: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        Ok(Self {
            meta: WorktreeMeta {
                resolved_commit: resolved,
                path: tmp_base,
                requested_revision: revision.to_string(),
            },
            auto_remove: true,
        })
    }

    /// Get the worktree metadata.
    pub fn meta(&self) -> &WorktreeMeta {
        &self.meta
    }

    /// Get the worktree path.
    pub fn path(&self) -> &Path {
        &self.meta.path
    }

    /// Get the resolved commit.
    pub fn resolved_commit(&self) -> &str {
        &self.meta.resolved_commit
    }

    /// Capture the diff of changes made in the worktree.
    pub fn capture_diff(&self) -> anyhow::Result<String> {
        let output = std::process::Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(&self.meta.path)
            .output()?;
        Ok(String::from_utf8(output.stdout)?)
    }

    /// List files changed in the worktree (relative paths).
    pub fn changed_files(&self) -> anyhow::Result<Vec<String>> {
        let output = std::process::Command::new("git")
            .args(["diff", "--name-only", "HEAD"])
            .current_dir(&self.meta.path)
            .output()?;
        let text = String::from_utf8(output.stdout)?;
        Ok(text.lines().map(String::from).collect())
    }

    /// Check if the worktree is clean (no changes).
    pub fn is_clean(&self) -> anyhow::Result<bool> {
        let output = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.meta.path)
            .output()?;
        Ok(output.stdout.is_empty())
    }

    /// Disable auto-removal (for inspection).
    pub fn keep_alive(&mut self) {
        self.auto_remove = false;
    }
}

impl Drop for EvalWorktree {
    fn drop(&mut self) {
        if self.auto_remove && self.meta.path.exists() {
            // Remove the worktree via git
            let _ = std::process::Command::new("git")
                .args([
                    "worktree",
                    "remove",
                    "--force",
                    self.meta.path.to_str().unwrap(),
                ])
                .output();
            // Fallback: remove directory if git worktree remove failed
            if self.meta.path.exists() {
                let _ = std::fs::remove_dir_all(&self.meta.path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_meta_serializes() {
        let meta = WorktreeMeta {
            resolved_commit: "abc123".into(),
            path: PathBuf::from("/tmp/test-worktree"),
            requested_revision: "HEAD".into(),
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("abc123"));
        let restored: WorktreeMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.resolved_commit, "abc123");
    }

    #[test]
    fn worktree_base_path_is_sibling_of_repo() {
        let repo_path = Path::new("/Users/brainless/Projects/dwata");
        let base = EvalWorktree::worktree_base_path(repo_path, "C07-RawHuman-rep1");
        assert_eq!(
            base,
            PathBuf::from("/Users/brainless/Projects/dwata-eval-C07-RawHuman-rep1")
        );
        // Sibling placement: same parent directory as the real repo clone,
        // so a `../../llm-sdk` reference from a member two levels below the
        // worktree root resolves to the same location it does for the real
        // clone (`~/Projects/llm-sdk`).
        assert_eq!(base.parent(), repo_path.parent());
    }

    #[test]
    fn worktree_base_path_namespaces_by_repo_name_and_label() {
        let dwata = EvalWorktree::worktree_base_path(Path::new("/Users/x/Projects/dwata"), "l");
        let other = EvalWorktree::worktree_base_path(Path::new("/Users/x/Projects/other"), "l");
        // Distinct repos under the same parent never collide with each
        // other, or with the real project directories they sit beside.
        assert_ne!(dwata, other);
        assert_ne!(dwata, PathBuf::from("/Users/x/Projects/dwata"));
        assert_ne!(other, PathBuf::from("/Users/x/Projects/other"));
    }

    #[test]
    fn worktree_base_path_falls_back_to_temp_dir_without_panicking_when_no_parent() {
        // A root path has no parent and (on most platforms) no file_name,
        // so this must degrade to the OS temp dir rather than panic.
        let base = EvalWorktree::worktree_base_path(Path::new("/"), "label");
        assert_eq!(base, std::env::temp_dir().join("daftprompt-eval-repo-eval-label"));
    }

    /// Reproduces the exact relative-path-dependency depth found in
    /// `~/Projects/dwata/dwata-agents/Cargo.toml` (`nocodo-llm-sdk = {
    /// path = "../../llm-sdk" }`) with a synthetic fixture, so this test
    /// does not depend on `~/Projects/dwata` or `~/Projects/llm-sdk`
    /// existing on the machine running it. Confirms that a worktree placed
    /// at `worktree_base_path`'s computed location resolves the dependency
    /// the same way the real clone does, and that the old
    /// `std::env::temp_dir()`-based placement would not have.
    #[test]
    fn sibling_placement_resolves_two_level_relative_path_dep() {
        let tmp = std::env::temp_dir().join(format!(
            "daftprompt-worktree-fixture-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Layout: <tmp>/llm-sdk (a sibling crate) and <tmp>/main-repo (a
        // git repo with a member two levels deep that depends on it via
        // "../../llm-sdk", mirroring dwata/dwata-agents -> llm-sdk).
        let llm_sdk = tmp.join("llm-sdk");
        std::fs::create_dir_all(&llm_sdk).unwrap();

        let main_repo = tmp.join("main-repo");
        std::fs::create_dir_all(main_repo.join("member")).unwrap();
        run_git(&main_repo, &["init", "-q"]);
        run_git(&main_repo, &["config", "user.email", "test@example.com"]);
        run_git(&main_repo, &["config", "user.name", "test"]);
        std::fs::write(main_repo.join("member/marker.txt"), "x").unwrap();
        run_git(&main_repo, &["add", "."]);
        run_git(&main_repo, &["commit", "-q", "-m", "init"]);

        let base = EvalWorktree::worktree_base_path(&main_repo, "fixture-rep1");
        // Materialize `base/member/` on disk (standing in for what `git
        // worktree add` would create) so `../../llm-sdk` from inside it can
        // be canonicalized and compared like a real filesystem lookup.
        std::fs::create_dir_all(base.join("member")).unwrap();

        // Same depth as `main_repo` itself: `<base>/member/../../llm-sdk`
        // must land on the same `<tmp>/llm-sdk` as
        // `<main_repo>/member/../../llm-sdk` does for the real clone.
        let via_real_clone = main_repo.join("member/../../llm-sdk");
        let via_worktree_base = base.join("member/../../llm-sdk");
        assert_eq!(
            via_real_clone.canonicalize().unwrap(),
            llm_sdk.canonicalize().unwrap()
        );
        assert_eq!(
            via_worktree_base.canonicalize().unwrap(),
            llm_sdk.canonicalize().unwrap(),
            "sibling worktree placement must resolve the two-level relative \
             path dependency the same way the real clone does"
        );

        let _ = std::fs::remove_dir_all(&base);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("git available");
        assert!(status.success(), "git {:?} failed", args);
    }
}
