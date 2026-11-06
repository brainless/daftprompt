//! The `git` subprocess backend.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{GitCommit, GitError, GitHistory, GitLog, GitShow, GitToolConfig, LogQuery, ShowQuery};

/// Field and record separators of the `--format` strings. A record ends at a
/// NUL byte (git refuses NUL in commit messages) and fields at 0x1f.
const FIELD: char = '\x1f';
const FORMAT: &str = "--format=%H%x1f%an%x1f%aI%x1f%s%x1f%b%x00";

/// Environment variables that could redirect git to another repository, add
/// configuration, or run an external program. They are removed.
const SCRUBBED_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_EXTERNAL_DIFF",
    "GIT_PAGER",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_ASKPASS",
    "PAGER",
];

/// Runs the `git` binary read-only against one project root.
///
/// Every invocation is `git -C <root> --no-pager ...` with fixed hardening
/// (see [`global_args`]), arguments built only from validated queries, no
/// shell, a wall-clock timeout, and a cap on the bytes read.
#[derive(Debug, Clone)]
pub struct SubprocessGit {
    root: PathBuf,
    program: PathBuf,
    timeout: Duration,
    max_bytes: usize,
}

impl SubprocessGit {
    /// A backend over `root` with the limits of `config`.
    pub fn new(root: impl AsRef<Path>, config: &GitToolConfig) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            program: PathBuf::from("git"),
            timeout: config.timeout,
            max_bytes: config.max_process_bytes,
        }
    }

    /// Use another executable instead of `git` (for tests of the
    /// git-is-missing path).
    pub fn with_program(mut self, program: impl Into<PathBuf>) -> Self {
        self.program = program.into();
        self
    }

    fn run(&self, args: &[String]) -> Result<Output, GitError> {
        let mut command = Command::new(&self.program);
        // The root is the value of -C, so it can never be read as an option.
        command.arg("-C").arg(&self.root).args(global_args());
        command.args(args);
        // No shell is involved and nothing may prompt or wait for input.
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in SCRUBBED_ENV {
            command.env_remove(name);
        }
        // Never prompt for credentials, never take the index lock (reads must
        // not disturb a concurrent git), ignore system-wide configuration, and
        // keep stderr in English so failures can be classified.
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LC_ALL", "C");

        let mut child = command
            .spawn()
            .map_err(|error| GitError::Unavailable(error.to_string()))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let max_bytes = self.max_bytes;
        // Readers cap what they keep. A reader that stops early closes the
        // pipe, which ends a git process that would write far more.
        let out_reader = std::thread::spawn(move || read_capped(stdout, max_bytes));
        let err_reader = std::thread::spawn(move || read_capped(stderr, 8 * 1024));

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() >= deadline => {
                    // A hung git (a slow filesystem, a lock) is killed.
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(error) => return Err(GitError::Failed(error.to_string())),
            }
        };
        let (stdout, capped) = out_reader.join().unwrap_or_default();
        let (stderr, _) = err_reader.join().unwrap_or_default();
        let Some(status) = status else {
            return Err(GitError::Timeout(self.timeout));
        };
        // A capped read closes the pipe, so git may die of SIGPIPE; the data
        // read so far is still good.
        if !status.success() && !capped {
            return Err(self.classify(&String::from_utf8_lossy(&stderr)));
        }
        Ok(Output {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            capped,
        })
    }

    /// Map git's stderr to an error without leaking the absolute root.
    fn classify(&self, stderr: &str) -> GitError {
        let lower = stderr.to_ascii_lowercase();
        if lower.contains("not a git repository") {
            return GitError::NotARepository;
        }
        if lower.contains("unknown revision")
            || lower.contains("bad object")
            || lower.contains("bad revision")
            || lower.contains("ambiguous argument")
            || lower.contains("not a valid object")
        {
            return GitError::NotFound("no such commit in this repository".to_owned());
        }
        let root = self.root.to_string_lossy();
        let line = stderr.lines().next().unwrap_or("").replace(&*root, ".");
        let line: String = line.chars().take(300).collect();
        GitError::Failed(if line.is_empty() {
            "git exited with an error".to_owned()
        } else {
            line
        })
    }
}

