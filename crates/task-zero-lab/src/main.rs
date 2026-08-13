//! Epic 014 Task 1: `task_zero_lab` CLI.
//!
//! Experimental, read-only research-harness CLI for the Task Zero Prompt
//! Lab (Epic 014). Not a production interface — see
//! `epics/014-task-zero-prompt-lab.md` Design Constraint 1. All default and
//! scripted paths are offline and credential-free. Only the explicit
//! `helper --live-openrouter` mode performs a credentialed network request;
//! no mode performs repository mutation, checkout, or dependency installation.
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
//! cargo run --bin task_zero_lab -- helper --repo . --rev HEAD --epics 014 \
//!   --request "continue closing the Task 0 blockers"
//! cargo run --bin task_zero_lab -- helper --repo . --rev HEAD --epics 014 \
//!   --request "continue closing the Task 0 blockers" \
//!   --scripted-fixture crates/task-zero-lab/fixtures/helper/search-then-submit.json \
//!   --prompt-patterns-dir crates/task-zero-lab/fixtures/prompt_patterns
//! cargo run --bin task_zero_lab -- helper --repo . --rev HEAD --epics 014 \
//!   --request "continue closing the Task 0 blockers" \
//!   --live-llama-cpp --llama-cpp-model qwen3.5-0.8b
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
    /// Render a prompt via the Epic 014 Task 5 optional helper-refinement
    /// stage. Omitting `--scripted-fixture` runs the always-available
    /// disabled path, byte-identical to `prompt` (Task 5 acceptance
    /// criterion). Live execution is opt-in: either `--live-openrouter`
    /// (requires OPENROUTER_API_KEY) or `--live-llama-cpp` (requires a
    /// locally-running llama-server).
    Helper {
        #[arg(long)] repo: PathBuf,
        #[arg(long, default_value = "HEAD")] rev: String,
        #[arg(long)] epics: String,
        #[arg(long)] request: String,
        #[arg(long)] objective: Option<String>,
        #[arg(long = "constraint")] negative_constraints: Vec<String>,
        #[arg(long)] include_dirty: bool,
        #[arg(long, default_value_t = task_zero_lab::prompt::PromptBudget::default().max_bytes)] max_prompt_bytes: usize,
        #[arg(long)] output: Option<PathBuf>,
        /// Path to a JSON `Vec<HelperModelOutput>` scripted session (see
        /// `crates/task-zero-lab/fixtures/helper/`). Omit to run disabled.
        #[arg(long)] scripted_fixture: Option<PathBuf>,
        /// Explicitly enable credentialed OpenRouter execution. Mutually
        /// exclusive with `--scripted-fixture` and `--live-llama-cpp`.
        #[arg(long, conflicts_with_all = ["scripted_fixture", "live_llama_cpp"])]
        live_openrouter: bool,
        /// Exact OpenRouter author/model ID; routed aliases are rejected.
        #[arg(long, requires = "live_openrouter")]
        openrouter_model: Option<String>,
        /// Ordered upstream provider pin. Repeat to specify a strict order;
        /// fallback remains disabled.
        #[arg(long, requires = "live_openrouter")]
        openrouter_provider: Vec<String>,
        #[arg(long, requires = "live_openrouter", default_value_t = task_zero_lab::openrouter_helper::DEFAULT_TEMPERATURE)]
        openrouter_temperature: f32,
        #[arg(long, requires = "live_openrouter", default_value_t = task_zero_lab::openrouter_helper::DEFAULT_OUTPUT_LIMIT)]
        openrouter_output_limit: u32,
        /// Explicitly enable local llama.cpp execution via llama-server.
        /// No API key or network access required. Mutually exclusive with
        /// `--scripted-fixture` and `--live-openrouter`.
        #[arg(long, conflicts_with_all = ["scripted_fixture", "live_openrouter"])]
        live_llama_cpp: bool,
        /// Model name to pass to llama-server (informational; the server
        /// loads whatever model was specified at startup).
        #[arg(long, requires = "live_llama_cpp", default_value = "qwen3.5-0.8b")]
        llama_cpp_model: String,
        /// llama-server base URL.
        #[arg(long, requires = "live_llama_cpp", default_value = "http://localhost:8080")]
        llama_cpp_url: String,
        #[arg(long, requires = "live_llama_cpp", default_value_t = task_zero_lab::llama_cpp_helper::DEFAULT_TEMPERATURE)]
        llama_cpp_temperature: f32,
        #[arg(long, requires = "live_llama_cpp", default_value_t = task_zero_lab::llama_cpp_helper::DEFAULT_OUTPUT_LIMIT)]
        llama_cpp_output_limit: u32,
        #[arg(long, default_value_t = task_zero_lab::helper::HelperPolicy::default().max_rounds)] max_rounds: usize,
        #[arg(long, default_value_t = task_zero_lab::helper::HelperPolicy::default().max_calls)] max_calls: usize,
        #[arg(long, default_value_t = task_zero_lab::helper::HelperPolicy::default().max_result_bytes)] max_result_bytes: usize,
        /// Directory of prompt-pattern JSON fixtures (see
        /// `crates/task-zero-lab/fixtures/prompt_patterns/`).
        #[arg(long)] prompt_patterns_dir: Option<PathBuf>,
        /// Where to write the `HelperRunReport` JSON. Prints to stderr when omitted.
        #[arg(long)] report_output: Option<PathBuf>,
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
        Command::Helper {
            repo,
            rev,
            epics,
            request,
            objective,
            negative_constraints,
            include_dirty,
            max_prompt_bytes,
            output,
            scripted_fixture,
            live_openrouter,
            openrouter_model,
            openrouter_provider,
            openrouter_temperature,
            openrouter_output_limit,
            live_llama_cpp,
            llama_cpp_model,
            llama_cpp_url,
            llama_cpp_temperature,
            llama_cpp_output_limit,
            max_rounds,
            max_calls,
            max_result_bytes,
            prompt_patterns_dir,
            report_output,
        } => {
            let epic_numbers = parse_epic_numbers(&epics)?;
            let extraction = task_zero_lab::graph_build::build_graph(&repo, &rev, &epic_numbers, include_dirty)?;
            let mut packet_budget = task_zero_lab::packet::PacketBudget::default();
            packet_budget.max_items = 200;
            packet_budget.max_total_bytes = 100_000;
            let packet = task_zero_lab::packet::select_packet(&extraction, &request, packet_budget)?;
            let prompt_request = task_zero_lab::prompt::PromptRequest {
                original: request,
                clarified_objective: objective,
                negative_constraints,
                mutation_boundary: task_zero_lab::prompt::MutationBoundary::RepositoryChangesOnlyWhenExplicitlyRequested,
            };
            let (patterns, pattern_diagnostics) = match &prompt_patterns_dir {
                Some(dir) => task_zero_lab::helper::load_prompt_patterns(dir)?,
                None => (Vec::new(), Vec::new()),
            };
            for diagnostic in &pattern_diagnostics {
                eprintln!("prompt pattern diagnostic: {diagnostic}");
            }
            let policy = task_zero_lab::helper::HelperPolicy {
                max_rounds,
                max_calls,
                max_result_bytes,
                ..task_zero_lab::helper::HelperPolicy::default()
            };
            let prompt_budget = task_zero_lab::prompt::PromptBudget {
                max_bytes: max_prompt_bytes,
                bytes_per_token_estimate: 4,
            };
            let (rendered, report) = match &scripted_fixture {
                Some(fixture_path) => {
                    let mut model = task_zero_lab::helper::ScriptedHelperModel::from_fixture_file(fixture_path)?;
                    task_zero_lab::helper::refine_prompt(
                        &extraction,
                        &packet,
                        &prompt_request,
                        prompt_budget,
                        &policy,
                        Some(&mut model),
                        &patterns,
                        &format!("scripted:{}", fixture_path.display()),
                    )?
                }
                None if live_openrouter => {
                    // Load `.env` only in this explicit credentialed mode.
                    // dotenvy never logs values, and neither does this CLI.
                    let _ = dotenvy::dotenv();
                    let api_key = std::env::var("OPENROUTER_API_KEY")
                        .map_err(|_| anyhow::anyhow!("live OpenRouter mode requires OPENROUTER_API_KEY in the environment or ignored .env"))?;
                    let config = task_zero_lab::openrouter_helper::OpenRouterHelperConfig {
                        model: openrouter_model.ok_or_else(|| anyhow::anyhow!("--openrouter-model is required in live OpenRouter mode"))?,
                        provider_order: openrouter_provider,
                        temperature: openrouter_temperature,
                        output_limit: openrouter_output_limit,
                    };
                    let adapter_name = format!("openrouter:{}", config.model);
                    let mut model = task_zero_lab::openrouter_helper::OpenRouterHelperModel::new(api_key, config)?;
                    task_zero_lab::helper::refine_prompt(
                        &extraction,
                        &packet,
                        &prompt_request,
                        prompt_budget,
                        &policy,
                        Some(&mut model),
                        &patterns,
                        &adapter_name,
                    )?
                }
                None if live_llama_cpp => {
                    let config = task_zero_lab::llama_cpp_helper::LlamaCppHelperConfig {
                        model: llama_cpp_model.clone(),
                        base_url: llama_cpp_url,
                        temperature: llama_cpp_temperature,
                        output_limit: llama_cpp_output_limit,
                    };
                    let adapter_name = format!("llama_cpp:{}", config.model);
                    let mut model = task_zero_lab::llama_cpp_helper::LlamaCppHelperModel::new(config)?;
                    task_zero_lab::helper::refine_prompt(
                        &extraction,
                        &packet,
                        &prompt_request,
                        prompt_budget,
                        &policy,
                        Some(&mut model),
                        &patterns,
                        &adapter_name,
                    )?
                }
                None => task_zero_lab::helper::refine_prompt(&extraction, &packet, &prompt_request, prompt_budget, &policy, None, &patterns, "disabled")?,
            };
            if let Some(path) = output {
                std::fs::write(&path, &rendered.text)
                    .map_err(|e| anyhow::anyhow!("failed to export prompt to {}: {}", path.display(), e))?;
                eprintln!("exported manual-handoff prompt to {} (no dispatch performed)", path.display());
            } else {
                print!("{}", rendered.text);
            }
            let report_json = serde_json::to_string_pretty(&report)?;
            if let Some(path) = report_output {
                std::fs::write(&path, &report_json)
                    .map_err(|e| anyhow::anyhow!("failed to write helper report to {}: {}", path.display(), e))?;
                eprintln!("wrote helper run report to {}", path.display());
            } else {
                eprintln!("{report_json}");
            }
        }
    }
    Ok(())
}
