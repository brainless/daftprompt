//! Read-only git history tools: `git_log` and `git_show`.
//!
//! The tools are independent of any agent, like [`crate::project_tools`].
//! Layers:
//!
//! - [`GitHistory`]: a small backend trait taking only validated, typed
//!   queries ([`LogQuery`], [`ShowQuery`]). [`SubprocessGit`] runs the `git`
//!   binary with a hardened command line; [`InMemoryGit`] is a test double.
//! - [`GitToolbox`]: one per agent run. It owns the per-run call cap, the
//!   result caps and truncation markers, the text the model sees, and the
//!   shared [`ToolRecorder`] ledger, which also records every commit hash a
//!   call returned so Rust can validate commit citations. Errors become
//!   tool-result text; they never fail the run.
//! - [`git_rig_tools`]: thin Rig `PortableTool` adapters over a toolbox.
//!
//! Not being in a repository, or `git` being missing, is reported to the model
//! and exposed through [`GitToolbox::problem`]; it is not an error.

mod memory;
mod rig_adapter;
mod subprocess;
mod toolbox;

use std::time::Duration;

pub use memory::{InMemoryGit, MemoryCommit};
pub use rig_adapter::{GitLogTool, GitShowTool, RigGitTools, git_rig_tools};
pub use subprocess::SubprocessGit;
pub use toolbox::{GitProblem, GitToolbox};

use crate::project_tools::check_relative_path;

/// Tool names, as seen by the model and recorded in `RecordedCall::name`.
pub const GIT_LOG: &str = "git_log";
pub const GIT_SHOW: &str = "git_show";

/// Caps for the git tools. All limits are enforced in Rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitToolConfig {
    /// Total tool calls per run (including rejected and failed ones).
    pub max_tool_calls: usize,
    /// Commits `git_log` returns when `count` is absent.
    pub default_log_commits: usize,
    /// Upper bound of `count`; larger values are lowered to it.
    pub max_log_commits: usize,
    /// Bytes of one commit body shown by `git_log`.
    pub max_log_body_bytes: usize,
    /// Bytes of the patch `git_show` returns when a patch is requested.
    pub max_patch_bytes: usize,
    /// Output bytes of any one call.
    pub max_output_bytes: usize,
    /// Wall-clock limit of one `git` process.
    pub timeout: Duration,
    /// Bytes read from one `git` process; the process is stopped beyond this.
    pub max_process_bytes: usize,
}

impl Default for GitToolConfig {
    fn default() -> Self {
        Self {
            max_tool_calls: 12,
            default_log_commits: 20,
            max_log_commits: 50,
            max_log_body_bytes: 600,
            max_patch_bytes: 16 * 1024,
            max_output_bytes: 32 * 1024,
            timeout: Duration::from_secs(10),
            max_process_bytes: 1024 * 1024,
        }
    }
}

/// Why a git call produced no result. Rendered to the model as
/// `error: <message>`; messages never contain absolute paths.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitError {
    #[error("invalid arguments: {0}")]
    InvalidArgument(String),
    #[error("this project is not in a git repository, so there is no history to read")]
    NotARepository,
    #[error("git is not available on this machine: {0}")]
    Unavailable(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("git did not finish within {0:?}")]
    Timeout(Duration),
    #[error("git failed: {0}")]
    Failed(String),
}

/// A validated `git_log` query.
///
/// The fields are private: the only way to build one is [`LogQuery::new`],
/// which is where every argument rule lives. Backends therefore never see an
/// unvalidated value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogQuery {
    count: usize,
    since: Option<String>,
    pathspec: Option<String>,
}

impl LogQuery {
    /// Validate the model's arguments.
    ///
    /// - `count`: positive; absent means the default; above the maximum it is
    ///   lowered to it.
    /// - `since`: a short date expression such as `2 weeks ago` or
    ///   `2026-10-01`; it never starts with `-`.
    /// - `path`: a root-relative path with no `..`, no absolute form, no
    ///   hidden or `.env` component, and no leading `-` or `:` (pathspec magic).
    pub fn new(
        count: Option<usize>,
        since: Option<&str>,
        path: Option<&str>,
        config: &GitToolConfig,
    ) -> Result<Self, GitError> {
        let count = match count {
            None => config.default_log_commits,
            Some(0) => {
                return Err(GitError::InvalidArgument("count must be at least 1".into()));
            }
            Some(n) => n,
        }
        .min(config.max_log_commits.max(1));
        let since = match since.map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(value) => Some(validate_since(value)?),
        };
        let pathspec = match path.map(str::trim).filter(|p| !p.is_empty()) {
            None => None,
            Some(value) => validate_pathspec(value)?,
        };
        Ok(Self {
            count,
            since,
            pathspec,
        })
    }

    /// Maximum commits to return (at least 1).
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn since(&self) -> Option<&str> {
        self.since.as_deref()
    }

    /// Normalized root-relative path; `None` means the whole root.
    pub fn pathspec(&self) -> Option<&str> {
        self.pathspec.as_deref()
    }
}

/// A validated `git_show` query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowQuery {
    hash: String,
    patch: bool,
}

impl ShowQuery {
    /// `hash` must be 7 to 64 hex characters (a full id or an abbreviation);
    /// it is lowercased.
    pub fn new(hash: &str, patch: bool) -> Result<Self, GitError> {
        let hash = hash.trim().to_ascii_lowercase();
        if hash.len() < 7 || hash.len() > 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(GitError::InvalidArgument(
                "hash must be 7 to 64 hexadecimal characters".into(),
            ));
        }
        Ok(Self { hash, patch })
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// Whether the diff itself is wanted, not only the stat.
    pub fn patch(&self) -> bool {
        self.patch
    }
}

