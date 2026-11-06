//! Git tool tests: the subprocess backend against a temporary repository with
//! a fixed author and dates, the toolbox over an in-memory double, and the
//! non-repository, missing-git and subdirectory behaviours. No credentials.
//! Subprocess tests skip quietly when `git` is not installed.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use daftprompt_core::git_tools::{
    GIT_LOG, GIT_SHOW, GitError, GitHistory, GitProblem, GitToolConfig, GitToolbox, InMemoryGit,
    LogQuery, MemoryCommit, ShowQuery, SubprocessGit,
};
use serde_json::json;

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn git(dir: &Path, date: &str, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_AUTHOR_NAME", "Ada Lovelace")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_NAME", "Ada Lovelace")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `a/one.txt` and `b/two.txt` over three commits:
/// 1 touches `a`, 2 touches `b`, 3 touches `a` (with a body).
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, "2026-10-01T10:00:00+00:00", &["init", "-q"]);
    std::fs::create_dir_all(root.join("a")).unwrap();
    std::fs::create_dir_all(root.join("b")).unwrap();

    std::fs::write(root.join("a/one.txt"), "one\n").unwrap();
    git(root, "2026-10-01T10:00:00+00:00", &["add", "."]);
    git(
        root,
        "2026-10-01T10:00:00+00:00",
        &["commit", "-q", "-m", "Add a"],
    );

    std::fs::write(root.join("b/two.txt"), "two\n").unwrap();
    git(root, "2026-10-02T10:00:00+00:00", &["add", "."]);
    git(
        root,
        "2026-10-02T10:00:00+00:00",
        &["commit", "-q", "-m", "Add b"],
    );

    std::fs::write(root.join("a/one.txt"), "one\nmore\n").unwrap();
    git(root, "2026-10-03T10:00:00+00:00", &["add", "."]);
    git(
        root,
        "2026-10-03T10:00:00+00:00",
        &[
            "commit",
            "-q",
            "-m",
            "Extend a",
            "-m",
            "Because it needed more.",
        ],
    );
    dir
}

fn config() -> GitToolConfig {
    GitToolConfig::default()
}

fn log(
    history: &dyn GitHistory,
    path: Option<&str>,
) -> Result<daftprompt_core::git_tools::GitLog, GitError> {
    history.log(&LogQuery::new(None, None, path, &config()).unwrap())
}

#[test]
fn the_subprocess_reads_a_temporary_repository() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let history = SubprocessGit::new(dir.path(), &config());

    let all = log(&history, None).unwrap();
    let subjects: Vec<&str> = all.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, ["Extend a", "Add b", "Add a"]);
    assert!(!all.more);
    let newest = &all.commits[0];
    assert_eq!(newest.hash.len(), 40);
    assert_eq!(newest.author, "Ada Lovelace");
    assert!(
        newest.date.starts_with("2026-10-03T10:00:00"),
        "{}",
        newest.date
    );
    assert_eq!(newest.body, "Because it needed more.");

    // A bounded count reports that more history exists.
    let two = history
        .log(&LogQuery::new(Some(2), None, None, &config()).unwrap())
        .unwrap();
    assert_eq!(two.commits.len(), 2);
    assert!(two.more);

    // `since` and `path` narrow the history.
    let since = history
        .log(&LogQuery::new(None, Some("2026-10-02T00:00:00"), None, &config()).unwrap())
        .unwrap();
    assert_eq!(since.commits.len(), 2);
    let only_a = log(&history, Some("a")).unwrap();
    assert_eq!(only_a.commits.len(), 2);

    // `show` gives the message and the stat, and a patch on request.
    let show = history
        .show(&ShowQuery::new(&newest.hash[..8], false).unwrap())
        .unwrap();
    assert_eq!(show.commit.hash, newest.hash);
    assert!(show.stat.contains("a/one.txt"), "{}", show.stat);
    assert_eq!(show.patch, None);
    let show = history
        .show(&ShowQuery::new(&newest.hash, true).unwrap())
        .unwrap();
    let patch = show.patch.expect("a patch");
    assert!(patch.starts_with("diff --git"), "{patch}");
    assert!(patch.contains("+more"));

    // An unknown commit is "not found", not a crash.
    let missing = history.show(&ShowQuery::new("0123456789abcdef", false).unwrap());
    assert!(matches!(missing, Err(GitError::NotFound(_))), "{missing:?}");
}

#[test]
fn a_repository_without_commits_has_an_empty_log() {
    if !git_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), "2026-10-01T10:00:00+00:00", &["init", "-q"]);
    let history = SubprocessGit::new(dir.path(), &config());
    let empty = log(&history, None).unwrap();
    assert!(empty.commits.is_empty());
    assert!(!empty.more);
}

