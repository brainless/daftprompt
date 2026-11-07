//! Helpers shared by the language modules.

use daftprompt_types::{CommandKind, LanguageDetection, LanguageId, LanguageProject};
use globset::{Glob, GlobSet, GlobSetBuilder};

use super::{CommandCandidate, CommandEvidence};
use crate::project_tools::{ProjectFiles, ProjectToolConfig, Scope};

/// Manifests deeper than this many directories are not looked at.
pub(super) const MAX_DEPTH: usize = 4;
/// Marker files taken per search; more set `truncated`.
pub(super) const MAX_MARKERS: usize = 200;
/// Directories whose contents are dependencies or build output, never part of
/// the project itself, even when not ignored.
const VENDORED: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    "venv",
    "site-packages",
    "__pycache__",
];

/// Marker files whose file name matches `glob` (a name glob, for example
/// `Cargo.toml` or `{uv.lock,poetry.lock}`), root-relative and sorted, with
/// vendored and over-deep paths removed. The flag reports a hit limit.
pub(super) fn find(files: &dyn ProjectFiles, glob: &str) -> (Vec<String>, bool) {
    let config = ProjectToolConfig {
        max_list_files: MAX_MARKERS,
        ..ProjectToolConfig::default()
    };
    let Ok(scope) = Scope::new(None, Some(glob)) else {
        return (Vec::new(), false);
    };
    match files.list(&scope, &config) {
        Ok(listing) => {
            let mut paths: Vec<String> = listing
                .paths
                .into_iter()
                .filter(|path| {
                    let mut parts: Vec<&str> = path.split('/').collect();
                    parts.pop();
                    parts.len() <= MAX_DEPTH && !parts.iter().any(|p| VENDORED.contains(p))
                })
                .collect();
            paths.sort();
            (paths, listing.truncated)
        }
        Err(_) => (Vec::new(), false),
    }
}

/// The text of one file, if it can be read.
pub(super) fn read(files: &dyn ProjectFiles, path: &str) -> Option<String> {
    files
        .read(path, &ProjectToolConfig::default())
        .ok()
        .map(|file| file.text)
}

/// The directory part of a root-relative path; empty for a root file.
pub(super) fn dir_of(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(dir, _)| dir.to_owned())
        .unwrap_or_default()
}

/// The path of `dir` below `base`, when it is below it.
pub(super) fn relative_to<'a>(dir: &'a str, base: &str) -> Option<&'a str> {
    if base.is_empty() {
        Some(dir)
    } else {
        dir.strip_prefix(base)?.strip_prefix('/')
    }
}

/// A matcher for workspace member patterns such as `crates/*`.
pub(super) fn members(patterns: &[String]) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    let mut any = false;
    for pattern in patterns {
        let trimmed = pattern
            .trim()
            .trim_start_matches("./")
            .trim_end_matches('/');
        if trimmed.is_empty() || trimmed.starts_with('!') {
            continue;
        }
        if let Ok(glob) = Glob::new(trimmed) {
            builder.add(glob);
            any = true;
        }
    }
    if any { builder.build().ok() } else { None }
}

/// One manifest directory before workspace members are folded into it.
pub(super) struct Entry {
    pub project: LanguageProject,
    /// Workspace member patterns, relative to `project.dir`.
    pub members: Option<GlobSet>,
}

/// Fold manifests that a workspace root lists as members into that root's
/// `packages`; every other manifest stays a project of its own. A member
/// without a name is listed by directory.
pub(super) fn fold(mut entries: Vec<Entry>) -> Vec<LanguageProject> {
    entries.sort_by(|a, b| a.project.dir.cmp(&b.project.dir));
    let mut kept: Vec<Entry> = Vec::new();
    for entry in entries {
        let label = entry
            .project
            .name
            .clone()
            .unwrap_or_else(|| entry.project.dir.clone());
        let owner = kept.iter().position(|root| {
            !entry.project.dir.is_empty()
                && root.project.dir != entry.project.dir
                && root.members.as_ref().is_some_and(|members| {
                    relative_to(&entry.project.dir, &root.project.dir)
                        .is_some_and(|rel| members.is_match(rel))
                })
        });
        match owner {
            Some(index) => kept[index].project.packages.push(label),
            None => kept.push(entry),
        }
    }
    kept.into_iter()
        .map(|mut entry| {
            entry.project.packages.sort();
            entry.project.packages.dedup();
            entry.project
        })
        .collect()
}

/// Files of a project that are worth reading: everything but lockfiles.
pub(super) fn key_files(detection: &LanguageDetection) -> Vec<String> {
    detection
        .projects
        .iter()
        .flat_map(|project| project.files.iter())
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            !(name.ends_with(".lock")
                || name.ends_with("-lock.json")
                || name.ends_with("-lock.yaml")
                || name == "bun.lockb"
                || name == "bun.lock"
                || name == "npm-shrinkwrap.json")
        })
        .cloned()
        .collect()
}

