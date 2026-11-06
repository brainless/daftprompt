//! The project registry: named local folders.
//!
//! Registry operations never read, write, or delete anything inside a
//! project's folder.

use std::path::Path;

use daftprompt_types::Project;
use rusqlite::{OptionalExtension, Row, params};

use super::{Store, StoreError};
use crate::scribe::support::{new_id, now};

const COLUMNS: &str = "id, name, root_path, created_at, last_opened_at";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        root_path: row.get(2)?,
        created_at: row.get(3)?,
        last_opened_at: row.get(4)?,
    })
}

fn not_found(id: &str) -> StoreError {
    StoreError::NotFound(format!("no project with id {id:?}"))
}

fn clean_name(name: &str) -> Result<String, StoreError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StoreError::Invalid("the project name is blank".to_owned()));
    }
    Ok(name.to_owned())
}

impl Store {
    /// Register the folder at `path` as a project.
    ///
    /// The path must exist and be a directory; it is canonicalized (symlinks
    /// resolved) so the same folder is never registered twice. `name`
    /// defaults to the folder name. Adding a root that is already registered
    /// is not an error: the existing project is returned unchanged (its name
    /// is not overwritten), so "open this folder" is idempotent. The second
    /// element is `true` when a new project was created.
    pub fn add_project(
        &self,
        path: impl AsRef<Path>,
        name: Option<&str>,
    ) -> Result<(Project, bool), StoreError> {
        let path = path.as_ref();
        let root = path.canonicalize().map_err(|error| {
            StoreError::Invalid(format!("{} cannot be used: {error}", path.display()))
        })?;
        if !root.is_dir() {
            return Err(StoreError::Invalid(format!(
                "{} is not a directory",
                root.display()
            )));
        }
        let root_path = root
            .to_str()
            .ok_or_else(|| StoreError::Invalid(format!("{} is not valid UTF-8", root.display())))?;
        let name = match name {
            Some(name) => clean_name(name)?,
            None => root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(root_path)
                .to_owned(),
        };

        let connection = self.lock();
        let existing = connection
            .query_row(
                &format!("SELECT {COLUMNS} FROM project WHERE root_path = ?1"),
                [root_path],
                from_row,
            )
            .optional()?;
        if let Some(project) = existing {
            return Ok((project, false));
        }

        let timestamp = now();
        let project = Project {
            id: new_id("project"),
            name,
            root_path: root_path.to_owned(),
            created_at: timestamp.clone(),
            last_opened_at: timestamp,
        };
        connection.execute(
            &format!("INSERT INTO project ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5)"),
            params![
                project.id,
                project.name,
                project.root_path,
                project.created_at,
                project.last_opened_at
            ],
        )?;
        Ok((project, true))
    }

