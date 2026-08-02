//! Epic 014 Task 1: read-only index-coverage inspection.
//!
//! Opens the existing per-repository `daftprompt-indexer` SQLite database
//! (if any) strictly read-only, and reuses its canonical schema, `repo_meta`
//! keys, and `(source_type, identifier)` conventions rather than
//! re-deriving them (`crates/daftprompt-indexer/src/db.rs`). This module
//! never creates, migrates, or writes to the database: `db::init_schema` and
//! `Indexer::new` are deliberately not called here (Task 1 acceptance
//! criterion: "Snapshot inspection performs no mutation, checkout, network
//! call, or dependency installation").

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

/// Languages the shared code indexer currently dispatches
/// (`crates/daftprompt-indexer/src/code.rs`, AGENTS.md "Code indexer
/// invariants"). Duplicated here as a small, static, human-readable list
/// rather than depending on `code.rs`'s internal extractor construction; if
/// the supported set changes, this list needs a matching update.
pub const SUPPORTED_CODE_LANGUAGES: &[&str] = &["rust", "typescript", "tsx", "javascript", "jsx"];

#[derive(Debug, Clone, Serialize)]
pub struct PartitionCoverage {
    pub source_type: String,
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexCoverage {
    pub db_path: String,
    pub db_exists: bool,
    /// `repo_meta.repo_path` as recorded by the indexer at last index time.
    pub recorded_repo_path: Option<String>,
    /// `repo_meta.indexed_at`, a wall-clock Unix-seconds string set by
    /// `Indexer::index_commits` after each successful commit-index pass.
    pub indexed_at: Option<String>,
    pub embedding_dimension: Option<usize>,
    pub partitions: Vec<PartitionCoverage>,
    pub supported_languages: Vec<&'static str>,
    /// `true` when `recorded_repo_path` does not match the canonicalized
    /// path this run was pointed at (e.g. the DB was built from a symlinked
    /// or differently-cased path). Only meaningful when `db_exists`.
    pub repo_path_mismatch: bool,
    /// Explicit, honest statement of what this report can and cannot
    /// establish about freshness. The current `repo_meta` schema records
    /// only a wall-clock `indexed_at` timestamp, not the Git commit the
    /// index was built from, so this report cannot state whether the index
    /// reflects any particular resolved revision. That is a known schema
    /// gap surfaced here rather than silently assumed away (Design
    /// Constraint 2: a prompt must not combine current index state with
    /// historic Git evidence without disclosing the mismatch risk).
    pub staleness_note: String,
}

pub fn inspect(repo_path: &Path) -> anyhow::Result<IndexCoverage> {
    let canonical_repo_path =
        std::fs::canonicalize(repo_path).unwrap_or_else(|_| repo_path.to_path_buf());
    let db_path = db_path_for_repo_readonly(&canonical_repo_path)?;

    if !db_path.exists() {
        return Ok(IndexCoverage {
            db_path: db_path.to_string_lossy().to_string(),
            db_exists: false,
            recorded_repo_path: None,
            indexed_at: None,
            embedding_dimension: None,
            partitions: Vec::new(),
            supported_languages: SUPPORTED_CODE_LANGUAGES.to_vec(),
            repo_path_mismatch: false,
            staleness_note: "no index database found at the expected path; this repository has not been indexed yet".to_string(),
        });
    }

    // sqlite-vec's virtual table module must be registered before opening
    // any connection to a database that defines `vec0` tables, even a
    // read-only connection that never queries them directly, because SQLite
    // resolves virtual table modules while loading the schema. Mirrors
    // `Indexer::new`'s `REGISTER_VEC` call (`lib.rs`), but this lab has no
    // long-lived `Indexer` to guard it with a `Once`; `sqlite3_auto_extension`
    // is documented as safe to call repeatedly with the same function
    // pointer, so calling it unconditionally here is fine.
    daftprompt_indexer::db::register_sqlite_vec();

    let db = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| anyhow::anyhow!("failed to open index database read-only at {}: {}", db_path.display(), e))?;

    let recorded_repo_path = daftprompt_indexer::db::repo_meta_get(&db, "repo_path")?;
    let indexed_at = daftprompt_indexer::db::repo_meta_get(&db, "indexed_at")?;
    let embedding_dimension = daftprompt_indexer::db::repo_meta_get(&db, "embedding_dimension")?
        .and_then(|v| v.parse::<usize>().ok());

    let mut partitions = Vec::new();
    for source_type in ["commit", "code", "document"] {
        let count = daftprompt_indexer::db::existing_identifiers(&db, source_type)?.len() as i64;
        partitions.push(PartitionCoverage {
            source_type: source_type.to_string(),
            item_count: count,
        });
    }

    let repo_path_mismatch = recorded_repo_path
        .as_deref()
        .map(|recorded| recorded != canonical_repo_path.to_string_lossy())
        .unwrap_or(false);

    Ok(IndexCoverage {
        db_path: db_path.to_string_lossy().to_string(),
        db_exists: true,
        recorded_repo_path,
        indexed_at,
        embedding_dimension,
        partitions,
        supported_languages: SUPPORTED_CODE_LANGUAGES.to_vec(),
        repo_path_mismatch,
        staleness_note: "repo_meta records only a wall-clock indexed_at timestamp, not the Git commit the index was built from; this report cannot state whether the index reflects the resolved revision requested by this run".to_string(),
    })
}

