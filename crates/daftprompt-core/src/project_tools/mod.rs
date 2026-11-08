//! Read-only project tools: `list_files`, `search`, and `read_file`.
//!
//! The tools are independent of any agent. Layers:
//!
//! - [`ProjectFiles`]: a small backend trait. [`FsProject`] reads a configured
//!   project root and enforces every path rule; [`InMemoryProject`] is a test
//!   stub that needs no filesystem.
//! - [`ProjectToolbox`]: one per agent run. It owns the per-run call cap, the
//!   per-call result caps and truncation markers, the text the model sees, and
//!   a [`ToolRecorder`] ledger of every call and every line range returned by
//!   `read_file`. Errors become tool-result text; they never fail the run.
//! - [`rig_tools`]: thin Rig `PortableTool` adapters over a toolbox.

mod fs;
mod memory;
mod rig_adapter;
mod toolbox;

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub use fs::FsProject;
pub use memory::InMemoryProject;
pub use rig_adapter::{ListFilesTool, ReadFileTool, RigProjectTools, SearchTool, rig_tools};
pub use toolbox::{ProjectToolbox, ToolResponse};

/// Tool names, as seen by the model and recorded in [`RecordedCall::name`].
pub const LIST_FILES: &str = "list_files";
pub const SEARCH: &str = "search";
pub const READ_FILE: &str = "read_file";

/// Caps for project tools. All limits are enforced in Rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectToolConfig {
    /// Total tool calls per run (including rejected and failed ones).
    pub max_tool_calls: usize,
    /// Files returned by one `list_files` call.
    pub max_list_files: usize,
    /// Matching lines returned by one `search` call.
    pub max_search_matches: usize,
    /// Files opened by one `search` call before it stops.
    pub max_scan_files: usize,
    /// Lines returned by one `read_file` call.
    pub max_read_lines: usize,
    /// Output bytes of any one call (the cap on a `read_file` body, a listing,
    /// or a set of matches).
    pub max_output_bytes: usize,
    /// Bytes shown from one line; longer lines are cut with a marker.
    pub max_line_bytes: usize,
    /// Larger files are not read or searched.
    pub max_file_bytes: u64,
}

impl Default for ProjectToolConfig {
    fn default() -> Self {
        Self {
            max_tool_calls: 20,
            max_list_files: 200,
            max_search_matches: 50,
            max_scan_files: 5_000,
            max_read_lines: 300,
            max_output_bytes: 32 * 1024,
            max_line_bytes: 400,
            max_file_bytes: 1024 * 1024,
        }
    }
}

/// Why a tool call produced no result. Rendered to the model as
/// `error: <message>`; messages never contain absolute paths.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("invalid arguments: {0}")]
    InvalidArgument(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("access denied: {0}")]
    Denied(String),
    #[error("{0} is a binary file")]
    Binary(String),
    #[error("{0} is not valid UTF-8")]
    NotUtf8(String),
    #[error("{path} is larger than {limit} bytes")]
    TooLarge { path: String, limit: u64 },
    #[error("tool call limit reached ({0} calls); answer with what you have")]
    CallLimit(usize),
    #[error("io error: {0}")]
    Io(String),
}

/// Arguments of `list_files`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct ListFilesArgs {
    /// Only immediate files in the requested directory.
    #[serde(default)]
    pub shallow: bool,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub glob: Option<String>,
}

/// Arguments of `search`: a literal text search.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SearchArgs {
    pub query: String,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub glob: Option<String>,
    #[serde(default)]
    pub case_sensitive: Option<bool>,
}

/// Arguments of `read_file`. Lines are 1-based and inclusive.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ReadFileArgs {
    pub path: String,
    #[serde(default)]
    pub start_line: Option<usize>,
    #[serde(default)]
    pub end_line: Option<usize>,
}

/// Which files a `list_files` or `search` call looks at.
#[derive(Debug, Clone)]
pub struct Scope {
    pub(crate) shallow: bool,
    /// Directory relative to the root; `None` is the root.
    pub directory: Option<String>,
    glob: Option<globset::GlobMatcher>,
}

