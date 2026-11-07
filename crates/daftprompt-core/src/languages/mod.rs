//! Deterministic, language-specific project knowledge.
//!
//! Each language is one module behind the [`LanguageSupport`] trait and one
//! line in the [registry](registry). It finds its marker files (manifests,
//! lockfiles) through the read-only [`ProjectFiles`] backend, so it respects
//! the ignore and hidden-file rules and the listing limits of the project
//! tools and runs against [`InMemoryProject`](crate::project_tools::InMemoryProject)
//! in tests. Detection walks nested manifests, so a monorepo reports each
//! project with its own directory and package manager.
//!
//! [`profile`] combines every registered language with the generic
//! documentation files into a [`ProjectProfile`], with no model call.
//! [`command_candidates`] parses each language's manifests into
//! [`CommandCandidate`]s: the commands the project supports, with the manifest
//! lines they came from. Nothing is ever run.

mod python;
mod rust;
mod scan;
mod typescript;

use std::path::Path;

use serde::Serialize;

pub use daftprompt_types::{
    CommandKind, LanguageDetection, LanguageId, LanguageProject, ProjectProfile,
};

use crate::project_tools::{FsProject, ProjectFiles, ProjectToolConfig, Scope, ToolError};

/// What one language knows about a project.
pub trait LanguageSupport: Send + Sync {
    /// The language this implementation covers.
    fn id(&self) -> LanguageId;

    /// Look for the language's marker files. `None` means the language is not
    /// used. Malformed manifests never fail detection: the project is still
    /// reported, with less detail.
    fn detect(&self, files: &dyn ProjectFiles) -> Option<LanguageDetection>;

    /// Root-relative manifests and similar files worth reading first.
    fn key_files(&self, detection: &LanguageDetection) -> Vec<String>;

    /// The commands the project's manifests support, parsed deterministically
    /// from the files `detection` found. Malformed manifests yield fewer (or
    /// no) candidates, never an error.
    fn command_candidates(
        &self,
        files: &dyn ProjectFiles,
        detection: &LanguageDetection,
    ) -> Vec<CommandCandidate>;
}

/// A command the project's manifests support, found by Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandCandidate {
    /// Exactly as it would be typed. One line, never chained with `&&`.
    pub command: String,
    /// Root-relative directory to run it in (a `LanguageProject.dir` or, for
    /// a workspace member's scripts, the member's directory); empty is the root.
    pub working_dir: String,
    pub kind: CommandKind,
    pub language: LanguageId,
    /// A short suggested button label, at most 24 characters.
    pub label: String,
    /// The manifest lines the command comes from.
    pub evidence: CommandEvidence,
}

/// Manifest lines that justify a [`CommandCandidate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandEvidence {
    /// Root-relative manifest path.
    pub path: String,
    /// Inclusive, 1-based, computed by Rust from the manifest text.
    pub start_line: u32,
    pub end_line: u32,
}

/// The most candidates [`command_candidates`] returns across languages.
pub const MAX_CANDIDATES: usize = 120;

/// Every candidate of the languages `profile` detected, in registry order.
pub fn command_candidates(
    files: &dyn ProjectFiles,
    profile: &ProjectProfile,
) -> Vec<CommandCandidate> {
    let mut all = Vec::new();
    for detection in &profile.languages {
        if let Some(language) = registry().iter().find(|l| l.id() == detection.language) {
            all.extend(language.command_candidates(files, detection));
        }
    }
    all.truncate(MAX_CANDIDATES);
    all
}

/// Every supported language. Adding one means adding its module and one line
/// here.
pub fn registry() -> &'static [&'static dyn LanguageSupport] {
    static REGISTRY: [&dyn LanguageSupport; 3] =
        [&rust::Rust, &typescript::TypeScript, &python::Python];
    &REGISTRY
}

/// The most key files listed in a profile.
pub const MAX_KEY_FILES: usize = 40;
/// The most files taken from the `docs/` folder.
const MAX_DOC_FILES: usize = 20;

