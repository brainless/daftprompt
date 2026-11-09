//! Persistent storage in one SQLite database.
//!
//! [`Store`] owns the connection, the migrations, and the project registry. It
//! knows nothing about HTTP or the GUI. The graph tables from
//! `docs/data-model.md` (payload, node, label, run, ...) are not created yet;
//! the migration list is where they will be added. Successful briefing
//! responses are stored as versioned snapshots (see [`StoredAgentResponseV1`]).
//!
//! The database lives at `<OS user data dir>/daftprompt/daftprompt.db` (see
//! [`default_db_path`]). Callers can open any path with [`Store::open`] or an
//! in-memory database with [`Store::open_in_memory`].

mod projects;
mod responses;

pub use responses::{
    CandidateQuery, FileInputsV1, NewResponse, ObservedCall, RESPONSE_VERSION, ResponseMeta,
    ResponseOutcome, ResponseQuery, ReuseInputsV1, StoredAgent, StoredAgentResponseV1,
    StoredDetailsV1, StoredResponse, StoredResult, UnusableReason, UnusableResponse,
};

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use directories::ProjectDirs;
use rusqlite::{Connection, Transaction, params};

use crate::scribe::support::new_id;

/// File name of the database inside the data directory.
const DB_FILE_NAME: &str = "daftprompt.db";

/// One schema upgrade step.
enum Migration {
    /// Plain SQL, run with `execute_batch`.
    Sql(&'static str),
    /// Code, for steps that need Rust-owned values such as generated ids.
    Rust(fn(&Transaction) -> Result<(), StoreError>),
}

/// Ordered migrations. Entry `n` (0-based) upgrades `user_version` from `n` to
/// `n + 1`; append new entries, never edit shipped ones.
///
/// Each entry runs in its own transaction, and `user_version` is set in that
/// transaction, so a failed step leaves the database at the previous version.
///
/// Rebuilding a table (rename, create, copy, drop) inside a migration is only
/// safe while no other table references it: `PRAGMA foreign_keys` cannot be
/// changed inside a transaction, and the connection runs with foreign keys on.
/// Migration 2 rebuilds `project` because nothing referenced it yet. Now that
/// `project_path` references `project` and `project_agent_response` references
/// `project_path`, a rebuild of any referenced table must follow SQLite's table-rebuild procedure instead: turn
/// foreign keys off on the connection before the transaction starts, rebuild,
/// run `PRAGMA foreign_key_check`, commit, and turn them back on.
const MIGRATIONS: &[Migration] = &[
    // 1: the project registry.
    Migration::Sql(
        "CREATE TABLE project (
       id              TEXT PRIMARY KEY,
       name            TEXT NOT NULL,
       root_path       TEXT NOT NULL UNIQUE,
       created_at      TEXT NOT NULL,
       last_opened_at  TEXT NOT NULL
     );",
    ),
    // 2: a project owns one or more registered paths.
    Migration::Rust(split_project_paths),
    // 3: append-only snapshots of successful agent responses.
    Migration::Sql(
        "CREATE TABLE project_agent_response (
           id                  TEXT PRIMARY KEY,
           project_id          TEXT NOT NULL,
           project_path_id     TEXT NOT NULL,
           agent               TEXT NOT NULL
             CHECK (agent IN ('chronicler', 'surveyor', 'operator')),
           request_key         TEXT NOT NULL,
           config_key          TEXT NOT NULL,
           commit_hash         TEXT,
           repository_subpath  TEXT,
           reusable            INTEGER NOT NULL CHECK (reusable IN (0, 1)),
           response_version    INTEGER NOT NULL,
           summary_text        TEXT NOT NULL,
           details_json        TEXT NOT NULL CHECK (json_valid(details_json)),
           created_at          TEXT NOT NULL,
           FOREIGN KEY (project_id, project_path_id)
             REFERENCES project_path(project_id, id) ON DELETE CASCADE
         );
         CREATE INDEX agent_response_path_lookup ON project_agent_response
           (project_id, project_path_id, agent, request_key, config_key, created_at DESC);
         CREATE INDEX agent_response_commit_lookup ON project_agent_response
           (project_id, agent, request_key, config_key, commit_hash, repository_subpath);",
    ),
];