fn validate_since(value: &str) -> Result<String, GitError> {
    // The value becomes the single argument `--since=<value>`. A leading `-`
    // is refused anyway, and the character set excludes anything but dates.
    let allowed = |c: char| c.is_ascii_alphanumeric() || " :.+-/_,".contains(c);
    if value.len() > 64 || value.starts_with('-') || !value.chars().all(allowed) {
        return Err(GitError::InvalidArgument(
            "since must be a short date such as `2 weeks ago` or `2026-10-01`".into(),
        ));
    }
    Ok(value.to_owned())
}

fn validate_pathspec(value: &str) -> Result<Option<String>, GitError> {
    if value.len() > 256 || value.chars().any(char::is_control) {
        return Err(GitError::InvalidArgument("path is not a plain path".into()));
    }
    // Same rules as the file tools: no absolute paths, `..`, hidden
    // components, or `.env` files.
    let normalized = check_relative_path(value).map_err(|error| {
        GitError::InvalidArgument(error.to_string().replace("access denied: ", ""))
    })?;
    // Option-like or magic pathspecs (`:(top)`) could widen the scope.
    if normalized.starts_with('-') || normalized.starts_with(':') {
        return Err(GitError::InvalidArgument(
            "path must not start with `-` or `:`".into(),
        ));
    }
    Ok((!normalized.is_empty()).then_some(normalized))
}

/// One commit as `git` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommit {
    /// Full lowercase hex commit id (40 or 64 characters).
    pub hash: String,
    pub author: String,
    /// Author date, strict ISO 8601.
    pub date: String,
    pub subject: String,
    /// The message after the subject; empty when there is none.
    pub body: String,
}

/// The result of a log query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLog {
    /// Newest first, at most `query.count()`.
    pub commits: Vec<GitCommit>,
    /// More matching history exists than was returned.
    pub more: bool,
}

/// The result of a show query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitShow {
    pub commit: GitCommit,
    /// `--stat` of the commit, limited to the project root.
    pub stat: String,
    /// The diff, when requested.
    pub patch: Option<String>,
    /// `git`'s output was cut at the process byte cap.
    pub truncated: bool,
}

/// A read-only view of a project's git history. Implementations enforce
/// process limits; the toolbox enforces output caps. Methods are synchronous.
pub trait GitHistory: Send + Sync {
    /// Commits for the query, newest first. A repository without commits is
    /// an empty log, not an error.
    fn log(&self, query: &LogQuery) -> Result<GitLog, GitError>;
    /// One commit's message and stat (and patch when asked).
    fn show(&self, query: &ShowQuery) -> Result<GitShow, GitError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> GitToolConfig {
        GitToolConfig::default()
    }

    #[test]
    fn log_query_defaults_and_clamps_the_count() {
        let query = LogQuery::new(None, None, None, &config()).unwrap();
        assert_eq!(query.count(), 20);
        assert_eq!(query.since(), None);
        assert_eq!(query.pathspec(), None);
        let query = LogQuery::new(Some(5000), None, None, &config()).unwrap();
        assert_eq!(query.count(), 50);
        assert!(LogQuery::new(Some(0), None, None, &config()).is_err());
    }

    #[test]
    fn since_accepts_dates_and_rejects_options() {
        for ok in ["2 weeks ago", "2026-10-01", "2026-10-01T12:00:00+02:00"] {
            let query = LogQuery::new(None, Some(ok), None, &config()).unwrap();
            assert_eq!(query.since(), Some(ok));
        }
        for bad in [
            "--output=/tmp/x",
            "-1",
            "$(id)",
            "a;b",
            "x\ny",
            &"9".repeat(65),
        ] {
            assert!(
                LogQuery::new(None, Some(bad), None, &config()).is_err(),
                "{bad:?}"
            );
        }
        // Blank means absent.
        assert_eq!(
            LogQuery::new(None, Some("  "), None, &config())
                .unwrap()
                .since(),
            None
        );
    }

    #[test]
    fn pathspec_stays_inside_the_root() {
        let query = LogQuery::new(None, None, Some("./src//auth"), &config()).unwrap();
        assert_eq!(query.pathspec(), Some("src/auth"));
        for bad in [
            "../etc",
            "/etc/passwd",
            "-p",
            ":(top)secret",
            ":!src",
            ".git/config",
            ".env",
            "a\0b",
            "a\nb",
        ] {
            assert!(
                LogQuery::new(None, None, Some(bad), &config()).is_err(),
                "{bad:?}"
            );
        }
        // The root itself is no restriction.
        assert_eq!(
            LogQuery::new(None, None, Some("."), &config())
                .unwrap()
                .pathspec(),
            None
        );
    }

    #[test]
    fn show_hashes_must_be_hex() {
        assert_eq!(ShowQuery::new("ABCDEF1", false).unwrap().hash(), "abcdef1");
        for bad in [
            "abc",
            "HEAD",
            "main",
            "abcdef1; rm",
            "--stat",
            "abcdefg",
            "",
        ] {
            assert!(ShowQuery::new(bad, false).is_err(), "{bad:?}");
        }
        assert!(ShowQuery::new(&"a".repeat(65), false).is_err());
    }
}