    /// All projects, most recently opened first (ties: newest created, then id).
    pub fn list_projects(&self) -> Result<Vec<Project>, StoreError> {
        let connection = self.lock();
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} FROM project ORDER BY last_opened_at DESC, created_at DESC, id DESC"
        ))?;
        let rows = statement.query_map([], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// One project by id.
    pub fn get_project(&self, id: &str) -> Result<Project, StoreError> {
        self.lock()
            .query_row(
                &format!("SELECT {COLUMNS} FROM project WHERE id = ?1"),
                [id],
                from_row,
            )
            .optional()?
            .ok_or_else(|| not_found(id))
    }

    /// Set `last_opened_at` to now and return the project.
    pub fn mark_project_opened(&self, id: &str) -> Result<Project, StoreError> {
        let changed = self.lock().execute(
            "UPDATE project SET last_opened_at = ?1 WHERE id = ?2",
            params![now(), id],
        )?;
        if changed == 0 {
            return Err(not_found(id));
        }
        self.get_project(id)
    }

    /// Change a project's display name.
    pub fn rename_project(&self, id: &str, name: &str) -> Result<Project, StoreError> {
        let name = clean_name(name)?;
        let changed = self.lock().execute(
            "UPDATE project SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        if changed == 0 {
            return Err(not_found(id));
        }
        self.get_project(id)
    }

    /// Remove a project from the registry. The folder on disk is untouched.
    pub fn remove_project(&self, id: &str) -> Result<(), StoreError> {
        let changed = self
            .lock()
            .execute("DELETE FROM project WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(not_found(id));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::create_dir(&path).unwrap();
        (dir, path)
    }

    #[test]
    fn add_defaults_the_name_and_canonicalizes() {
        let store = Store::open_in_memory().unwrap();
        let (_dir, path) = folder("my-app");
        let (project, created) = store.add_project(&path, None).unwrap();
        assert!(created);
        assert_eq!(project.name, "my-app");
        assert_eq!(
            project.root_path,
            path.canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(store.get_project(&project.id).unwrap(), project);
    }

    #[test]
    fn adding_an_existing_root_returns_the_existing_project() {
        let store = Store::open_in_memory().unwrap();
        let (_dir, path) = folder("app");
        let (first, _) = store.add_project(&path, Some("First")).unwrap();
        // A different spelling of the same folder resolves to the same root.
        let alias = path.join("..").join("app");
        let (second, created) = store.add_project(&alias, Some("Second")).unwrap();
        assert!(!created);
        assert_eq!(second, first);
        assert_eq!(store.list_projects().unwrap().len(), 1);
    }

    #[test]
    fn bad_paths_and_names_are_rejected() {
        let store = Store::open_in_memory().unwrap();
        let (dir, path) = folder("app");
        assert!(matches!(
            store.add_project(dir.path().join("missing"), None),
            Err(StoreError::Invalid(_))
        ));
        let file = path.join("file.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(matches!(
            store.add_project(&file, None),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            store.add_project(&path, Some("   ")),
            Err(StoreError::Invalid(_))
        ));
    }

    #[test]
    fn list_orders_by_last_opened() {
        let store = Store::open_in_memory().unwrap();
        let (_a, a) = folder("a");
        let (_b, b) = folder("b");
        let (first, _) = store.add_project(&a, None).unwrap();
        let (second, _) = store.add_project(&b, None).unwrap();
        // Force distinct timestamps (second precision) deterministically.
        let connection = store.lock();
        connection
            .execute(
                "UPDATE project SET last_opened_at = '2020-01-01T00:00:00Z' WHERE id = ?1",
                [&first.id],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE project SET last_opened_at = '2021-01-01T00:00:00Z' WHERE id = ?1",
                [&second.id],
            )
            .unwrap();
        drop(connection);
        let ids: Vec<_> = store
            .list_projects()
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, [second.id.clone(), first.id.clone()]);

        let opened = store.mark_project_opened(&first.id).unwrap();
        assert!(opened.last_opened_at.as_str() > "2021-01-01T00:00:00Z");
        assert_eq!(store.list_projects().unwrap()[0].id, first.id);
    }

    #[test]
    fn rename_and_remove_leave_the_folder_alone() {
        let store = Store::open_in_memory().unwrap();
        let (_dir, path) = folder("app");
        std::fs::write(path.join("keep.txt"), "x").unwrap();
        let (project, _) = store.add_project(&path, None).unwrap();

        let renamed = store.rename_project(&project.id, "  Renamed ").unwrap();
        assert_eq!(renamed.name, "Renamed");
        assert!(matches!(
            store.rename_project(&project.id, ""),
            Err(StoreError::Invalid(_))
        ));

        store.remove_project(&project.id).unwrap();
        assert!(path.join("keep.txt").is_file());
        assert!(matches!(
            store.get_project(&project.id),
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            store.remove_project(&project.id),
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            store.mark_project_opened("nope"),
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            store.rename_project("nope", "x"),
            Err(StoreError::NotFound(_))
        ));
    }

    #[test]
    fn projects_survive_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db/daftprompt.db");
        let (_f, path) = folder("app");
        let id = {
            let store = Store::open(&db).unwrap();
            store.add_project(&path, None).unwrap().0.id
        };
        let store = Store::open(&db).unwrap();
        assert_eq!(store.get_project(&id).unwrap().name, "app");
    }
}
