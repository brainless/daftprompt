//! The project registry: named projects and their registered local folders.
//!
//! A project is a logical identity and a name; a path is one local folder
//! registered to it. Registry operations never read, write, or delete anything
//! inside a registered folder, and a folder that goes missing stays
//! registered. Every project always has at least one path and exactly one
//! default path; the transactions here keep it so.

use std::path::Path;

use daftprompt_types::{Project, ProjectPath};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{Store, StoreError};
use crate::scribe::support::{new_id, now};

const PROJECT_COLUMNS: &str = "id, name, created_at, last_opened_at";
const PATH_COLUMNS: &str = "id, project_id, root_path, is_default, created_at, last_opened_at";

fn path_from_row(row: &Row<'_>) -> rusqlite::Result<ProjectPath> {
    Ok(ProjectPath {
        id: row.get(0)?,
        project_id: row.get(1)?,
        root_path: row.get(2)?,
        is_default: row.get(3)?,
        created_at: row.get(4)?,
        last_opened_at: row.get(5)?,
    })
}

fn not_found(id: &str) -> StoreError {
    StoreError::NotFound(format!("no project with id {id:?}"))
}

fn path_not_found(project_id: &str, path_id: &str) -> StoreError {
    StoreError::NotFound(format!(
        "project {project_id:?} has no path with id {path_id:?}"
    ))
}

fn clean_name(name: &str) -> Result<String, StoreError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StoreError::Invalid("the project name is blank".to_owned()));
    }
    Ok(name.to_owned())
}

/// Canonicalize `path` (resolving symlinks) and require an existing
/// directory, so the same folder is never registered twice.
fn canonical_root(path: &Path) -> Result<(std::path::PathBuf, String), StoreError> {
    let root = path.canonicalize().map_err(|error| {
        StoreError::Invalid(format!("{} cannot be used: {error}", path.display()))
    })?;
    if !root.is_dir() {
        return Err(StoreError::Invalid(format!(
            "{} is not a directory",
            root.display()
        )));
    }
    let text = root
        .to_str()
        .ok_or_else(|| StoreError::Invalid(format!("{} is not valid UTF-8", root.display())))?
        .to_owned();
    Ok((root, text))
}

/// A project's paths, the default first, then in registration order.
fn load_paths(connection: &Connection, project_id: &str) -> Result<Vec<ProjectPath>, StoreError> {
    let mut statement = connection.prepare(&format!(
        "SELECT {PATH_COLUMNS} FROM project_path WHERE project_id = ?1
         ORDER BY is_default DESC, rowid"
    ))?;
    let rows = statement.query_map([project_id], path_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Assemble a project from its row (`PROJECT_COLUMNS`) and its paths.
fn assemble(
    connection: &Connection,
    id: String,
    name: String,
    created_at: String,
    last_opened_at: String,
) -> Result<Project, StoreError> {
    let paths = load_paths(connection, &id)?;
    let default = paths.iter().find(|path| path.is_default).ok_or_else(|| {
        StoreError::Storage(format!(
            "project {id:?} has no default path in the registry"
        ))
    })?;
    Ok(Project {
        root_path: default.root_path.clone(),
        default_path_id: default.id.clone(),
        id,
        name,
        created_at,
        last_opened_at,
        paths,
    })
}

fn load_project(connection: &Connection, id: &str) -> Result<Project, StoreError> {
    let (id, name, created_at, last_opened_at) = connection
        .query_row(
            &format!("SELECT {PROJECT_COLUMNS} FROM project WHERE id = ?1"),
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| not_found(id))?;
    assemble(connection, id, name, created_at, last_opened_at)
}

/// The project that owns the registered `root_path`, if any.
fn owner_of(connection: &Connection, root_path: &str) -> Result<Option<String>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT project_id FROM project_path WHERE root_path = ?1",
            [root_path],
            |row| row.get(0),
        )
        .optional()?)
}

impl Store {
    /// Register the folder at `path` as a project with that folder as its
    /// default path.
    ///
    /// The path must exist and be a directory; it is canonicalized (symlinks
    /// resolved) so the same folder is never registered twice. `name`
    /// defaults to the folder name. Adding a root that is already registered
    /// is not an error: the owning project is returned unchanged (its name
    /// is not overwritten), so "open this folder" is idempotent. The second
    /// element is `true` when a new project was created.
    pub fn add_project(
        &self,
        path: impl AsRef<Path>,
        name: Option<&str>,
    ) -> Result<(Project, bool), StoreError> {
        let (root, root_path) = canonical_root(path.as_ref())?;
        let name = match name {
            Some(name) => clean_name(name)?,
            None => root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&root_path)
                .to_owned(),
        };

        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        if let Some(owner) = owner_of(&transaction, &root_path)? {
            return Ok((load_project(&transaction, &owner)?, false));
        }