impl Scope {
    pub fn new(directory: Option<String>, glob: Option<&str>) -> Result<Self, ToolError> {
        let glob = match glob.filter(|g| !g.trim().is_empty()) {
            None => None,
            Some(pattern) => Some(
                globset::Glob::new(pattern)
                    .map_err(|e| ToolError::InvalidArgument(format!("bad glob: {e}")))?
                    .compile_matcher(),
            ),
        };
        let directory = directory.filter(|d| !d.trim().is_empty());
        Ok(Self {
            directory,
            glob,
            shallow: false,
        })
    }

    /// Whether the glob (if any) accepts this root-relative path. A glob
    /// without a `/` also matches the file name alone.
    pub fn glob_matches(&self, rel_path: &str) -> bool {
        if self.shallow {
            let dir = self
                .directory
                .as_deref()
                .map(normalize_path_text)
                .unwrap_or_default();
            let parent = rel_path
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .unwrap_or("");
            if parent != dir {
                return false;
            }
        }
        match &self.glob {
            None => true,
            Some(g) => {
                g.is_match(rel_path)
                    || rel_path
                        .rsplit('/')
                        .next()
                        .is_some_and(|name| g.is_match(name))
            }
        }
    }
}

/// A `search` request handed to a backend.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub paths: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: String,
    /// 1-based.
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchScan {
    pub hits: Vec<SearchHit>,
    /// The match cap or the scanned-file cap stopped the search early.
    pub truncated: bool,
}

/// A whole text file, root-relative path normalized to `/` separators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContent {
    pub path: String,
    pub text: String,
}

/// A read-only view of a project. Implementations enforce path rules; the
/// toolbox enforces output caps. Methods are synchronous and short.
pub trait ProjectFiles: Send + Sync {
    /// Visible files under the scope in path order, at most
    /// `config.max_list_files`.
    fn list(&self, scope: &Scope, config: &ProjectToolConfig) -> Result<Listing, ToolError>;
    /// Matching lines in path then line order, at most
    /// `config.max_search_matches`, scanning at most `config.max_scan_files`.
    fn search(
        &self,
        query: &SearchQuery,
        scope: &Scope,
        config: &ProjectToolConfig,
    ) -> Result<SearchScan, ToolError>;
    /// The full text of one file.
    fn read(&self, path: &str, config: &ProjectToolConfig) -> Result<FileContent, ToolError>;
}

/// Lines `start_line..=end_line` (1-based) that one `read_file` call returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRead {
    /// Normalized root-relative path.
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    /// SHA-256 (hex) of the file text the call read.
    pub file_sha256: String,
}

/// One tool invocation, as the model made it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedCall {
    /// 0-based order of completion.
    pub index: usize,
    pub name: String,
    /// The JSON arguments exactly as received.
    pub args: serde_json::Value,
    /// The text returned to the model, including any `error:` text and
    /// truncation markers.
    pub output: String,
    pub is_error: bool,
    pub truncated: bool,
    /// Set for a successful `read_file` that returned at least one line.
    pub read: Option<FileRead>,
    /// Commits a git tool returned to the model, with full hashes. Citations
    /// of commits are validated against these.
    #[serde(default)]
    pub commits: Vec<SeenCommit>,
}

/// A commit a git tool showed the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenCommit {
    /// Full lowercase hex commit id (40 or 64 characters).
    pub hash: String,
    pub subject: String,
}

/// Why a cited commit does not resolve against the ledger.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommitLookupError {
    #[error("`{0}` is not a commit hash of at least 7 hex characters")]
    Malformed(String),
    #[error("commit `{0}` was not returned by any git tool call in this run")]
    Unseen(String),
    #[error("commit prefix `{0}` matches {1} commits seen in this run; cite more characters")]
    Ambiguous(String, usize),
}

/// Shared per-run ledger. Clone it freely; all clones see the same calls.
#[derive(Debug, Clone, Default)]
pub struct ToolRecorder {
    calls: Arc<Mutex<Vec<RecordedCall>>>,
}