/// Migration 2: move each project's `root_path` into a default `project_path`.
///
/// Project ids, names, and timestamps are preserved. The path gets a
/// Rust-generated id and its project's timestamps. Folders are not touched, so
/// a root that no longer exists is still registered.
fn split_project_paths(transaction: &Transaction) -> Result<(), StoreError> {
    transaction.execute_batch(
        "ALTER TABLE project RENAME TO project_v1;
         CREATE TABLE project (
           id              TEXT PRIMARY KEY,
           name            TEXT NOT NULL,
           created_at      TEXT NOT NULL,
           last_opened_at  TEXT NOT NULL
         );
         CREATE TABLE project_path (
           id              TEXT PRIMARY KEY,
           project_id      TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
           root_path       TEXT NOT NULL UNIQUE,
           is_default      INTEGER NOT NULL CHECK (is_default IN (0, 1)),
           created_at      TEXT NOT NULL,
           last_opened_at  TEXT NOT NULL,
           UNIQUE(project_id, id)
         );
         CREATE UNIQUE INDEX project_one_default_path
           ON project_path(project_id) WHERE is_default = 1;
         INSERT INTO project (id, name, created_at, last_opened_at)
           SELECT id, name, created_at, last_opened_at FROM project_v1;",
    )?;
    let old: Vec<(String, String, String, String)> = {
        let mut statement = transaction
            .prepare("SELECT id, root_path, created_at, last_opened_at FROM project_v1")?;
        statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<_, _>>()?
    };
    for (project_id, root_path, created_at, last_opened_at) in old {
        transaction.execute(
            "INSERT INTO project_path
               (id, project_id, root_path, is_default, created_at, last_opened_at)
             VALUES (?1, ?2, ?3, 1, ?4, ?5)",
            params![
                new_id("path"),
                project_id,
                root_path,
                created_at,
                last_opened_at
            ],
        )?;
    }
    transaction.execute_batch("DROP TABLE project_v1;")?;
    Ok(())
}

/// Why a store operation failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The caller's input is unusable (bad path, blank name).
    #[error("{0}")]
    Invalid(String),
    /// No record has this id.
    #[error("{0}")]
    NotFound(String),
    /// SQLite, filesystem, or migration failure.
    #[error("storage failure: {0}")]
    Storage(String),
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

/// The default database path for this OS and user.
///
/// `ProjectDirs` (not `BaseDirs`) gives an application-specific directory
/// that follows each platform's convention: `~/Library/Application
/// Support/daftprompt` on macOS, `$XDG_DATA_HOME/daftprompt` on Linux, and
/// `%APPDATA%\daftprompt\data` on Windows. Returns `None` when no home
/// directory can be determined.
pub fn default_db_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "daftprompt").map(|dirs| dirs.data_dir().join(DB_FILE_NAME))
}