        let timestamp = now();
        let project_id = new_id("project");
        transaction.execute(
            &format!("INSERT INTO project ({PROJECT_COLUMNS}) VALUES (?1, ?2, ?3, ?4)"),
            params![project_id, name, timestamp, timestamp],
        )?;
        transaction.execute(
            &format!("INSERT INTO project_path ({PATH_COLUMNS}) VALUES (?1, ?2, ?3, 1, ?4, ?5)"),
            params![new_id("path"), project_id, root_path, timestamp, timestamp],
        )?;
        let project = load_project(&transaction, &project_id)?;
        transaction.commit()?;
        Ok((project, true))
    }

    /// All projects, most recently opened first (ties: newest created, then id).
    pub fn list_projects(&self) -> Result<Vec<Project>, StoreError> {
        let connection = self.lock();
        let mut statement = connection.prepare(&format!(
            "SELECT {PROJECT_COLUMNS} FROM project
             ORDER BY last_opened_at DESC, created_at DESC, id DESC"
        ))?;
        let rows: Vec<(String, String, String, String)> = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, name, created_at, last_opened_at)| {
                assemble(&connection, id, name, created_at, last_opened_at)
            })
            .collect()
    }

    /// One project by id.
    pub fn get_project(&self, id: &str) -> Result<Project, StoreError> {
        load_project(&self.lock(), id)
    }

    /// Set `last_opened_at` to now on the project and its default path, and
    /// return the project.
    pub fn mark_project_opened(&self, id: &str) -> Result<Project, StoreError> {
        let default = self.get_project(id)?.default_path_id;
        self.mark_path_opened(id, &default)
    }

    /// Set `last_opened_at` to now on the project and on its path
    /// `path_id`, and return the project.
    pub fn mark_path_opened(&self, id: &str, path_id: &str) -> Result<Project, StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        let timestamp = now();
        let changed = transaction.execute(
            "UPDATE project_path SET last_opened_at = ?1 WHERE id = ?2 AND project_id = ?3",
            params![timestamp, path_id, id],
        )?;
        if changed == 0 {
            load_project(&transaction, id)?;
            return Err(path_not_found(id, path_id));
        }
        transaction.execute(
            "UPDATE project SET last_opened_at = ?1 WHERE id = ?2",
            params![timestamp, id],
        )?;
        let project = load_project(&transaction, id)?;
        transaction.commit()?;
        Ok(project)
    }

    /// Change a project's display name.
    pub fn rename_project(&self, id: &str, name: &str) -> Result<Project, StoreError> {
        let name = clean_name(name)?;
        let connection = self.lock();
        let changed = connection.execute(
            "UPDATE project SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        if changed == 0 {
            return Err(not_found(id));
        }
        load_project(&connection, id)
    }

    /// Remove a project, and with it all its paths, from the registry. The
    /// folders on disk are untouched.
    pub fn remove_project(&self, id: &str) -> Result<(), StoreError> {
        let changed = self
            .lock()
            .execute("DELETE FROM project WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(not_found(id));
        }
        Ok(())
    }

    /// The registered paths of a project, the default first.
    pub fn list_paths(&self, project_id: &str) -> Result<Vec<ProjectPath>, StoreError> {
        Ok(self.get_project(project_id)?.paths)
    }

    /// One path of a project. A path id owned by another project is
    /// [`StoreError::NotFound`], the same as an unknown id.
    pub fn get_path(&self, project_id: &str, path_id: &str) -> Result<ProjectPath, StoreError> {
        let project = self.get_project(project_id)?;
        project
            .paths
            .into_iter()
            .find(|path| path.id == path_id)
            .ok_or_else(|| path_not_found(project_id, path_id))
    }

    /// Attach the folder at `path` to an existing project (never as the
    /// default; see [`Store::set_default_path`]).
    ///
    /// The same validation as [`Store::add_project`] applies. Attaching a
    /// folder the project already has returns that path unchanged; a folder
    /// registered to a different project is [`StoreError::Invalid`]. The second
    /// element is `true` when a new path was created.
    pub fn add_path(
        &self,
        project_id: &str,
        path: impl AsRef<Path>,
    ) -> Result<(ProjectPath, bool), StoreError> {
        let (_, root_path) = canonical_root(path.as_ref())?;
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM project WHERE id = ?1)",
            [project_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(not_found(project_id));
        }
        if let Some(owner) = owner_of(&transaction, &root_path)? {
            if owner != project_id {
                return Err(StoreError::Invalid(format!(
                    "{root_path} is already registered to another project"
                )));
            }
            let existing = transaction.query_row(
                &format!("SELECT {PATH_COLUMNS} FROM project_path WHERE root_path = ?1"),
                [&root_path],
                path_from_row,
            )?;
            return Ok((existing, false));
        }

        let timestamp = now();
        let added = ProjectPath {
            id: new_id("path"),
            project_id: project_id.to_owned(),
            root_path,
            is_default: false,
            created_at: timestamp.clone(),
            last_opened_at: timestamp,
        };
        transaction.execute(
            &format!("INSERT INTO project_path ({PATH_COLUMNS}) VALUES (?1, ?2, ?3, 0, ?4, ?5)"),
            params![
                added.id,
                added.project_id,
                added.root_path,
                added.created_at,
                added.last_opened_at
            ],
        )?;
        transaction.commit()?;
        Ok((added, true))
    }

    /// Make `path_id` the project's default path.
    pub fn set_default_path(&self, project_id: &str, path_id: &str) -> Result<Project, StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        make_default(&transaction, project_id, path_id)?;
        let project = load_project(&transaction, project_id)?;
        transaction.commit()?;
        Ok(project)
    }

    /// Rename and/or change the default path in one transaction. Nothing is
    /// applied if either part is invalid.
    pub fn update_project(
        &self,
        id: &str,
        name: Option<&str>,
        default_path_id: Option<&str>,
    ) -> Result<Project, StoreError> {
        if name.is_none() && default_path_id.is_none() {
            return Err(StoreError::Invalid(
                "nothing to update: give a name or a default_path_id".to_owned(),
            ));
        }
        let name = name.map(clean_name).transpose()?;
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        load_project(&transaction, id)?;
        if let Some(name) = name {
            transaction.execute(
                "UPDATE project SET name = ?1 WHERE id = ?2",
                params![name, id],
            )?;
        }
        if let Some(path_id) = default_path_id {
            make_default(&transaction, id, path_id)?;
        }
        let project = load_project(&transaction, id)?;
        transaction.commit()?;
        Ok(project)
    }

    /// Remove one path's registration; the folder is untouched.
    ///
    /// Removing the project's only path is [`StoreError::Invalid`] (remove the
    /// project instead). Removing the default path requires
    /// `replacement_default`, another path of the project, which becomes the
    /// default in the same transaction. A replacement given while removing a
    /// non-default path also becomes the default. Returns the updated project.
    pub fn remove_path(
        &self,
        project_id: &str,
        path_id: &str,
        replacement_default: Option<&str>,
    ) -> Result<Project, StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        let project = load_project(&transaction, project_id)?;
        let removed = project
            .paths
            .iter()
            .find(|path| path.id == path_id)
            .ok_or_else(|| path_not_found(project_id, path_id))?;
        if project.paths.len() == 1 {
            return Err(StoreError::Invalid(
                "a project must keep at least one path; remove the project instead".to_owned(),
            ));
        }
        if let Some(replacement) = replacement_default {
            if replacement == path_id || !project.paths.iter().any(|p| p.id == replacement) {
                return Err(StoreError::Invalid(format!(
                    "{replacement:?} is not another path of project {project_id:?}"
                )));
            }
        } else if removed.is_default {
            return Err(StoreError::Invalid(
                "the default path cannot be removed without choosing a replacement default"
                    .to_owned(),
            ));
        }
        transaction.execute("DELETE FROM project_path WHERE id = ?1", [path_id])?;
        if let Some(replacement) = replacement_default {
            make_default(&transaction, project_id, replacement)?;
        }
        let project = load_project(&transaction, project_id)?;
        transaction.commit()?;
        Ok(project)
    }
}

