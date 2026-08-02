//! Epic 014 Task 1: deterministic non-Git content identity.
//!
//! Acceptance criterion: "Non-Git fixtures use deterministic xxh3 content
//! identities and separate content identity from filesystem mtime and
//! observation time." `content_hash` below is derived only from file bytes;
//! `mtime_unix` and `observed_at_unix` are recorded alongside it purely as
//! separate informational fields and never fold into the hash itself.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct NonGitContentIdentity {
    pub path: String,
    /// xxh3 content hash, reusing `daftprompt_indexer::db::content_hash` —
    /// the same stable-hashing convention `code_files`/`document_files` use
    /// (never `std::collections::hash_map::DefaultHasher`; see AGENTS.md
    /// "Stable hashing").
    pub content_hash: String,
    pub size_bytes: u64,
    /// Filesystem modification time, informational only. Never used to
    /// derive `content_hash`.
    pub mtime_unix: Option<i64>,
    /// Wall-clock time this inspection ran, distinct from `mtime_unix`: a
    /// file can be observed many times with an unchanged mtime and an
    /// unchanged hash, or re-observed after being touched with an unchanged
    /// hash but a new mtime (touch-without-content-change).
    pub observed_at_unix: i64,
}

pub fn inspect_file(path: &Path) -> anyhow::Result<NonGitContentIdentity> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {}", path.display(), e))?;
    let content_hash = daftprompt_indexer::db::content_hash(&bytes);
    let metadata = std::fs::metadata(path)?;
    let mtime_unix = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let observed_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    Ok(NonGitContentIdentity {
        path: path.to_string_lossy().to_string(),
        content_hash,
        size_bytes: bytes.len() as u64,
        mtime_unix,
        observed_at_unix,
    })
}

/// Recursively inspect regular files under `root`.
///
/// Skips `.git` and common generated/vendor directories so pointing `--path`
/// at a repository root by mistake does not scan build artifacts. This is a
/// lab convenience filter, not a policy statement about what "counts" as a
/// fixture — callers who need every file, generated or not, should target
/// [`inspect_file`] directly on the paths they care about.
pub fn inspect_dir(root: &Path) -> anyhow::Result<Vec<NonGitContentIdentity>> {
    const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".next", "dist", "build"];

    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if SKIP_DIRS.contains(&name.as_ref()) {
                    continue;
                }
                stack.push(path);
            } else if file_type.is_file() {
                out.push(inspect_file(&path)?);
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_stable_and_mtime_independent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("guide.md");
        std::fs::write(&file, b"# Guide\n\nHello.\n").unwrap();

        let first = inspect_file(&file).unwrap();

        // Touch (change mtime) without changing content.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let f = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
        f.set_modified(std::time::SystemTime::now()).unwrap();

        let second = inspect_file(&file).unwrap();

        assert_eq!(
            first.content_hash, second.content_hash,
            "touch-only must not change content identity"
        );
        assert_eq!(first.size_bytes, second.size_bytes);
    }

    #[test]
    fn content_hash_changes_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("guide.md");

        std::fs::write(&file, b"version one").unwrap();
        let first = inspect_file(&file).unwrap();

        std::fs::write(&file, b"version two").unwrap();
        let second = inspect_file(&file).unwrap();

        assert_ne!(first.content_hash, second.content_hash);
    }

    #[test]
    fn content_restore_reproduces_identical_hash() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("guide.md");

        std::fs::write(&file, b"original content").unwrap();
        let original = inspect_file(&file).unwrap();

        std::fs::write(&file, b"temporary edit").unwrap();
        inspect_file(&file).unwrap();

        std::fs::write(&file, b"original content").unwrap();
        let restored = inspect_file(&file).unwrap();

        assert_eq!(
            original.content_hash, restored.content_hash,
            "restoring identical bytes must reproduce the identical content identity"
        );
    }
}
