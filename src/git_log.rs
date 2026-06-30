use std::path::Path;

use gix::bstr::ByteSlice;
use gix::date::time::format;

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub short_hash: String,
    pub author_name: String,
    pub time: String,
    pub message_title: String,
    pub message_body: String,
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

fn extract_commit_info(commit: &gix::Commit) -> Option<CommitInfo> {
    let commit_ref = commit.decode().ok()?;
    let author = commit_ref.author().ok()?;
    let actor = author.actor();

    let sha = commit.id().to_string();
    let short_hash = commit.id().shorten_or_id().to_string();
    let author_name = actor.name.to_string();
    let time = author
        .time()
        .map(|t| t.format_or_unix(format::DEFAULT))
        .unwrap_or_default();

    let message = commit_ref.message();
    let message_title = message
        .title
        .to_str_lossy()
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    let message_body = message
        .body
        .map(|b| b.to_str_lossy().to_string())
        .unwrap_or_default();

    Some(CommitInfo {
        sha,
        short_hash,
        author_name,
        time,
        message_title,
        message_body,
    })
}

fn walk_commits(repo: &gix::Repository, tips: Vec<gix::hash::ObjectId>) -> Result<Vec<CommitInfo>, GitLogError> {
    let mut commits = Vec::new();
    let walk = repo
        .rev_walk(tips)
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

        if let Some(ci) = extract_commit_info(&commit) {
            commits.push(ci);
        }
    }

    Ok(commits)
}

pub fn read_log(repo_path: &Path) -> Result<Vec<CommitInfo>, GitLogError> {
    let repo = gix::discover(repo_path)?;

    let head = repo
        .rev_parse_single("HEAD")
        .map_err(|_| GitLogError::HeadNotFound)?
        .object()?
        .try_into_commit()
        .map_err(|e| GitLogError::Decode(Box::new(e)))?;

    walk_commits(&repo, vec![head.id().into()])
}

pub fn read_log_all_branches(repo_path: &Path) -> Result<Vec<CommitInfo>, GitLogError> {
    let repo = gix::discover(repo_path)?;

    let branch_heads: Vec<gix::hash::ObjectId> = repo
        .references()
        .map_err(|e| GitLogError::Decode(Box::new(e)))?
        .local_branches()
        .map_err(|e| GitLogError::Decode(Box::new(e)))?
        .peeled()
        .map_err(|e| GitLogError::Decode(Box::new(e)))?
        .filter_map(|r| r.ok())
        .filter_map(|r| r.try_id().map(|id| id.detach()))
        .collect();

    if branch_heads.is_empty() {
        let head = repo
            .rev_parse_single("HEAD")
            .map_err(|_| GitLogError::HeadNotFound)?
            .object()?
            .try_into_commit()
            .map_err(|e| GitLogError::Decode(Box::new(e)))?;
        walk_commits(&repo, vec![head.id().into()])
    } else {
        walk_commits(&repo, branch_heads)
    }
}