/// Move the default flag to `path_id`, which must belong to `project_id`.
/// The old default is cleared first: the unique index is checked per
/// statement.
fn make_default(
    connection: &Connection,
    project_id: &str,
    path_id: &str,
) -> Result<(), StoreError> {
    let owned: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM project_path WHERE id = ?1 AND project_id = ?2)",
        params![path_id, project_id],
        |row| row.get(0),
    )?;
    if !owned {
        load_project(connection, project_id)?;
        return Err(path_not_found(project_id, path_id));
    }
    connection.execute(
        "UPDATE project_path SET is_default = 0 WHERE project_id = ?1 AND is_default = 1",
        [project_id],
    )?;
    connection.execute(
        "UPDATE project_path SET is_default = 1 WHERE id = ?1",
        [path_id],
    )?;
    Ok(())
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

    #[test]
    fn a_new_project_has_one_default_path_and_a_computed_root() {
        let store = Store::open_in_memory().unwrap();
        let (_dir, path) = folder("app");
        let (project, _) = store.add_project(&path, None).unwrap();
        assert_eq!(project.paths.len(), 1);
        let only = &project.paths[0];
        assert!(only.is_default);
        assert_eq!(only.project_id, project.id);
        assert_eq!(project.default_path_id, only.id);
        assert_eq!(project.root_path, only.root_path);
    }

    #[test]
    fn two_paths_attach_idempotently_and_reject_other_owners() {
        let store = Store::open_in_memory().unwrap();
        let (_a, a) = folder("a");
        let (_b, b) = folder("b");
        let (_c, c) = folder("c");
        let (first, _) = store.add_project(&a, None).unwrap();
        let (second, _) = store.add_project(&c, None).unwrap();

        let (added, created) = store.add_path(&first.id, &b).unwrap();
        assert!(created);
        assert!(!added.is_default);
        let (again, created) = store.add_path(&first.id, b.join("..").join("b")).unwrap();
        assert!(!created);
        assert_eq!(again, added);
        // Re-adding the default path is also idempotent.
        let (default, created) = store.add_path(&first.id, &a).unwrap();
        assert!(!created && default.is_default);

        let error = store.add_path(&second.id, &b);
        assert!(matches!(error, Err(StoreError::Invalid(_))), "{error:?}");
        // Creating a project from a registered root returns the owner.
        let (owner, created) = store.add_project(&b, Some("Other")).unwrap();
        assert!(!created);
        assert_eq!(owner.id, first.id);
        assert_eq!(owner.paths.len(), 2);
        assert_eq!(owner.root_path, first.root_path);

        assert!(matches!(
            store.add_path("project-nope", &b),
            Err(StoreError::NotFound(_))
        ));
        let (dir, _) = folder("x");
        assert!(matches!(
            store.add_path(&first.id, dir.path().join("missing")),
            Err(StoreError::Invalid(_))
        ));
        assert_eq!(store.list_paths(&first.id).unwrap().len(), 2);
        assert_eq!(store.get_path(&first.id, &added.id).unwrap(), added);
    }

    #[test]
    fn wrong_path_ids_are_not_found() {
        let store = Store::open_in_memory().unwrap();
        let (_a, a) = folder("a");
        let (_b, b) = folder("b");
        let (first, _) = store.add_project(&a, None).unwrap();
        let (second, _) = store.add_project(&b, None).unwrap();
        for result in [
            store.get_path(&first.id, "path-nope").map(|_| ()),
            store
                .get_path(&first.id, &second.default_path_id)
                .map(|_| ()),
            store
                .set_default_path(&first.id, &second.default_path_id)
                .map(|_| ()),
            store
                .remove_path(&first.id, &second.default_path_id, None)
                .map(|_| ()),
            store
                .mark_path_opened(&first.id, &second.default_path_id)
                .map(|_| ()),
        ] {
            assert!(matches!(result, Err(StoreError::NotFound(_))), "{result:?}");
        }
        // Nothing changed.
        assert_eq!(store.get_project(&first.id).unwrap(), first);
        assert_eq!(store.get_project(&second.id).unwrap(), second);
    }

    #[test]
    fn the_default_is_explicit_and_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("daftprompt.db");
        let (_a, a) = folder("a");
        let (_b, b) = folder("b");
        let (id, second) = {
            let store = Store::open(&db).unwrap();
            let (project, _) = store.add_project(&a, None).unwrap();
            let (second, _) = store.add_path(&project.id, &b).unwrap();
            let updated = store.set_default_path(&project.id, &second.id).unwrap();
            assert_eq!(updated.default_path_id, second.id);
            assert_eq!(updated.root_path, second.root_path);
            assert_eq!(updated.paths[0].id, second.id);
            assert_eq!(updated.paths.iter().filter(|p| p.is_default).count(), 1);
            (project.id, second)
        };
        let store = Store::open(&db).unwrap();
        assert_eq!(store.get_project(&id).unwrap().default_path_id, second.id);
    }

    #[test]
    fn update_project_renames_and_sets_the_default_atomically() {
        let store = Store::open_in_memory().unwrap();
        let (_a, a) = folder("a");
        let (_b, b) = folder("b");
        let (project, _) = store.add_project(&a, None).unwrap();
        let (second, _) = store.add_path(&project.id, &b).unwrap();

        let updated = store
            .update_project(&project.id, Some(" New "), Some(&second.id))
            .unwrap();
        assert_eq!(updated.name, "New");
        assert_eq!(updated.default_path_id, second.id);

        // A bad half applies nothing.
        let error = store.update_project(&project.id, Some("Again"), Some("path-nope"));
        assert!(matches!(error, Err(StoreError::NotFound(_))), "{error:?}");
        assert_eq!(store.get_project(&project.id).unwrap().name, "New");
        assert!(matches!(
            store.update_project(&project.id, None, None),
            Err(StoreError::Invalid(_))
        ));
    }

    #[test]
    fn removing_a_path_keeps_one_default_and_never_the_last_path() {
        let store = Store::open_in_memory().unwrap();
        let (_a, a) = folder("a");
        let (_b, b) = folder("b");
        let (_c, c) = folder("c");
        let (project, _) = store.add_project(&a, None).unwrap();
        let (second, _) = store.add_path(&project.id, &b).unwrap();
        let (third, _) = store.add_path(&project.id, &c).unwrap();
        let first = project.default_path_id.clone();
        let project = store.get_project(&project.id).unwrap();

        // The default needs a replacement, which must be another path.
        for replacement in [None, Some(first.as_str()), Some("path-nope")] {
            let error = store.remove_path(&project.id, &first, replacement);
            assert!(matches!(error, Err(StoreError::Invalid(_))), "{error:?}");
        }
        assert_eq!(store.get_project(&project.id).unwrap(), project);

        let updated = store
            .remove_path(&project.id, &first, Some(&third.id))
            .unwrap();
        assert_eq!(updated.default_path_id, third.id);
        assert_eq!(updated.paths.len(), 2);
        assert_eq!(updated.paths.iter().filter(|p| p.is_default).count(), 1);

        // A non-default path goes without a replacement.
        let updated = store.remove_path(&project.id, &second.id, None).unwrap();
        assert_eq!(updated.paths.len(), 1);
        assert_eq!(updated.default_path_id, third.id);

        let error = store.remove_path(&project.id, &third.id, None);
        assert!(matches!(error, Err(StoreError::Invalid(_))), "{error:?}");
        // Folders are untouched.
        assert!(a.is_dir() && b.is_dir() && c.is_dir());
    }

    #[test]
    fn removing_a_project_cascades_to_its_paths() {
        let store = Store::open_in_memory().unwrap();
        let (_a, a) = folder("a");
        let (_b, b) = folder("b");
        let (project, _) = store.add_project(&a, None).unwrap();
        store.add_path(&project.id, &b).unwrap();
        store.remove_project(&project.id).unwrap();
        let count: i64 = store
            .lock()
            .query_row("SELECT COUNT(*) FROM project_path", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        // The freed root can be registered again.
        assert!(store.add_project(&a, None).unwrap().1);
    }

    #[test]
    fn opening_updates_the_project_and_that_path() {
        let store = Store::open_in_memory().unwrap();
        let (_a, a) = folder("a");
        let (_b, b) = folder("b");
        let (project, _) = store.add_project(&a, None).unwrap();
        let (second, _) = store.add_path(&project.id, &b).unwrap();
        let old = "2020-01-01T00:00:00Z";
        store
            .lock()
            .execute_batch(&format!(
                "UPDATE project SET last_opened_at = '{old}';
                 UPDATE project_path SET last_opened_at = '{old}';"
            ))
            .unwrap();

        let opened = store.mark_path_opened(&project.id, &second.id).unwrap();
        assert!(opened.last_opened_at.as_str() > old);
        for path in &opened.paths {
            let expected_new = path.id == second.id;
            assert_eq!(path.last_opened_at.as_str() > old, expected_new, "{path:?}");
        }

        let opened = store.mark_project_opened(&project.id).unwrap();
        let default = &opened.paths[0];
        assert_eq!(default.id, opened.default_path_id);
        assert!(default.last_opened_at.as_str() > old);
    }

    #[test]
    fn a_missing_folder_stays_registered_and_listed() {
        let store = Store::open_in_memory().unwrap();
        let (dir, a) = folder("a");
        let (project, _) = store.add_project(&a, None).unwrap();
        drop(dir);
        let listed = store.list_projects().unwrap();
        assert_eq!(listed, [project]);
    }
}
