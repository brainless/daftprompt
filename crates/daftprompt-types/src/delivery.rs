//! Delivery metadata for agent results ([Epic 007](../../../epics/007-project-paths-and-agent-storage.md)).
//!
//! `BriefingResult` and `CommandsResult` stay the exact agent results, stored
//! and reloaded unchanged. [`Delivered`] wraps one with where it came from, so
//! a reused response is never mistaken for a new model run.

use serde::{Deserialize, Serialize};

/// An agent result and how it reached the caller.
///
/// The result's fields stay at the top level of the JSON (including those
/// flattened inside `CommandsResult`), so clients that predate this wrapper
/// ignore the two extra fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivered<T> {
    #[serde(flatten)]
    pub result: T,
    /// The registered path the request ran against. `None` only for the
    /// legacy unregistered fallback root, which has no path record.
    #[serde(default)]
    pub project_path_id: Option<String>,
    /// Where the result is stored. `None` when nothing was stored (the
    /// fallback root, or a service without a database).
    #[serde(default)]
    pub cache: Option<CacheDelivery>,
}

/// Which stored response was delivered and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheDelivery {
    /// The path the request ran against.
    pub requested_path_id: String,
    /// The path that generated (and owns) the response.
    pub source_path_id: String,
    /// The stored response's id: not a run id.
    pub response_id: String,
    /// When the response was stored (RFC 3339 UTC).
    pub captured_at: String,
    pub kind: CacheKind,
}

/// How a delivered result relates to the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheKind {
    /// A new run, stored just now. No model was skipped.
    Fresh,
    /// A verified earlier response of the requested path. No model ran.
    SamePath,
    /// A verified earlier response of another path of the same project. No
    /// model ran; `source_path_id` owns it.
    OtherPath,
}