struct Output {
    stdout: String,
    capped: bool,
}

/// Options placed before the subcommand on every invocation.
fn global_args() -> Vec<String> {
    [
        "--no-pager",
        // Belt and braces with --no-pager: even a configured pager is `cat`.
        "-c",
        "core.pager=cat",
        // A repository config may name an fsmonitor hook, which is a program.
        "-c",
        "core.fsmonitor=false",
        // Signature verification would run gpg.
        "-c",
        "log.showSignature=false",
        // Pathspecs are literal paths: no `:(top)` or glob magic can widen
        // the scope beyond the validated path.
        "--literal-pathspecs",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Options placed after the subcommand of every diff-producing call.
fn output_args() -> Vec<String> {
    // No external diff driver, no textconv filter (both run programs named in
    // repository configuration), and no color escapes in the text.
    ["--no-ext-diff", "--no-textconv", "--no-color"]
        .map(str::to_owned)
        .to_vec()
}

/// The arguments after the global ones for a log query.
pub(crate) fn log_args(query: &LogQuery) -> Vec<String> {
    let mut args = vec!["log".to_owned()];
    args.extend(output_args());
    // One more than asked, to learn whether history continues.
    args.push(format!("-n{}", query.count() + 1));
    args.push(FORMAT.to_owned());
    if let Some(since) = query.since() {
        args.push(format!("--since={since}"));
    }
    // A subdirectory root limits history to itself: `.` is relative to the
    // `-C` directory, so it is the root whether or not it is the repository top.
    args.push("--".to_owned());
    args.push(query.pathspec().unwrap_or(".").to_owned());
    args
}

/// The arguments after the global ones for a show query.
pub(crate) fn show_args(query: &ShowQuery) -> Vec<String> {
    let mut args = vec!["show".to_owned()];
    args.extend(output_args());
    // Paths relative to the root, and changes outside it left out.
    args.push("--relative".to_owned());
    args.push("--stat=200".to_owned());
    args.push("--stat-count=40".to_owned());
    if query.patch() {
        args.push("--patch".to_owned());
    }
    args.push(FORMAT.to_owned());
    // The hash is validated hex; `^{commit}` refuses blobs, trees and tags.
    args.push(format!("{}^{{commit}}", query.hash()));
    args.push("--".to_owned());
    args.push(".".to_owned());
    args
}

impl GitHistory for SubprocessGit {
    fn log(&self, query: &LogQuery) -> Result<GitLog, GitError> {
        let output = match self.run(&log_args(query)) {
            Ok(output) => output,
            // A repository without commits has an empty history.
            Err(GitError::Failed(detail)) if detail.contains("does not have any commits") => {
                return Ok(GitLog {
                    commits: Vec::new(),
                    more: false,
                });
            }
            Err(error) => return Err(error),
        };
        let mut commits = parse_commits(&output.stdout);
        let more = commits.len() > query.count() || output.capped;
        commits.truncate(query.count());
        Ok(GitLog { commits, more })
    }

    fn show(&self, query: &ShowQuery) -> Result<GitShow, GitError> {
        let output = self.run(&show_args(query))?;
        let Some((header, rest)) = output.stdout.split_once('\0') else {
            // `-- .` hides a commit that changed nothing under the root, the
            // same commits `git_log` leaves out of its history.
            return Err(GitError::NotFound(
                "that commit is not part of this project's history (it changes no file under \
                 the project root)"
                    .to_owned(),
            ));
        };
        let commit = parse_commit(header.trim_start())
            .ok_or_else(|| GitError::Failed("git produced an unreadable commit".to_owned()))?;
        let rest = rest.trim_matches('\n');
        let (stat, patch) = match rest.find("\ndiff --git ") {
            Some(at) => (
                rest[..at].trim_matches('\n'),
                Some(rest[at + 1..].to_owned()),
            ),
            None if rest.starts_with("diff --git ") => ("", Some(rest.to_owned())),
            None => (rest, None),
        };
        Ok(GitShow {
            commit,
            stat: stat.to_owned(),
            patch: if query.patch() {
                Some(patch.unwrap_or_default())
            } else {
                None
            },
            truncated: output.capped,
        })
    }
}

/// Records of a log, dropping the last piece: it is the empty tail of a
/// complete output, or a record cut off by the byte cap.
fn parse_commits(text: &str) -> Vec<GitCommit> {
    let mut records: Vec<&str> = text.split('\0').collect();
    records.pop();
    records
        .into_iter()
        .filter_map(|record| parse_commit(record.trim_start()))
        .collect()
}

/// One `hash FS author FS date FS subject FS body` record. The body is split
/// last, so a stray separator inside a message stays in the message, and a
/// record whose first field is not a commit id is dropped.
fn parse_commit(record: &str) -> Option<GitCommit> {
    let mut fields = record.splitn(5, FIELD);
    let hash = fields.next()?;
    let valid = (hash.len() == 40 || hash.len() == 64)
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !valid {
        return None;
    }
    Some(GitCommit {
        hash: hash.to_owned(),
        author: fields.next()?.to_owned(),
        date: fields.next()?.to_owned(),
        subject: fields.next()?.to_owned(),
        body: fields.next().unwrap_or("").trim().to_owned(),
    })
}

/// Reads at most `max` bytes. The flag reports that more data existed.
fn read_capped<R: Read>(source: Option<R>, max: usize) -> (Vec<u8>, bool) {
    let Some(source) = source else {
        return (Vec::new(), false);
    };
    let mut buffer = Vec::new();
    // `max + 1` bytes tell "exactly max" from "more than max".
    let _ = source.take(max as u64 + 1).read_to_end(&mut buffer);
    let capped = buffer.len() > max;
    buffer.truncate(max);
    (buffer, capped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> GitToolConfig {
        GitToolConfig::default()
    }

    #[test]
    fn log_arguments_are_fixed_and_pathspec_comes_last() {
        let query =
            LogQuery::new(Some(3), Some("2 weeks ago"), Some("src/auth"), &config()).unwrap();
        let args = log_args(&query);
        assert_eq!(
            args,
            [
                "log",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "-n4",
                FORMAT,
                "--since=2 weeks ago",
                "--",
                "src/auth"
            ]
        );
        let whole = log_args(&LogQuery::new(None, None, None, &config()).unwrap());
        assert_eq!(&whole[whole.len() - 2..], ["--", "."]);
    }

    #[test]
    fn show_arguments_force_a_commit_and_limit_to_the_root() {
        let args = show_args(&ShowQuery::new("ABC1234", true).unwrap());
        assert!(args.contains(&"abc1234^{commit}".to_owned()));
        assert!(args.contains(&"--patch".to_owned()));
        assert_eq!(&args[args.len() - 2..], ["--", "."]);
        assert!(
            !show_args(&ShowQuery::new("abc1234", false).unwrap()).contains(&"--patch".to_owned())
        );
    }

    #[test]
    fn the_global_arguments_carry_every_hardening_option() {
        let global = global_args();
        for needle in [
            "--no-pager",
            "core.pager=cat",
            "core.fsmonitor=false",
            "log.showSignature=false",
            "--literal-pathspecs",
        ] {
            assert!(global.iter().any(|a| a == needle), "{needle}");
        }
        let output = output_args();
        for needle in ["--no-ext-diff", "--no-textconv", "--no-color"] {
            assert!(output.iter().any(|a| a == needle), "{needle}");
        }
    }

    #[test]
    fn a_forged_record_is_dropped_and_stray_separators_stay_in_the_body() {
        let hash = "a".repeat(40);
        let text = format!(
            "{hash}\x1fAda\x1f2026-10-01T00:00:00+00:00\x1fsubject\x1fbody\x1fmore\0\n\
             not-a-hash\x1fx\x1fy\x1fz\x1f\0\n{}\x1fB\x1fd\x1fcut",
            "b".repeat(40)
        );
        let commits = parse_commits(&text);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].body, "body\x1fmore");
    }

    #[test]
    fn capped_reads_report_extra_data() {
        let (data, capped) = read_capped(Some(&b"abcdef"[..]), 4);
        assert_eq!((data.as_slice(), capped), (&b"abcd"[..], true));
        let (data, capped) = read_capped(Some(&b"abcd"[..]), 4);
        assert_eq!((data.as_slice(), capped), (&b"abcd"[..], false));
    }
}
