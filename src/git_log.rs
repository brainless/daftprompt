use std::path::Path;

use gix::bstr::ByteSlice;
use gix::date::time::format;

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub short_hash: String,
    pub author_name: String,
    pub time: String,
    pub message_title: String,
}

#[derive(Debug)]
pub enum GitLogError {
    Discovery(gix::discover::Error),
    RevisionWalk(gix::revision::walk::Error),
    RevisionIter(gix::revision::walk::iter::Error),
    Object(gix::object::find::existing::Error),
    Decode(Box<dyn std::error::Error + Send + Sync>),
    HeadNotFound,
}

impl std::fmt::Display for GitLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitLogError::Discovery(e) => write!(f, "Failed to discover repository: {}", e),
            GitLogError::RevisionWalk(e) => write!(f, "Failed to create revision walk: {}", e),
            GitLogError::RevisionIter(e) => write!(f, "Failed during revision walk: {}", e),
            GitLogError::Object(e) => write!(f, "Failed to read object: {}", e),
            GitLogError::Decode(e) => write!(f, "Failed to decode commit: {}", e),
            GitLogError::HeadNotFound => write!(f, "HEAD reference not found"),
        }
    }
}

impl std::error::Error for GitLogError {}

impl From<gix::discover::Error> for GitLogError {
    fn from(e: gix::discover::Error) -> Self {
        GitLogError::Discovery(e)
    }
}

impl From<gix::revision::walk::Error> for GitLogError {
    fn from(e: gix::revision::walk::Error) -> Self {
        GitLogError::RevisionWalk(e)
    }
}

impl From<gix::revision::walk::iter::Error> for GitLogError {
    fn from(e: gix::revision::walk::iter::Error) -> Self {
        GitLogError::RevisionIter(e)
    }
}

impl From<gix::object::find::existing::Error> for GitLogError {
    fn from(e: gix::object::find::existing::Error) -> Self {
        GitLogError::Object(e)
    }
}

pub fn read_log(repo_path: &Path) -> Result<Vec<CommitInfo>, GitLogError> {
    let repo = gix::discover(repo_path)?;

    let head = repo
        .rev_parse_single("HEAD")
        .map_err(|_| GitLogError::HeadNotFound)?
        .object()?
        .try_into_commit()
        .map_err(|e| GitLogError::Decode(Box::new(e)))?;

    let mut commits = Vec::new();
    let walk = repo
        .rev_walk([head.id()])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            Default::default(),
        ))
        .all()?;

    for info in walk {
        let info = info?;

        let commit = match info.object() {
            Ok(c) => c,
            Err(_) => continue,
        };

        let commit_ref = match commit.decode() {
            Ok(r) => r,
            Err(_) => continue,
        };

        let author = match commit_ref.author() {
            Ok(a) => a,
            Err(_) => continue,
        };

        let actor = author.actor();
        let short_hash = commit.id().shorten_or_id().to_string();
        let author_name = actor.name.to_string();
        let time = author
            .time()
            .map(|t| t.format_or_unix(format::DEFAULT))
            .unwrap_or_default();
        let message_title = commit_ref
            .message()
            .title
            .to_str_lossy()
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        commits.push(CommitInfo {
            short_hash,
            author_name,
            time,
            message_title,
        });
    }

    Ok(commits)
}