/// Cap on the projects one language reports.
pub(super) const MAX_PROJECTS: usize = 50;

/// Apply the project cap, reporting whether it cut anything.
pub(super) fn cap(
    mut projects: Vec<LanguageProject>,
    truncated: bool,
) -> (Vec<LanguageProject>, bool) {
    let over = projects.len() > MAX_PROJECTS;
    projects.truncate(MAX_PROJECTS);
    (projects, truncated || over)
}

/// Longest suggested label, matching the Operator's button limit.
pub(super) const MAX_LABEL_CHARS: usize = 24;
/// Candidates one language reports at most.
pub(super) const MAX_CANDIDATES: usize = 80;

/// `dir` joined with a file name; `dir` is empty for the root.
pub(super) fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// A label cut to [`MAX_LABEL_CHARS`] characters.
pub(super) fn label(text: &str) -> String {
    text.chars().take(MAX_LABEL_CHARS).collect()
}

/// The project that governs the manifest in `dir`: the one in that directory,
/// else the nearest ancestor project (a workspace root owns its members).
pub(super) fn owner<'a>(
    detection: &'a LanguageDetection,
    dir: &str,
) -> Option<&'a LanguageProject> {
    detection
        .projects
        .iter()
        .filter(|project| project.dir == dir || relative_to(dir, &project.dir).is_some())
        .max_by_key(|project| project.dir.len())
}

/// A name that is safe to put into a command line without quoting.
pub(super) fn is_plain_word(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '.' | '-' | '/' | '@'))
}

/// The 1-based number of the first line at or after `from` (1-based) for
/// which `matches` is true.
pub(super) fn line_where(text: &str, from: u32, matches: impl Fn(&str) -> bool) -> Option<u32> {
    text.lines()
        .enumerate()
        .skip(from.saturating_sub(1) as usize)
        .find(|(_, line)| matches(line))
        .map(|(index, _)| index as u32 + 1)
}

/// Whether `line` has `"key"` followed by a colon (a JSON object key).
fn has_json_key(line: &str, key: &str) -> bool {
    let needle = format!("\"{key}\"");
    line.match_indices(&needle)
        .any(|(at, _)| line[at + needle.len()..].trim_start().starts_with(':'))
}

/// The line of `"key"` inside the top-level object `"section"` of a JSON
/// file, found by text. `None` when the layout is not recognized.
pub(super) fn json_key_line(text: &str, section: &str, key: &str) -> Option<u32> {
    let start = line_where(text, 1, |line| has_json_key(line, section))?;
    line_where(text, start, |line| has_json_key(line, key))
}

/// The header line of the TOML table `[section]` (for example
/// `project.scripts`).
pub(super) fn toml_section_line(text: &str, section: &str) -> Option<u32> {
    line_where(text, 1, |line| header_of(line).as_deref() == Some(section))
}

fn header_of(line: &str) -> Option<String> {
    let line = line.trim();
    if line.starts_with("[[") {
        return None;
    }
    let inner = line.strip_prefix('[')?;
    let end = inner.find(']')?;
    Some(
        inner[..end]
            .split('.')
            .map(|part| part.trim().trim_matches('"'))
            .collect::<Vec<_>>()
            .join("."),
    )
}

/// The line of `key = ...` inside the TOML table `[section]`.
pub(super) fn toml_key_line(text: &str, section: &str, key: &str) -> Option<u32> {
    let mut inside = false;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            inside = header_of(line).as_deref() == Some(section);
            continue;
        }
        if inside {
            let name = trimmed.split('=').next().unwrap_or("").trim();
            if trimmed.contains('=') && name.trim_matches(['"', '\'']) == key {
                return Some(index as u32 + 1);
            }
        }
    }
    None
}

/// Evidence at `line` of `text` (the file at `path`), or the whole file when
/// the line is unknown.
pub(super) fn evidence(path: &str, text: &str, line: Option<u32>) -> CommandEvidence {
    let total = (text.lines().count() as u32).max(1);
    let (start_line, end_line) = match line {
        Some(line) if (1..=total).contains(&line) => (line, line),
        _ => (1, total),
    };
    CommandEvidence {
        path: path.to_owned(),
        start_line,
        end_line,
    }
}

/// A candidate with the fields every language fills the same way.
pub(super) fn candidate(
    language: LanguageId,
    dir: &str,
    command: impl Into<String>,
    kind: CommandKind,
    label_text: &str,
    evidence: CommandEvidence,
) -> CommandCandidate {
    CommandCandidate {
        command: command.into(),
        working_dir: dir.to_owned(),
        kind,
        language,
        label: label(label_text),
        evidence,
    }
}
