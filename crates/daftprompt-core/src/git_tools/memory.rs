use super::{GitCommit, GitError, GitHistory, GitLog, GitShow, LogQuery, ShowQuery};

/// A commit of the in-memory double, with what `git_show` reports for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryCommit {
    pub commit: GitCommit,
    /// Root-relative files the commit touched; a log `path` keeps commits
    /// with a file under it.
    pub files: Vec<String>,
    pub stat: String,
    pub patch: String,
}

impl MemoryCommit {
    /// A commit with no body, files, stat, or patch.
    pub fn new(hash: &str, subject: &str) -> Self {
        Self {
            commit: GitCommit {
                hash: hash.to_owned(),
                author: "Ada".to_owned(),
                date: "2026-10-01T12:00:00+00:00".to_owned(),
                subject: subject.to_owned(),
                body: String::new(),
            },
            files: Vec::new(),
            stat: String::new(),
            patch: String::new(),
        }
    }
}

/// A test double for [`GitHistory`]: a fixed list of commits, newest first,
/// or a fixed failure. `since` is accepted but not interpreted.
#[derive(Debug, Clone, Default)]
pub struct InMemoryGit {
    commits: Vec<MemoryCommit>,
    failure: Option<GitError>,
}

impl InMemoryGit {
    /// A repository with `commits`, newest first.
    pub fn new(commits: Vec<MemoryCommit>) -> Self {
        Self {
            commits,
            failure: None,
        }
    }

    /// A folder that is not a repository.
    pub fn not_a_repository() -> Self {
        Self {
            commits: Vec::new(),
            failure: Some(GitError::NotARepository),
        }
    }

    /// A machine without `git`.
    pub fn unavailable() -> Self {
        Self {
            commits: Vec::new(),
            failure: Some(GitError::Unavailable("git: command not found".to_owned())),
        }
    }
}

impl GitHistory for InMemoryGit {
    fn log(&self, query: &LogQuery) -> Result<GitLog, GitError> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let mut matching = self.commits.iter().filter(|entry| {
            query.pathspec().is_none_or(|prefix| {
                entry
                    .files
                    .iter()
                    .any(|file| file == prefix || file.starts_with(&format!("{prefix}/")))
            })
        });
        let commits: Vec<GitCommit> = matching
            .by_ref()
            .take(query.count())
            .map(|entry| entry.commit.clone())
            .collect();
        Ok(GitLog {
            commits,
            more: matching.next().is_some(),
        })
    }

    fn show(&self, query: &ShowQuery) -> Result<GitShow, GitError> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let mut found = self
            .commits
            .iter()
            .filter(|entry| entry.commit.hash.starts_with(query.hash()));
        let Some(entry) = found.next() else {
            return Err(GitError::NotFound(format!(
                "no commit matches {}",
                query.hash()
            )));
        };
        if found.next().is_some() {
            return Err(GitError::Failed(format!(
                "{} is ambiguous; use more characters",
                query.hash()
            )));
        }
        Ok(GitShow {
            commit: entry.commit.clone(),
            stat: entry.stat.clone(),
            patch: query.patch().then(|| entry.patch.clone()),
            truncated: false,
        })
    }
}
