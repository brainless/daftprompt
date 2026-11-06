//! Saved projects: a named local folder registered with daftprompt.

use serde::{Deserialize, Serialize};

/// A project in the registry.
///
/// Registering a project never copies or modifies the folder; it only records
/// where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Rust-generated identifier.
    pub id: String,
    /// Display name; defaults to the folder name.
    pub name: String,
    /// Absolute, canonicalized folder path. Unique across projects.
    pub root_path: String,
    /// RFC 3339 UTC creation time.
    pub created_at: String,
    /// RFC 3339 UTC time the project was last opened (creation time until the
    /// first explicit open).
    pub last_opened_at: String,
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

/// Body of `PATCH /projects/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameProjectRequest {
    /// The new display name; must not be blank.
    pub name: String,
}
