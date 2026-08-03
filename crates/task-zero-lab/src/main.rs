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
//! cargo run --bin task_zero_lab -- graph --repo . --rev HEAD --epics 011,012,013
//! cargo run --bin task_zero_lab -- packet --repo . --rev HEAD --epics 014 \
//!   --request "continue closing the Task 0 blockers"
//! cargo run --bin task_zero_lab -- prompt --repo . --rev HEAD --epics 011,012,013 \
//!   --request "continue closing the Task 0 blockers" --output /tmp/task-zero.md
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
    /// Extract the Epic 014 Task 2 evidence graph: selected epics'
    /// Markdown structure, `AGENTS.md`/`DEVELOP.md`/`Cargo.toml` project
    /// rules, and revision-pinned Git facts.
    Graph {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value = "HEAD")]
        rev: String,
        /// Comma-separated epic numbers, e.g. `011,012,013` or `11,12,13`.
        #[arg(long)]
        epics: String,
        /// See `Snapshot`'s `--include-dirty`; same disclosed dirty-input
        /// policy, now actually consumed by document reading.
        #[arg(long)]
        include_dirty: bool,
    },
    /// Select a bounded, source-stratified context packet for `--request`
    /// over the Epic 014 Task 2 evidence graph (Epic 014 Task 3).
    Packet {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value = "HEAD")]
        rev: String,
        /// Comma-separated epic numbers, e.g. `011,012,013` or `11,12,13`.
        #[arg(long)]
        epics: String,
        #[arg(long)]
        request: String,
        /// See `Snapshot`'s `--include-dirty`.
        #[arg(long)]
        include_dirty: bool,
        #[arg(long, default_value_t = task_zero_lab::packet::PacketBudget::default().max_depth)]
        max_depth: usize,
        #[arg(long, default_value_t = task_zero_lab::packet::PacketBudget::default().max_items)]
        max_items: usize,
        #[arg(long, default_value_t = task_zero_lab::packet::PacketBudget::default().max_excerpt_bytes)]
        max_excerpt_bytes: usize,
        #[arg(long, default_value_t = task_zero_lab::packet::PacketBudget::default().max_total_bytes)]
        max_total_bytes: usize,
    },
    /// Render a deterministic Markdown prompt for manual handoff. This never
    /// dispatches it and writes only when `--output` is explicitly supplied.
    Prompt {
        #[arg(long)] repo: PathBuf,
        #[arg(long, default_value = "HEAD")] rev: String,
        #[arg(long)] epics: String,
        #[arg(long)] request: String,
        #[arg(long)] objective: Option<String>,
        #[arg(long = "constraint")] negative_constraints: Vec<String>,
        #[arg(long)] include_dirty: bool,
        #[arg(long, default_value_t = task_zero_lab::prompt::PromptBudget::default().max_bytes)] max_prompt_bytes: usize,
        #[arg(long)] output: Option<PathBuf>,
    },
}

/// Shared `--epics` parsing for `Graph` and `Packet`.
fn parse_epic_numbers(spec: &str) -> anyhow::Result<Vec<u32>> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u32>().map_err(|e| anyhow::anyhow!("invalid epic number '{}' in --epics: {}", s, e)))
        .collect()
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
        Command::Graph {
            repo,
            rev,
            epics,
            include_dirty,
        } => {
            let epic_numbers = parse_epic_numbers(&epics)?;
            let extraction = task_zero_lab::graph_build::build_graph(&repo, &rev, &epic_numbers, include_dirty)?;
            println!("{}", extraction.to_normalized_json()?);
        }
        Command::Packet {
            repo,
            rev,
            epics,
            request,
            include_dirty,
            max_depth,
            max_items,
            max_excerpt_bytes,
            max_total_bytes,
        } => {
            let epic_numbers = parse_epic_numbers(&epics)?;
            let extraction = task_zero_lab::graph_build::build_graph(&repo, &rev, &epic_numbers, include_dirty)?;
            let budget = task_zero_lab::packet::PacketBudget {
                max_depth,
                max_items,
                max_excerpt_bytes,
                max_total_bytes,
                allowed_relations: task_zero_lab::packet::default_expansion_allowlist(),
            };
            let packet = task_zero_lab::packet::select_packet(&extraction, &request, budget)?;
            println!("{}", packet.to_normalized_json()?);
        }
        Command::Prompt { repo, rev, epics, request, objective, negative_constraints, include_dirty, max_prompt_bytes, output } => {
            let epic_numbers = parse_epic_numbers(&epics)?;
            let extraction = task_zero_lab::graph_build::build_graph(&repo, &rev, &epic_numbers, include_dirty)?;
            let mut packet_budget = task_zero_lab::packet::PacketBudget::default();
            packet_budget.max_items = 200;
            packet_budget.max_total_bytes = 100_000;
            let packet = task_zero_lab::packet::select_packet(&extraction, &request, packet_budget)?;
            let request = task_zero_lab::prompt::PromptRequest {
                original: request,
                clarified_objective: objective,
                negative_constraints,
                mutation_boundary: task_zero_lab::prompt::MutationBoundary::RepositoryChangesOnlyWhenExplicitlyRequested,
            };
            let rendered = task_zero_lab::prompt::render_prompt(&request, &packet, task_zero_lab::prompt::PromptBudget {
                max_bytes: max_prompt_bytes,
                bytes_per_token_estimate: 4,
            })?;
            if let Some(path) = output {
                std::fs::write(&path, &rendered.text)
                    .map_err(|e| anyhow::anyhow!("failed to export prompt to {}: {}", path.display(), e))?;
                eprintln!("exported manual-handoff prompt to {} (no dispatch performed)", path.display());
            } else {
                print!("{}", rendered.text);
            }
        }
    }
    Ok(())
}