impl ToolRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&self, mut call: RecordedCall) {
        let mut calls = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        call.index = calls.len();
        calls.push(call);
    }

    /// Every call so far, in completion order.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Every line range returned by `read_file`, in call order.
    pub fn reads(&self) -> Vec<FileRead> {
        self.calls()
            .into_iter()
            .filter_map(|call| call.read)
            .collect()
    }

    /// Whether lines `start_line..=end_line` of `path` were all returned to
    /// the model, possibly across several `read_file` calls. `path` is
    /// compared after normalization (`./a//b` equals `a/b`).
    pub fn covers(&self, path: &str, start_line: usize, end_line: usize) -> bool {
        if start_line == 0 || end_line < start_line {
            return false;
        }
        let path = normalize_path_text(path);
        let mut ranges: Vec<(usize, usize)> = self
            .reads()
            .into_iter()
            .filter(|r| r.path == path)
            .map(|r| (r.start_line, r.end_line))
            .collect();
        ranges.sort_unstable();
        let mut next = start_line;
        for (s, e) in ranges {
            if s > next {
                break;
            }
            if e >= next {
                next = e + 1;
            }
            if next > end_line {
                return true;
            }
        }
        false
    }

    /// Every distinct commit returned by git tools, in the order first seen.
    pub fn commits(&self) -> Vec<SeenCommit> {
        let mut seen: Vec<SeenCommit> = Vec::new();
        for commit in self.calls().into_iter().flat_map(|call| call.commits) {
            if !seen.iter().any(|known| known.hash == commit.hash) {
                seen.push(commit);
            }
        }
        seen
    }

    /// Resolve a cited hash or prefix (at least 7 hex characters,
    /// case-insensitive) to exactly one commit the git tools returned.
    pub fn resolve_commit(&self, cited: &str) -> Result<SeenCommit, CommitLookupError> {
        let prefix = cited.trim().to_ascii_lowercase();
        if prefix.len() < 7 || prefix.len() > 64 || !prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CommitLookupError::Malformed(cited.to_owned()));
        }
        let mut matches: Vec<SeenCommit> = self
            .commits()
            .into_iter()
            .filter(|commit| commit.hash.starts_with(&prefix))
            .collect();
        match matches.len() {
            0 => Err(CommitLookupError::Unseen(cited.to_owned())),
            1 => Ok(matches.remove(0)),
            n => Err(CommitLookupError::Ambiguous(cited.to_owned(), n)),
        }
    }

    /// The hash of the file text last read at `path`, if any.
    pub fn file_sha256(&self, path: &str) -> Option<String> {
        let path = normalize_path_text(path);
        self.reads()
            .into_iter()
            .rev()
            .find(|r| r.path == path)
            .map(|r| r.file_sha256)
    }
}

/// Lexical normalization: drop `.` and empty components, use `/`.
pub(crate) fn normalize_path_text(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Lexical checks shared by every backend. Returns the normalized relative
/// path (empty for the root). Rejects absolute paths, `..`, NUL bytes, hidden
/// components, and `.env` files.
pub(crate) fn check_relative_path(path: &str) -> Result<String, ToolError> {
    if path.contains('\0') {
        return Err(ToolError::InvalidArgument("path contains NUL".into()));
    }
    let std_path = std::path::Path::new(path);
    if std_path.is_absolute()
        || path.starts_with('/')
        || path.starts_with('\\')
        || std_path
            .components()
            .any(|c| matches!(c, std::path::Component::Prefix(_)))
    {
        return Err(ToolError::Denied(format!(
            "{path}: absolute paths are not allowed; use a path relative to the project root"
        )));
    }
    let normalized = normalize_path_text(path);
    check_components(path, &normalized)?;
    Ok(normalized)
}

/// Denies `..`, `.env*`, and hidden components of a normalized path.
pub(crate) fn check_components(shown: &str, normalized: &str) -> Result<(), ToolError> {
    for component in normalized.split('/').filter(|c| !c.is_empty()) {
        if component == ".." {
            return Err(ToolError::Denied(format!(
                "{shown}: paths must stay inside the project root"
            )));
        }
        if component == ".env" || component.starts_with(".env.") {
            return Err(ToolError::Denied(format!(
                "{shown}: environment files are never readable"
            )));
        }
        if component.starts_with('.') {
            return Err(ToolError::Denied(format!(
                "{shown}: hidden files and directories are not available"
            )));
        }
    }
    Ok(())
}

/// Cuts `text` to at most `max` bytes on a char boundary. The flag reports
/// whether anything was removed.
pub(crate) fn cut_to_bytes(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

/// Splits file text into lines: `\n` or `\r\n` terminated, no trailing empty
/// line, so an empty file has zero lines.
pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    if text.is_empty() || text.ends_with('\n') {
        lines.pop();
    }
    lines
}
