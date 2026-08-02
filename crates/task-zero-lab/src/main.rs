//! Epic 014 Task 1: `task_zero_lab` CLI.
//!
//! Experimental, read-only research-harness CLI for the Task Zero Prompt
//! Lab (Epic 014). Not a production interface — see
//! `epics/014-task-zero-prompt-lab.md` Design Constraint 1. Every subcommand
//! is offline, credential-free, and performs no mutation, checkout, network
//! call, or dependency installation.
//!
//! ```text
//! cargo run --bin task_zero_lab -- snapshot --repo . --rev HEAD
//! cargo run --bin task_zero_lab -- coverage --repo .
//! cargo run --bin task_zero_lab -- content --path some/file.md
//! ```

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "task_zero_lab",
    about = "Epic 014 experimental, read-only Task Zero Prompt Lab CLI. Not a production interface; see epics/014-task-zero-prompt-lab.md."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Resolve a Git revision and report its identity, parents, changed
    /// files, working-tree/staleness signals, and index coverage.
    Snapshot {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value = "HEAD")]
        rev: String,
        /// Explicitly record that a dirty/non-pinned working tree may be
        /// used downstream for this run (Design Constraint 2's
        /// dirty-worktree policy). Task 1 itself never substitutes current
        /// file content for the requested revision's content regardless of
        /// this flag; see `run::DirtyInputPolicy`.
        #[arg(long)]
        include_dirty: bool,
    },
    /// Report deterministic xxh3 content identity for a non-Git file or
    /// directory of files.
    Content {
        #[arg(long)]
        path: PathBuf,
    },
    /// Report index coverage only (no Git resolution); works for any
    /// directory, Git repository or not.
    Coverage {
        #[arg(long)]
        repo: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Snapshot {
            repo,
            rev,
            include_dirty,
        } => {
            let snapshot = task_zero_lab::run::build_snapshot(&repo, &rev, include_dirty)?;
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        Command::Content { path } => {
            let value = if path.is_dir() {
                serde_json::to_value(task_zero_lab::content_identity::inspect_dir(&path)?)?
            } else {
                serde_json::to_value(task_zero_lab::content_identity::inspect_file(&path)?)?
            };
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        Command::Coverage { repo } => {
            let coverage = task_zero_lab::index_coverage::inspect(&repo)?;
            println!("{}", serde_json::to_string_pretty(&coverage)?);
        }
    }
    Ok(())
}
