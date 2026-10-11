//! Agent chat CLI (Epic 016): send a prompt to Claude Code, Codex, or
//! opencode, ask for a sectioned Markdown reply, and log the whole run.

use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use clap::{Parser, ValueEnum};

const SYSTEM_PROMPT: &str = "\
Work on the user's request using whatever tools you need. When you are done, \
write your final reply as Markdown using only these top-level sections, in \
this order, and omit any section that has nothing in it:

## Overview
A short summary of what you found or did.

## Thoughts
Reasoning, observations, and trade-offs worth keeping.

## Open Questions
One bullet per question that needs the user's decision or more information.

## Tasks
One bullet per concrete follow-up task, each self-contained enough to hand to \
another agent.

Do not add other top-level sections or text outside these sections.";

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Agent {
    Claude,
    Codex,
    Opencode,
}

impl Agent {
    fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Opencode => "opencode",
        }
    }

    /// Program and arguments for one headless run with all tools allowed.
    fn argv(self, model: &str, prompt: &str) -> Vec<String> {
        let s = |v: &str| v.to_string();
        match self {
            Agent::Claude => vec![
                s("claude"),
                s("-p"),
                s("--model"),
                s(model),
                s("--append-system-prompt"),
                s(SYSTEM_PROMPT),
                s("--dangerously-skip-permissions"),
                s(prompt),
            ],
            // `-c` values are parsed as TOML; a JSON string is a valid TOML
            // basic string.
            Agent::Codex => vec![
                s("codex"),
                s("exec"),
                s("--model"),
                s(model),
                s("-c"),
                format!(
                    "developer_instructions={}",
                    serde_json::to_string(SYSTEM_PROMPT).unwrap()
                ),
                s("--dangerously-bypass-approvals-and-sandbox"),
                s(prompt),
            ],
            // opencode run has no system prompt flag, so the preamble goes in
            // the message.
            Agent::Opencode => vec![
                s("opencode"),
                s("run"),
                s("--model"),
                s(model),
                s("--auto"),
                format!("{SYSTEM_PROMPT}\n\n---\n\n{prompt}"),
            ],
        }
    }
}

#[derive(Parser, Debug)]
#[command(about = "Prompt an external coding agent and log the run")]
struct Args {
    /// External agent CLI to run.
    #[arg(long, value_enum)]
    agent: Agent,
    /// Model passed to the agent CLI (opencode expects provider/model).
    #[arg(long)]
    model: String,
    /// Directory the agent works in.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Prompt for a single run. Omit it to start an interactive loop.
    prompt: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let repo = args
        .repo
        .canonicalize()
        .with_context(|| format!("repo path {}", args.repo.display()))?;
    let project = repo
        .file_name()
        .context("repo path has no base name")?
        .to_string_lossy()
        .into_owned();
    let runs_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".projects")
        .join(&project)
        .join("runs");

    if let Some(prompt) = args.prompt {
        run(args.agent, &args.model, &repo, &runs_dir, &prompt)?;
        return Ok(());
    }

    eprintln!(
        "{} ({}) in {}. Press Enter on an empty line to send, Ctrl-D to quit.",
        args.agent.name(),
        args.model,
        repo.display()
    );
    let stdin = io::stdin();
    loop {
        eprint!("\n> ");
        io::stderr().flush()?;
        let mut prompt = String::new();
        let mut eof = false;
        loop {
            let mut line = String::new();
            if stdin.lock().read_line(&mut line)? == 0 {
                eof = true;
                break;
            }
            if line.trim().is_empty() && !prompt.trim().is_empty() {
                break;
            }
            prompt.push_str(&line);
        }
        let prompt = prompt.trim();
        if !prompt.is_empty() {
            if let Err(e) = run(args.agent, &args.model, &repo, &runs_dir, prompt) {
                eprintln!("error: {e:#}");
            }
        }
        if eof {
            return Ok(());
        }
    }
}

/// Runs the agent once, streaming its output, and writes the run log.
fn run(agent: Agent, model: &str, repo: &Path, runs_dir: &Path, prompt: &str) -> Result<()> {
    let started = Utc::now();
    let dir = runs_dir.join(format!(
        "{}-{}",
        started.format("%Y%m%dT%H%M%S%.3fZ"),
        agent.name()
    ));
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    std::fs::write(dir.join("prompt.md"), prompt)?;
    std::fs::write(dir.join("system_prompt.md"), SYSTEM_PROMPT)?;

    let status_before = git(repo, &["status", "--porcelain=v1"]);
    let head_before = git(repo, &["rev-parse", "HEAD"]);
    std::fs::write(dir.join("git_status_before.txt"), &status_before)?;

    let argv = agent.argv(model, prompt);
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(repo)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {}", argv[0]))?;
    let stdout = tee(child.stdout.take().unwrap(), io::stdout());
    let stderr = tee(child.stderr.take().unwrap(), io::stderr());
    let exit = child.wait()?;
    let stdout = stdout.join().unwrap()?;
    let stderr = stderr.join().unwrap()?;
    let finished = Utc::now();

    let status_after = git(repo, &["status", "--porcelain=v1"]);
    let head_after = git(repo, &["rev-parse", "HEAD"]);
    std::fs::write(dir.join("stdout.md"), &stdout)?;
    std::fs::write(dir.join("stderr.txt"), &stderr)?;
    std::fs::write(dir.join("git_status_after.txt"), &status_after)?;

    let meta = serde_json::json!({
        "agent": agent.name(),
        "model": model,
        "repo": repo,
        "argv": argv,
        "started_at": started.to_rfc3339(),
        "finished_at": finished.to_rfc3339(),
        "exit_code": exit.code(),
        "head_before": head_before.trim(),
        "head_after": head_after.trim(),
    });
    std::fs::write(dir.join("run.json"), serde_json::to_string_pretty(&meta)?)?;

    eprintln!("\n[logged to {}]", dir.display());
    if status_before != status_after || head_before != head_after {
        eprintln!("[working tree changed]\n{status_after}");
    }
    if !exit.success() {
        bail!("{} exited with {exit}", agent.name());
    }
    Ok(())
}

/// Copies a child stream to `out` as it arrives and returns everything read.
fn tee<R, W>(mut src: R, mut out: W) -> thread::JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = src.read(&mut buf)?;
            if n == 0 {
                return Ok(all);
            }
            out.write_all(&buf[..n])?;
            out.flush()?;
            all.extend_from_slice(&buf[..n]);
        }
    })
}

/// Git output for the run log; a non-git directory or git error is recorded
/// as text rather than failing the run.
fn git(repo: &Path, args: &[&str]) -> String {
    match Command::new("git").args(args).current_dir(repo).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => format!("git error: {}", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => format!("git error: {e}"),
    }
}
