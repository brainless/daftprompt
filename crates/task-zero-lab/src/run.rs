//! Epic 014 Task 1: combines Git snapshot resolution and index-coverage
//! inspection into one immutable run-identity record.
//!
//! Fields here are the Task 1 slice of Design Constraint 2's "Every run
//! records at least..." list: canonical repository path, requested/resolved
//! revision, dirty-worktree policy, index database identity/watermark, and
//! lab version. Input file/blob/content identities and detector/prompt-
//! template/model versions belong to later tasks (graph extraction, packet
//! selection, prompt rendering) and are not part of this record.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::{git_snapshot, index_coverage};

pub const LAB_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize)]
pub enum DirtyInputPolicy {
    /// Default: a dirty or non-HEAD working tree is reported but this run
    /// never treats it as belonging to `requested_rev` (Task 1 acceptance
    /// criterion: "dirty or unindexed inputs are included only by explicit
    /// fixture policy and remain visibly labelled").
    ExcludeDirty,
    /// Explicit opt-in (`--include-dirty`): the caller has decided
    /// uncommitted worktree content may be used by a later stage for this
    /// run. Task 1 itself still never substitutes current file content for
    /// `requested_rev`'s historic content; this flag only records the
    /// caller's declared intent so a later packet/prompt stage inherits an
    /// explicit, labelled decision instead of an implicit one.
    IncludeDirtyLabelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunSnapshot {
    pub lab_version: String,
    pub generated_at_unix: i64,
    pub git: git_snapshot::GitSnapshot,
    pub index: index_coverage::IndexCoverage,
    pub dirty_policy: DirtyInputPolicy,
}

pub fn build_snapshot(repo: &Path, rev: &str, include_dirty: bool) -> anyhow::Result<RunSnapshot> {
    let git = git_snapshot::resolve_snapshot(repo, rev)?;
    let index = index_coverage::inspect(repo)?;
    let dirty_policy = if include_dirty {
        DirtyInputPolicy::IncludeDirtyLabelled
    } else {
        DirtyInputPolicy::ExcludeDirty
    };
    let generated_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    Ok(RunSnapshot {
        lab_version: LAB_VERSION.to_string(),
        generated_at_unix,
        git,
        index,
        dirty_policy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn build_snapshot_combines_git_and_index_evidence() {
        let repo_dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .current_dir(repo_dir.path())
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@test.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@test.com")
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@test.com"]);
        git(&["config", "user.name", "Test"]);
        std::fs::write(repo_dir.path().join("a.txt"), "hello").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "initial"]);

        let snapshot = build_snapshot(repo_dir.path(), "HEAD", false).unwrap();

        assert!(snapshot.git.is_root_commit);
        assert!(matches!(snapshot.dirty_policy, DirtyInputPolicy::ExcludeDirty));
        assert!(!snapshot.index.db_exists, "fresh temp repo has never been indexed");

        // Round-trips through serde without panicking, since this is the
        // shape the CLI actually prints.
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"lab_version\""));

        let with_dirty = build_snapshot(repo_dir.path(), "HEAD", true).unwrap();
        assert!(matches!(
            with_dirty.dirty_policy,
            DirtyInputPolicy::IncludeDirtyLabelled
        ));
    }
}
