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
    /// Create a new worktree at the given revision.
    ///
    /// The worktree is created under a temporary directory and checked out
    /// to the specified revision. The parent repo is at `repo_path`.
    pub fn create(repo_path: &Path, revision: &str, label: &str) -> anyhow::Result<Self> {
        let tmp_base = std::env::temp_dir().join(format!("daftprompt-eval-{label}"));
        // Clean up any stale worktree at the same path
        if tmp_base.exists() {
            std::fs::remove_dir_all(&tmp_base).ok();
        }

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
}
