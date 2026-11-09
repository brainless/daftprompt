//! Saved projects: a named local folder registered with daftprompt.

use serde::{Deserialize, Serialize};

/// One registered local folder of a project.
///
/// A path belongs to exactly one project and is unique across the registry.
/// Registering a path never copies or modifies the folder; a folder that later
/// disappears stays registered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPath {
    /// Rust-generated identifier.
    pub id: String,
    /// The owning project.
    pub project_id: String,
    /// Absolute, canonicalized folder path. Unique across the registry.
    pub root_path: String,
    /// Whether requests that name no path use this one. Exactly one path of a
    /// project is the default.
    pub is_default: bool,
    /// RFC 3339 UTC time the path was registered.
    pub created_at: String,
    /// RFC 3339 UTC time the project was last opened through this path.
    pub last_opened_at: String,
}

/// A project in the registry: a logical identity with one or more paths.
///
/// Registering a project never copies or modifies the folder; it only records
/// where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Rust-generated identifier.
    pub id: String,
    /// Display name; defaults to the folder name.
    pub name: String,
    /// The default path's folder. Computed from `paths`, never stored or
    /// written separately; kept so clients that know one folder keep working.
    #[serde(default)]
    pub root_path: String,
    /// RFC 3339 UTC creation time.
    pub created_at: String,
    /// RFC 3339 UTC time the project was last opened (creation time until the
    /// first explicit open).
    pub last_opened_at: String,
    /// Every registered path, the default first. Never empty.
    #[serde(default)]
    pub paths: Vec<ProjectPath>,
    /// The id of the default path.
    #[serde(default)]
    pub default_path_id: String,
}

/// Body of `POST /projects`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    /// The folder to register. It must exist and be a directory.
    pub path: String,
    /// Optional display name; absent means the folder name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Body of `PATCH /projects/{id}`: rename, change the default path, or both.
/// At least one field is required.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateProjectRequest {
    /// The new display name; must not be blank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The id of a registered path of this project to make the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_path_id: Option<String>,
}

/// Body of `POST /projects/{id}/paths`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddProjectPathRequest {
    /// The folder to attach. It must exist and be a directory.
    pub path: String,
}