/// A SQLite-backed store. Cheap to share by reference or `Arc`: the
/// connection is serialized behind a mutex.
#[derive(Debug)]
pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    /// Open (creating the file and its parent directories if missing) the
    /// database at `path` and apply pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|error| {
                StoreError::Storage(format!("could not create {}: {error}", parent.display()))
            })?;
        }
        Self::from_connection(Connection::open(path)?)
    }

    /// Open the database at [`default_db_path`].
    pub fn open_default() -> Result<Self, StoreError> {
        let path = default_db_path().ok_or_else(|| {
            StoreError::Storage("could not determine the user data directory".to_owned())
        })?;
        Self::open(path)
    }

    /// A private in-memory database, for tests.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, StoreError> {
        connection.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock cannot leave SQLite inconsistent, so
        // a poisoned mutex is safe to keep using.
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Apply every migration newer than the database's `user_version`, each in
/// its own transaction.
fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let current: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current as usize > MIGRATIONS.len() {
        return Err(StoreError::Storage(format!(
            "the database schema version {current} is newer than this build supports ({})",
            MIGRATIONS.len()
        )));
    }
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let transaction = connection.transaction()?;
        match migration {
            Migration::Sql(sql) => transaction.execute_batch(sql)?,
            Migration::Rust(apply) => apply(&transaction)?,
        }
        transaction.pragma_update(None, "user_version", index as i64 + 1)?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_parent_directories_and_migrates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/daftprompt.db");
        let store = Store::open(&path).unwrap();
        assert!(path.is_file());
        let version: i64 = store
            .lock()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        drop(store);
        // Reopening is a no-op migration.
        Store::open(&path).unwrap();
    }

    #[test]
    fn a_newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        drop(connection);
        assert!(matches!(Store::open(&path), Err(StoreError::Storage(_))));
    }

    #[test]
    fn default_path_ends_with_the_documented_name() {
        if let Some(path) = default_db_path() {
            assert!(
                path.ends_with("daftprompt/daftprompt.db") || path.ends_with("data/daftprompt.db")
            );
        }
    }

    /// A database exactly as migration 1 left it, with two projects.
    fn v1_database(path: &Path, roots: [&str; 2]) {
        let connection = Connection::open(path).unwrap();
        let Migration::Sql(sql) = &MIGRATIONS[0] else {
            panic!("migration 1 is SQL");
        };
        connection.execute_batch(sql).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        for (n, root) in roots.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO project VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        format!("project-{n}"),
                        format!("Name {n}"),
                        root,
                        format!("2021-01-0{}T00:00:00Z", n + 1),
                        format!("2022-01-0{}T00:00:00Z", n + 1),
                    ],
                )
                .unwrap();
        }
    }

    #[test]
    fn upgrading_v1_keeps_projects_and_adds_default_paths() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("present");
        std::fs::create_dir(&present).unwrap();
        let missing = dir.path().join("gone");
        let db = dir.path().join("db.sqlite");
        v1_database(&db, [present.to_str().unwrap(), missing.to_str().unwrap()]);

        let store = Store::open(&db).unwrap();
        let mut projects = store.list_projects().unwrap();
        projects.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(projects.len(), 2);
        for (n, (project, root)) in projects.iter().zip([&present, &missing]).enumerate() {
            assert_eq!(project.id, format!("project-{n}"));
            assert_eq!(project.name, format!("Name {n}"));
            assert_eq!(project.created_at, format!("2021-01-0{}T00:00:00Z", n + 1));
            assert_eq!(
                project.last_opened_at,
                format!("2022-01-0{}T00:00:00Z", n + 1)
            );
            // A missing root is still registered, as the only default path.
            assert_eq!(project.root_path, root.to_str().unwrap());
            assert_eq!(project.paths.len(), 1);
            let path = &project.paths[0];
            assert!(path.is_default);
            assert!(path.id.starts_with("path-"));
            assert_eq!(project.default_path_id, path.id);
            assert_eq!(path.project_id, project.id);
            assert_eq!(path.created_at, project.created_at);
            assert_eq!(path.last_opened_at, project.last_opened_at);
        }
        let ids = projects
            .iter()
            .map(|p| &p.default_path_id)
            .collect::<Vec<_>>();
        assert_ne!(ids[0], ids[1]);

        let connection = store.lock();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let leftover: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'project_v1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(leftover, 0);
        drop(connection);
        let default_ids: Vec<_> = projects.iter().map(|p| p.default_path_id.clone()).collect();
        drop(store);

        // Reopening does not migrate again: path ids are unchanged.
        let store = Store::open(&db).unwrap();
        let mut again = store.list_projects().unwrap();
        again.sort_by(|a, b| a.id.cmp(&b.id));
        let again: Vec<_> = again.into_iter().map(|p| p.default_path_id).collect();
        assert_eq!(again, default_ids);
    }

    #[test]
    fn the_registry_allows_only_one_default_path_per_project() {
        let store = Store::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (project, _) = store.add_project(dir.path(), None).unwrap();
        let error = store.lock().execute(
            "INSERT INTO project_path VALUES ('p2', ?1, '/other', 1, 't', 't')",
            [&project.id],
        );
        assert!(error.is_err());
    }
}