/// The profile of the project `files` shows.
pub fn profile(files: &dyn ProjectFiles) -> ProjectProfile {
    let languages: Vec<LanguageDetection> = registry()
        .iter()
        .filter_map(|language| language.detect(files))
        .collect();

    // Priority under the cap: top-level docs, docs directly in `docs/`,
    // manifests, then nested `docs/**` files.
    let docs = generic_key_files(files);
    let mut key_files = docs.top;
    key_files.extend(docs.direct);
    for detection in &languages {
        if let Some(language) = registry().iter().find(|l| l.id() == detection.language) {
            key_files.extend(language.key_files(detection));
        }
    }
    key_files.extend(docs.nested);
    let mut seen = std::collections::BTreeSet::new();
    key_files.retain(|path| seen.insert(path.clone()));
    key_files.truncate(MAX_KEY_FILES);
    ProjectProfile {
        languages,
        key_files,
    }
}

/// The profile of the project folder at `root`.
pub fn profile_root(root: impl AsRef<Path>) -> Result<ProjectProfile, ToolError> {
    let project = FsProject::new(root)?;
    Ok(profile(&project))
}

/// Documentation every project may have, independent of language, in
/// priority groups.
struct GenericDocs {
    /// Root-level README, DEVELOP(MENT), CONTRIBUTING and ARCHITECTURE files,
    /// AGENTS.md and CLAUDE.md.
    top: Vec<String>,
    /// Text files directly in `docs/`.
    direct: Vec<String>,
    /// Text files in subdirectories of `docs/`. Together with `direct` at
    /// most [`MAX_DOC_FILES`] files are taken from `docs/`.
    nested: Vec<String>,
}

fn generic_key_files(files: &dyn ProjectFiles) -> GenericDocs {
    let (mut roots, _) = scan::find(
        files,
        "{README*,Readme*,readme*,DEVELOP*,Develop*,develop*,CONTRIBUTING*,Contributing*,\
         contributing*,ARCHITECTURE*,Architecture*,architecture*,AGENTS.md,CLAUDE.md}",
    );
    roots.retain(|path| !path.contains('/') && is_root_doc(path));
    roots.sort_by_key(|path| (doc_rank(path), path.to_ascii_lowercase()));

    let mut direct = Vec::new();
    let mut nested = Vec::new();
    if let Ok(scope) = Scope::new(Some("docs".to_owned()), None)
        && let Ok(listing) = files.list(&scope, &ProjectToolConfig::default())
    {
        let mut docs: Vec<String> = listing
            .paths
            .into_iter()
            .filter(|path| is_doc_text(path))
            .collect();
        docs.sort();
        for path in docs {
            if path.matches('/').count() <= 1 {
                direct.push(path);
            } else {
                nested.push(path);
            }
        }
        direct.truncate(MAX_DOC_FILES);
        nested.truncate(MAX_DOC_FILES.saturating_sub(direct.len()));
    }
    GenericDocs {
        top: roots,
        direct,
        nested,
    }
}

fn is_root_doc(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper == "AGENTS.MD"
        || upper == "CLAUDE.MD"
        || (["README", "DEVELOP", "CONTRIBUTING", "ARCHITECTURE"]
            .iter()
            .any(|prefix| upper.starts_with(prefix))
            && is_doc_text(name))
}

/// Documentation text: a known extension, or none at all (`README`).
fn is_doc_text(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    match name.rsplit_once('.') {
        None => true,
        Some((_, ext)) => matches!(ext, "md" | "mdx" | "markdown" | "rst" | "txt" | "adoc"),
    }
}

/// README first, then the contributor docs, then agent instructions.
fn doc_rank(name: &str) -> u8 {
    let upper = name.to_ascii_uppercase();
    if upper.starts_with("README") {
        0
    } else if upper.starts_with("DEVELOP") || upper.starts_with("CONTRIBUTING") {
        1
    } else if upper.starts_with("ARCHITECTURE") {
        2
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_docs_are_recognized_case_insensitively() {
        for name in [
            "README.md",
            "readme",
            "README.rst",
            "DEVELOPMENT.md",
            "Contributing.md",
            "ARCHITECTURE.md",
            "AGENTS.md",
            "CLAUDE.md",
        ] {
            assert!(is_root_doc(name), "{name}");
        }
        for name in ["Cargo.toml", "READMEFIRST.png", "main.rs", "authors.md"] {
            assert!(!is_root_doc(name), "{name}");
        }
    }
}
