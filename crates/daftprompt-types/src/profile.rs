//! The deterministic project profile of [Epic 006](../../../epics/006-project-briefing.md):
//! which languages a project uses and the files worth reading first.
//!
//! Rust computes it from the project files with no model call; the shapes live
//! here so the API, the GUI, and the briefing agents share them.

use serde::{Deserialize, Serialize};

/// A language daftprompt has built-in knowledge of. TypeScript covers
/// JavaScript projects too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageId {
    Rust,
    #[serde(rename = "typescript")]
    TypeScript,
    Python,
}

impl LanguageId {
    /// The wire name, for example `typescript`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::Python => "python",
        }
    }
}

/// One project (a manifest directory) of a language. A monorepo has several.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LanguageProject {
    /// Root-relative directory of the manifest; empty is the project root.
    pub dir: String,
    /// The package name the manifest declares, when it parsed.
    pub name: Option<String>,
    /// `cargo`, `npm`, `pnpm`, `yarn`, `bun`, `uv`, `poetry` or `pip`.
    pub package_manager: Option<String>,
    /// Toolchain, runtime or dialect: the Rust channel, `typescript` or
    /// `javascript`, or Python's `requires-python`.
    pub toolchain: Option<String>,
    /// The manifest declares a workspace of several packages.
    pub workspace: bool,
    /// Names (or directories, for unnamed ones) of workspace member packages
    /// found under this project, sorted.
    pub packages: Vec<String>,
    /// Marker files that are the evidence, root-relative and sorted.
    pub files: Vec<String>,
}

/// What one language's detection found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageDetection {
    pub language: LanguageId,
    /// One entry per project, in directory order.
    pub projects: Vec<LanguageProject>,
    /// The search or the number of manifests hit a limit, so more may exist.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// The deterministic profile of a project folder.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProjectProfile {
    /// Detected languages in registry order. Empty when none was recognized.
    pub languages: Vec<LanguageDetection>,
    /// Root-relative files worth reading first: documentation, then each
    /// language's manifests.
    pub key_files: Vec<String>,
}