/// Mirrors `daftprompt_indexer::db::db_path_for_repo`'s slug algorithm
/// without its `create_dir_all` side effect, so a read-only coverage check
/// never creates the cache directory as a side effect of merely being run
/// (Task 1 acceptance criterion: no mutation). `Indexer::new`'s
/// custom-cache-dir branch already duplicates this same slug formula for an
/// analogous reason (`crates/daftprompt-indexer/src/lib.rs`, the
/// `config.cache_dir.is_some()` branch); if that algorithm ever changes,
/// this copy and that one both need updating together.
fn db_path_for_repo_readonly(canonical_repo_path: &Path) -> anyhow::Result<PathBuf> {
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine cache directory"))?
        .join("daftprompt");
    let abs_str = canonical_repo_path.to_string_lossy();
    let stem: String = abs_str
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let mut h: u64 = 0;
    for b in abs_str.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u64);
    }
    let hash = format!("{:06x}", h & 0xFFFFFF);
    let slug = format!("{}_{}", stem, hash);
    Ok(cache_dir.join(format!("{}.db", slug)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_index_is_reported_without_creating_anything() {
        let repo_dir = tempfile::tempdir().unwrap();
        // A directory that plausibly could not have been indexed before.
        std::fs::create_dir_all(repo_dir.path().join("src")).unwrap();

        let coverage = inspect(repo_dir.path()).unwrap();

        assert!(!coverage.db_exists);
        assert!(coverage.partitions.is_empty());
        assert!(!std::path::Path::new(&coverage.db_path).exists());
        assert_eq!(coverage.supported_languages, SUPPORTED_CODE_LANGUAGES.to_vec());
    }

    #[test]
    fn existing_index_reports_partitions_and_metadata_read_only() {
        // Build a real per-repo DB via the production indexer crate, exactly
        // as `cargo run -- --repo . --index-git-log` would, then verify this
        // module reads it back without mutating it.
        let repo_dir = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();

        let status = std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap();
        assert!(status.success());
        std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["config", "user.email", "test@test.com"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["config", "user.name", "Test"])
            .status()
            .unwrap();
        std::fs::write(repo_dir.path().join("a.txt"), "hello").unwrap();
        std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["commit", "-q", "-m", "initial"])
            .status()
            .unwrap();

        {
            let config = daftprompt_indexer::IndexerConfig {
                cache_dir: Some(cache_dir.path().to_path_buf()),
                model_name: String::new(),
            };
            let mut indexer = daftprompt_indexer::Indexer::new(repo_dir.path(), &config).unwrap();
            let commits = vec![daftprompt_indexer::CommitData {
                sha: "deadbeef".to_string(),
                short_hash: "deadbee".to_string(),
                author_name: "Test".to_string(),
                time: "now".to_string(),
                message_title: "initial".to_string(),
                message_body: String::new(),
            }];
            indexer.index_commits(&commits).unwrap();
        }

        // This lab's own coverage lookup uses the *production* cache-dir
        // slug logic, not the test's custom `cache_dir`, so point it at a
        // custom-cache-dir replica by inspecting the file the indexer just
        // wrote directly rather than re-deriving the production path (which
        // would look under the real OS cache dir and always report
        // db_exists == false in this sandboxed test).
        let db_path = cache_dir_db_path(&cache_dir.path().to_path_buf(), repo_dir.path());

        daftprompt_indexer::db::register_sqlite_vec();
        let db = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let commit_count = daftprompt_indexer::db::existing_identifiers(&db, "commit")
            .unwrap()
            .len();
        assert_eq!(commit_count, 1);

        let recorded_repo_path = daftprompt_indexer::db::repo_meta_get(&db, "repo_path").unwrap();
        assert!(recorded_repo_path.is_some());

        // The DB file must be unchanged in size across two read-only opens
        // (a smoke check that nothing was silently written to it).
        let before = std::fs::metadata(&db_path).unwrap().len();
        drop(db);
        let db2 = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let _ = daftprompt_indexer::db::repo_meta_get(&db2, "repo_path").unwrap();
        drop(db2);
        let after = std::fs::metadata(&db_path).unwrap().len();
        assert_eq!(before, after, "read-only inspection must not mutate the DB file");
    }

    /// Duplicate of `Indexer::new`'s custom-cache-dir slug derivation
    /// (`crates/daftprompt-indexer/src/lib.rs`), used only to locate the DB
    /// this test just built under a temp cache dir. Production `inspect()`
    /// does not take this path; it always resolves the real OS cache dir via
    /// [`db_path_for_repo_readonly`].
    fn cache_dir_db_path(cache_dir: &std::path::Path, repo_path: &std::path::Path) -> PathBuf {
        let abs_path = std::fs::canonicalize(repo_path).unwrap();
        let abs_str = abs_path.to_string_lossy();
        let stem: String = abs_str
            .chars()
            .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect();
        let mut h: u64 = 0;
        for b in abs_str.bytes() {
            h = h.wrapping_mul(31).wrapping_add(b as u64);
        }
        let hash = format!("{:06x}", h & 0xFFFFFF);
        let slug = format!("{}_{}", stem, hash);
        cache_dir.join(format!("{}.db", slug))
    }
}