#[test]
fn a_folder_outside_any_repository_is_reported_not_failed() {
    if !git_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let history = SubprocessGit::new(dir.path(), &config());
    assert_eq!(log(&history, None), Err(GitError::NotARepository));
    assert_eq!(
        history.show(&ShowQuery::new("abcdef1", false).unwrap()),
        Err(GitError::NotARepository)
    );

    // Through the toolbox the model sees text and the run records the problem.
    let toolbox = GitToolbox::new(Arc::new(history), config());
    let response = toolbox.call(GIT_LOG, json!({}));
    assert!(response.is_error);
    assert!(
        response.text.contains("not in a git repository"),
        "{}",
        response.text
    );
    assert!(
        !response.text.contains(dir.path().to_str().unwrap()),
        "no absolute path in tool text"
    );
    assert_eq!(toolbox.problem(), Some(GitProblem::NotARepository));
}

#[test]
fn a_missing_git_binary_is_reported_not_failed() {
    let dir = tempfile::tempdir().unwrap();
    let history =
        SubprocessGit::new(dir.path(), &config()).with_program("daftprompt-no-such-git-binary");
    assert!(matches!(log(&history, None), Err(GitError::Unavailable(_))));
    let toolbox = GitToolbox::new(Arc::new(history), config());
    let response = toolbox.call(GIT_LOG, json!({"count": 3}));
    assert!(response.is_error);
    assert!(matches!(
        toolbox.problem(),
        Some(GitProblem::Unavailable(_))
    ));
}

#[test]
fn a_subdirectory_root_sees_only_its_own_history() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let history = SubprocessGit::new(dir.path().join("a"), &config());

    let scoped = log(&history, None).unwrap();
    let subjects: Vec<&str> = scoped.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["Extend a", "Add a"],
        "commit `Add b` is outside the root"
    );

    // The stat uses root-relative paths and leaves out files outside the root.
    let show = history
        .show(&ShowQuery::new(&scoped.commits[0].hash, false).unwrap())
        .unwrap();
    assert!(show.stat.contains("one.txt"), "{}", show.stat);
    assert!(!show.stat.contains("a/one.txt"), "{}", show.stat);

    // A commit that touched nothing under the root is outside its history.
    let all = log(&SubprocessGit::new(dir.path(), &config()), None).unwrap();
    let add_b = all.commits.iter().find(|c| c.subject == "Add b").unwrap();
    let outside = history.show(&ShowQuery::new(&add_b.hash, true).unwrap());
    assert!(matches!(outside, Err(GitError::NotFound(_))), "{outside:?}");

    // `path` is relative to the root, so `b` does not exist from here.
    assert!(log(&history, Some("b")).unwrap().commits.is_empty());
}

#[test]
fn the_toolbox_rejects_bad_arguments_and_runs_no_git() {
    // A double that fails every call proves validation happens first.
    let toolbox = GitToolbox::new(Arc::new(InMemoryGit::unavailable()), config());
    for (tool, args) in [
        (GIT_LOG, json!({"since": "--output=/tmp/x"})),
        (GIT_LOG, json!({"path": "../outside"})),
        (GIT_LOG, json!({"path": "/etc/passwd"})),
        (GIT_LOG, json!({"count": 0})),
        (GIT_SHOW, json!({"hash": "HEAD"})),
        (GIT_SHOW, json!({"hash": "--stat"})),
        (GIT_SHOW, json!({})),
        ("git_push", json!({})),
    ] {
        let response = toolbox.call(tool, args.clone());
        assert!(response.is_error, "{tool} {args}");
        assert!(response.text.starts_with("error: "), "{}", response.text);
    }
    assert_eq!(
        toolbox.problem(),
        None,
        "rejected arguments are not a git problem"
    );
}

#[test]
fn the_ledger_records_returned_commits_and_the_call_cap_applies() {
    let hash_a = "a".repeat(40);
    let hash_b = format!("{}{}", "ab", "c".repeat(38));
    let history = InMemoryGit::new(vec![
        MemoryCommit::new(&hash_a, "Newest"),
        MemoryCommit::new(&hash_b, "Older"),
    ]);
    let config = GitToolConfig {
        max_tool_calls: 3,
        ..GitToolConfig::default()
    };
    let toolbox = GitToolbox::new(Arc::new(history), config);

    let response = toolbox.call(GIT_LOG, json!({"count": 1}));
    assert!(!response.is_error);
    assert!(response.text.contains(&hash_a));
    assert!(response.text.contains("[truncated"), "{}", response.text);
    // Only the commit that was shown is citable.
    assert_eq!(toolbox.recorder().commits().len(), 1);
    assert!(toolbox.recorder().resolve_commit(&hash_a[..7]).is_ok());
    assert!(toolbox.recorder().resolve_commit(&hash_b[..7]).is_err());

    toolbox.call(GIT_SHOW, json!({"hash": &hash_b[..8], "patch": true}));
    assert_eq!(toolbox.recorder().commits().len(), 2);

    toolbox.call(GIT_LOG, json!({}));
    let over = toolbox.call(GIT_LOG, json!({}));
    assert!(over.is_error);
    assert!(over.text.contains("limit"), "{}", over.text);
    assert_eq!(toolbox.recorder().call_count(), 4);
}
