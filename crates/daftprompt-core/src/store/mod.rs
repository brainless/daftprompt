//! Persistent storage in one SQLite database.
//!
//! [`Store`] owns the connection, the migrations, and the project registry. It
//! knows nothing about HTTP or the GUI. The graph tables from
//! `docs/data-model.md` (payload, node, label, run, ...) are not created yet;
//! the migration list is where they will be added.
//!
//! The database lives at `<OS user data dir>/daftprompt/daftprompt.db` (see
//! [`default_db_path`]). Callers can open any path with [`Store::open`] or an
//! in-memory database with [`Store::open_in_memory`].

mod projects;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use directories::ProjectDirs;
use rusqlite::Connection;

/// File name of the database inside the data directory.
const DB_FILE_NAME: &str = "daftprompt.db";

/// Ordered migrations. Entry `n` (0-based) upgrades `user_version` from `n` to
/// `n + 1`; append new entries, never edit shipped ones.
const MIGRATIONS: &[&str] = &[
    // 1: the project registry.
    "CREATE TABLE project (
       id              TEXT PRIMARY KEY,
       name            TEXT NOT NULL,
       root_path       TEXT NOT NULL UNIQUE,
       created_at      TEXT NOT NULL,
       last_opened_at  TEXT NOT NULL
     );",
];

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
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let transaction = connection.transaction()?;
        transaction.execute_batch(sql)?;
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
}
