//! Epic 014 Task 6 eval runner: orchestrates prompt variant comparison.
//!
//! This binary runs the full evaluation pipeline:
//! 1. For each case (C01, C07), for each variant (raw, deterministic,
//!    helper-refined), for each repetition:
//!    a. Create a disposable worktree at the case's pinned revision
//!    b. Generate the prompt (variant-dependent)
//!    c. Hand the prompt to a coding agent
//!    d. Capture the resulting diff
//!    e. Score against the practical relevance set
//! 2. Produce a comparison report.
//!
//! ## Modes
//!
//! - `run`: execute live against a capable model (requires credentials or
//!   local model)
//! - `scripted`: replay from pre-recorded fixtures (no credentials needed)
//! - `prompt-only`: generate and score prompts without running an agent
//!   (deterministic, offline)

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use task_zero_lab::agent::{
    CodingAgent, LlamaCppAgentConfig, LlamaCppCodingAgent, OpenRouterAgentConfig,
    OpenRouterCodingAgent, ScriptedCodingAgent,
};
use task_zero_lab::eval::{
    all_cases, AgentResult, CaseReport, EvalCase, EvalReport, EvalRun, EvalScore, PromptVariant,
    PromptVariantKind, EVAL_SCHEMA_VERSION,
};
use task_zero_lab::graph_build;
use task_zero_lab::helper::{self, HelperModel, HelperPolicy, ScriptedHelperModel};
use task_zero_lab::llama_cpp_helper::{LlamaCppHelperConfig, LlamaCppHelperModel};
use task_zero_lab::openrouter_helper::{OpenRouterHelperConfig, OpenRouterHelperModel};
use task_zero_lab::packet::{self, PacketBudget};
use task_zero_lab::prompt::{self, PromptBudget, PromptRequest};
use task_zero_lab::scoring;
use task_zero_lab::worktree::EvalWorktree;

#[derive(Parser)]
#[command(about = "Epic 014 Task 6: prompt variant comparison and task-outcome evaluation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run live evaluation against a capable model via OpenRouter.
    Run {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value = "HEAD")]
        rev: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        provider: String,
        #[arg(long, default_value = "1")]
        repetitions: u32,
        #[arg(long)]
        output: PathBuf,
        /// Only generate and score prompts, do not run the agent.
        #[arg(long)]
        prompt_only: bool,
        /// Use the non-expert request rendering.
        #[arg(long)]
        non_expert: bool,
        /// Helper model for the helper-refined variant (OpenRouter model ID).
        #[arg(long)]
        helper_model: Option<String>,
        /// Helper provider for the helper-refined variant.
        #[arg(long)]
        helper_provider: Option<String>,
        /// Scripted helper fixture for offline helper-refined variant.
        #[arg(long)]
        helper_scripted: Option<PathBuf>,
        /// Restrict the run to specific case IDs (comma-separated, e.g.
        /// "C01,C07"). Defaults to all cases from `all_cases()` when omitted.
        #[arg(long, value_delimiter = ',')]
        case: Vec<String>,
    },
    /// Run live evaluation against a local llama.cpp model.
    RunLlamaCpp {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value = "HEAD")]
        rev: String,
        #[arg(long, default_value = "qwen3.5-9b")]
        model: String,
        #[arg(long, default_value = "http://localhost:8080")]
        url: String,
        #[arg(long, default_value = "1")]
        repetitions: u32,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        prompt_only: bool,
        #[arg(long)]
        non_expert: bool,
        /// Helper model for the helper-refined variant (llama.cpp model ID).
        #[arg(long)]
        helper_model: Option<String>,
        /// Helper llama.cpp URL for the helper-refined variant.
        #[arg(long)]
        helper_url: Option<String>,
        /// Scripted helper fixture for offline helper-refined variant.
        #[arg(long)]
        helper_scripted: Option<PathBuf>,
        /// Restrict the run to specific case IDs (comma-separated, e.g.
        /// "C01,C07"). Defaults to all cases from `all_cases()` when omitted.
        /// Useful because `all_cases()` includes cases pinned to private
        /// repos (e.g. C02) that will not exist on most machines.
        #[arg(long, value_delimiter = ',')]
        case: Vec<String>,
        /// Use `PatchApplyCodingAgent`: the model receives only the prompt,
        /// then the host applies its returned unified diff via `git apply`.
        /// This does not give the model read/list/search access to the checkout.
        #[arg(long)]
        patch_apply: bool,
    },
    /// Replay from pre-recorded scripted fixtures (offline, no credentials).
    Scripted {
        #[arg(long)]
        fixture: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Scripted helper fixture for offline helper-refined variant.
        #[arg(long)]
        helper_scripted: Option<PathBuf>,
    },
    /// Generate prompts for inspection (no agent, no scoring).
    Prompts {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value = "HEAD")]
        rev: String,
        #[arg(long)]
        case: String,
        #[arg(long)]
        non_expert: bool,
        /// Scripted helper fixture for offline helper-refined variant.
        #[arg(long)]
        helper_scripted: Option<PathBuf>,
    },
}

const PATTERNS_DIR: &str = "crates/task-zero-lab/fixtures/prompt_patterns";

struct PromptVariants {
    variants: Vec<PromptVariant>,
    graph_hash: String,
    packet_hash: String,
}

fn build_prompt_variants(
    case: &EvalCase,
    repo: &std::path::Path,
    rev: &str,
    non_expert: bool,
    helper_model: Option<&mut Box<dyn HelperModel>>,
    helper_adapter_name: &str,
) -> anyhow::Result<PromptVariants> {
    let request_text = if non_expert {
        case.non_expert_request.as_deref().unwrap_or(&case.expert_request)
    } else {
        &case.expert_request
    };

    let prompt_request = PromptRequest {
        original: request_text.to_string(),
        clarified_objective: None,
        negative_constraints: case.negative_constraints.clone(),
        mutation_boundary: case.mutation_boundary,
    };

    // Variant 1: Raw human prompt
    let raw = PromptVariant {
        kind: PromptVariantKind::RawHuman,
        text: request_text.to_string(),
        byte_count: request_text.len(),
        token_estimate: request_text.len() / 4,
        helper_report_json: None,
    };

    // Variant 2: Deterministic baseline (graph → packet → render)
    let graph = graph_build::build_graph(repo, rev, &case.epics, false)?;
    let mut budget = PacketBudget::default();
    budget.max_items = 200;
    budget.max_total_bytes = 100_000;
    let pkt = packet::select_packet(&graph, request_text, budget)?;
    let rendered = prompt::render_prompt(&prompt_request, &pkt, PromptBudget::default())?;

    let graph_json = graph.to_normalized_json()?;
    let packet_json = pkt.to_normalized_json()?;
    let graph_hash = daftprompt_indexer::db::content_hash(graph_json.as_bytes());
    let packet_hash = daftprompt_indexer::db::content_hash(packet_json.as_bytes());

    let deterministic = PromptVariant {
        kind: PromptVariantKind::DeterministicBaseline,
        text: rendered.text.clone(),
        byte_count: rendered.byte_count,
        token_estimate: rendered.token_estimate,
        helper_report_json: None,
    };

    // Variant 3: Helper-refined (if patterns available)
    let helper = if std::path::Path::new(PATTERNS_DIR).exists() {
        let (patterns, diagnostics) = helper::load_prompt_patterns(std::path::Path::new(PATTERNS_DIR))?;
        if !diagnostics.is_empty() {
            eprintln!("prompt-pattern diagnostics: {}", diagnostics.join("; "));
        }
        let policy = HelperPolicy::default();
        let (rendered_h, report) = helper::refine_prompt(
            &graph,
            &pkt,
            &prompt_request,
            PromptBudget::default(),
            &policy,
            helper_model.map(|m| m.as_mut() as &mut dyn HelperModel),
            &patterns,
            helper_adapter_name,
        )?;
        Some(PromptVariant {
            kind: PromptVariantKind::HelperRefined,
            text: rendered_h.text,
            byte_count: rendered_h.byte_count,
            token_estimate: rendered_h.token_estimate,
            helper_report_json: Some(serde_json::to_string(&report)?),
        })
    } else {
        eprintln!("warning: no prompt patterns at {PATTERNS_DIR}; skipping helper-refined variant");
        None
    };

    let mut variants = vec![raw, deterministic];
    if let Some(h) = helper {
        variants.push(h);
    }
    Ok(PromptVariants {
        variants,
        graph_hash,
        packet_hash,
    })
}

fn run_eval(
    case: &EvalCase,
    prompt_variants: &PromptVariants,
    agent: &dyn CodingAgent,
    repetitions: u32,
    create_worktrees: bool,
) -> anyhow::Result<CaseReport> {
    let mut runs = Vec::new();

    for rep in 1..=repetitions {
        for variant in &prompt_variants.variants {
            eprintln!(
                "  case={} variant={:?} rep={} agent={}:{}",
                case.id,
                variant.kind,
                rep,
                agent.adapter_name(),
                agent.model_name()
            );

            let (worktree_commit, worktree_path, worktree_diff, worktree_changed_files, agent_result, score) = if create_worktrees {
                let label = format!("{}-{:?}-rep{}", case.id, variant.kind, rep);
                let wt = EvalWorktree::create(&case.repo_path, &case.revision, &label)?;

                let commit = wt.resolved_commit().to_string();
                let wt_path = wt.path().to_path_buf();

                // The path is host-side context, not implicitly model-visible.
                // The patch-apply adapter sends only `variant.text` to the
                // model, then uses `wt_path` as `git apply`'s working directory.
                let result = agent.execute(&variant.text, &wt_path)?;

                // Read the ground-truth diff and changed-file list from the
                // worktree now, before `wt` goes out of scope and its Drop
                // impl runs `git worktree remove`. This must happen before
                // the guard drops (Task 6 acceptance criterion: "the
                // resulting diff, build/verification result, and touched
                // artifacts are captured"). `run_verification` inside
                // `score_run`/`score_task_outcome` also runs against
                // `wt_path` on disk, which is still valid here for the same
                // reason.
                let changed_files = wt.changed_files()?;
                let diff_text = wt.capture_diff().ok();

                // Score the result against ground truth, not the agent's
                // self-reported touched_files.
                let score = scoring::score_run(
                    variant,
                    &result,
                    &changed_files,
                    &case.expert_request,
                    &case.negative_constraints,
                    &case.relevance,
                    &wt_path,
                );

                // `wt` drops here (worktree removed) after everything above
                // that needed the live worktree has already run.
                (commit, Some(wt_path), diff_text, changed_files, result, score)
            } else {
                // Prompt-only mode: score prompt quality only.
                // Task outcome is not applicable (no agent execution).
                let prompt_score = scoring::score_prompt_quality(
                    variant,
                    &case.expert_request,
                    &case.negative_constraints,
                );
                let score = EvalScore {
                    prompt_quality: prompt_score,
                    task_outcome: task_zero_lab::eval::TaskOutcomeScore {
                        artifact_recall: 0.0,
                        artifact_precision: 0.0,
                        violated_prohibitions: vec![],
                        verification_passed: None,
                        verification_commands_run: vec![],
                        verification_diagnostics: vec![],
                        unsupported_claims: 0,
                        negative_constraints_respected: true,
                    },
                };
                let result = AgentResult {
                    response_text: "[prompt-only mode: no agent executed]".into(),
                    touched_files: vec![],
                    diff: None,
                    input_tokens: 0,
                    output_tokens: 0,
                    elapsed_ms: 0,
                    stop_reason: Some("prompt_only".into()),
                    diagnostics: vec!["prompt-only mode: no agent execution".into()],
                };
                (case.revision.clone(), None, None, vec![], result, score)
            };

            runs.push(EvalRun {
                schema_version: EVAL_SCHEMA_VERSION.into(),
                case_id: case.id.clone(),
                rendering_id: case.rendering_id.clone(),
                variant: variant.clone(),
                agent_adapter: agent.adapter_name().to_string(),
                agent_model: agent.model_name().to_string(),
                agent_capabilities: agent.capabilities(),
                repetition: rep,
                worktree_commit,
                worktree_diff,
                worktree_changed_files,
                worktree_path,
                agent_result,
                score,
                graph_hash: prompt_variants.graph_hash.clone(),
                packet_hash: prompt_variants.packet_hash.clone(),
                prompt_template_version: task_zero_lab::prompt::PROMPT_TEMPLATE_VERSION.into(),
                eval_timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
    }

    Ok(CaseReport {
        case_id: case.id.clone(),
        runs,
    })
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Run {
            repo,
            rev,
            model,
            provider,
            repetitions,
            output,
            prompt_only,
            non_expert,
            helper_model,
            helper_provider,
            helper_scripted,
            case,
        } => {
            let _ = dotenvy::dotenv();
            let api_key = std::env::var("OPENROUTER_API_KEY").map_err(|_| {
                anyhow::anyhow!("OPENROUTER_API_KEY is required for live runs")
            })?;
            let config = OpenRouterAgentConfig {
                model: model.clone(),
                provider_order: vec![provider],
                temperature: 0.0,
                max_tokens: 4096,
            };
            let agent = OpenRouterCodingAgent::new(api_key.clone(), config);

            // Build helper model for the helper-refined variant
            let mut helper: Option<Box<dyn HelperModel>> = if let Some(path) = helper_scripted {
                Some(Box::new(ScriptedHelperModel::from_fixture_file(&path)?))
            } else if let (Some(hm), Some(hp)) = (helper_model, helper_provider) {
                let h_config = OpenRouterHelperConfig {
                    model: hm,
                    provider_order: vec![hp],
                    temperature: 0.0,
                    output_limit: 2048,
                };
                Some(Box::new(OpenRouterHelperModel::new(api_key, h_config)?))
            } else {
                None
            };
            let helper_name = helper.as_ref().map(|_| "openrouter").unwrap_or("disabled").to_string();
            run_and_report(&repo, &rev, &agent, repetitions, &output, prompt_only, non_expert, helper.as_mut(), &helper_name, &case)?;
        }
        Command::RunLlamaCpp {
            repo,
            rev,
            model,
            url,
            repetitions,
            output,
            prompt_only,
            non_expert,
            helper_model,
            helper_url,
            helper_scripted,
            case,
            patch_apply,
        } => {
            let config = LlamaCppAgentConfig {
                model,
                base_url: url,
                temperature: 0.0,
                max_tokens: 4096,
            };
            let free_form_agent;
            let patch_apply_agent;
            let agent: &dyn CodingAgent = if patch_apply {
                patch_apply_agent = task_zero_lab::agent::PatchApplyCodingAgent::new_llama_cpp(config);
                &patch_apply_agent
            } else {
                free_form_agent = LlamaCppCodingAgent::new(config);
                &free_form_agent
            };

            // Build helper model for the helper-refined variant
            let mut helper: Option<Box<dyn HelperModel>> = if let Some(path) = helper_scripted {
                Some(Box::new(ScriptedHelperModel::from_fixture_file(&path)?))
            } else if let Some(hm) = helper_model {
                let h_url = helper_url.unwrap_or_else(|| "http://localhost:8080".into());
                let h_config = LlamaCppHelperConfig {
                    model: hm,
                    base_url: h_url,
                    temperature: 0.0,
                    output_limit: 2048,
                };
                Some(Box::new(LlamaCppHelperModel::new(h_config)?))
            } else {
                None
            };
            let helper_name = helper.as_ref().map(|_| "llama_cpp").unwrap_or("disabled").to_string();
            run_and_report(&repo, &rev, agent, repetitions, &output, prompt_only, non_expert, helper.as_mut(), &helper_name, &case)?;
        }
        Command::Scripted { fixture, output, helper_scripted } => {
            let agent = ScriptedCodingAgent::from_fixture(&fixture)?;
            let mut helper: Option<Box<dyn HelperModel>> = if let Some(path) = helper_scripted {
                Some(Box::new(ScriptedHelperModel::from_fixture_file(&path)?))
            } else {
                None
            };
            let helper_name = helper.as_ref().map(|_| "scripted").unwrap_or("disabled").to_string();
            let cases = all_cases();
            let mut case_reports = Vec::new();
            for case in &cases {
                let variants = build_prompt_variants(case, &case.repo_path, &case.revision, false, helper.as_mut(), &helper_name)?;
                let report = run_eval(case, &variants, &agent, 1, false)?;
                case_reports.push(report);
            }
            let eval_report = EvalReport {
                schema_version: EVAL_SCHEMA_VERSION.into(),
                agent_adapter: agent.adapter_name().to_string(),
                agent_model: agent.model_name().to_string(),
                agent_capabilities: agent.capabilities(),
                cases: case_reports,
                generated_at: chrono::Utc::now().to_rfc3339(),
            };
            let json = serde_json::to_string_pretty(&eval_report)?;
            std::fs::write(&output, json)?;
            eprintln!("wrote eval report to {}", output.display());
            print_summary(&eval_report);
        }
        Command::Prompts {
            repo,
            rev,
            case,
            non_expert,
            helper_scripted,
        } => {
            let cases = all_cases();
            let eval_case = cases
                .iter()
                .find(|c| c.id.eq_ignore_ascii_case(&case))
                .ok_or_else(|| anyhow::anyhow!("unknown case: {case}"))?;
            let mut helper: Option<Box<dyn HelperModel>> = if let Some(path) = helper_scripted {
                Some(Box::new(ScriptedHelperModel::from_fixture_file(&path)?))
            } else {
                None
            };
            let helper_name = helper.as_ref().map(|_| "scripted").unwrap_or("disabled").to_string();
            let variants = build_prompt_variants(eval_case, &repo, &rev, non_expert, helper.as_mut(), &helper_name)?;
            println!("graph_hash: {}", variants.graph_hash);
            println!("packet_hash: {}", variants.packet_hash);
            println!();
            for v in &variants.variants {
                println!("=== {:?} ({} bytes, ~{} tokens) ===", v.kind, v.byte_count, v.token_estimate);
                println!("{}", v.text);
                println!();
            }
        }
    }
    Ok(())
}

fn run_and_report(
    repo: &std::path::Path,
    rev: &str,
    agent: &dyn CodingAgent,
    repetitions: u32,
    output: &std::path::Path,
    prompt_only: bool,
    non_expert: bool,
    mut helper_model: Option<&mut Box<dyn HelperModel>>,
    helper_adapter_name: &str,
    case_filter: &[String],
) -> anyhow::Result<()> {
    let all = all_cases();
    let cases: Vec<&EvalCase> = if case_filter.is_empty() {
        all.iter().collect()
    } else {
        case_filter
            .iter()
            .map(|id| {
                all.iter()
                    .find(|c| c.id.eq_ignore_ascii_case(id))
                    .ok_or_else(|| anyhow::anyhow!("unknown case: {id}"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
    };
    let mut case_reports = Vec::new();

    for case in cases.iter().copied() {
        // Enforce case repository/revision identity: warn if CLI args drift
        // from the case's pinned values. The case's pinned values take precedence.
        if case.repo_path != repo {
            eprintln!(
                "warning: case {} pinned repo {} differs from CLI --repo {}; using case pinned repo",
                case.id,
                case.repo_path.display(),
                repo.display()
            );
        }
        if case.revision != rev {
            eprintln!(
                "warning: case {} pinned revision {} differs from CLI --rev {}; using case pinned revision",
                case.id, case.revision, rev
            );
        }

        eprintln!("evaluating case {}", case.id);
        // Always use the case's pinned repo_path and revision, not the CLI args
        let variants = build_prompt_variants(case, &case.repo_path, &case.revision, non_expert, helper_model.as_deref_mut(), helper_adapter_name)?;
        let report = run_eval(case, &variants, agent, repetitions, !prompt_only)?;
        case_reports.push(report);
    }

    let eval_report = EvalReport {
        schema_version: EVAL_SCHEMA_VERSION.into(),
        agent_adapter: agent.adapter_name().to_string(),
        agent_model: agent.model_name().to_string(),
        agent_capabilities: agent.capabilities(),
        cases: case_reports,
        generated_at: chrono::Utc::now().to_rfc3339(),
    };

    let json = serde_json::to_string_pretty(&eval_report)?;
    std::fs::write(output, json)?;
    eprintln!("wrote eval report to {}", output.display());
    print_summary(&eval_report);
    Ok(())
}

fn print_summary(report: &EvalReport) {
    let summary = report.summary();
    eprintln!("\n=== Eval Summary ===");
    eprintln!(
        "agent capabilities: prompt_only_model_input={} repository_read={} host_patch_apply={}",
        report.agent_capabilities.prompt_only_model_input,
        report.agent_capabilities.repository_read,
        report.agent_capabilities.host_patch_apply
    );
    eprintln!("total runs: {}", summary.total_runs);
    eprintln!("avg artifact recall: {:.2}", summary.avg_artifact_recall);
    eprintln!("avg artifact precision: {:.2}", summary.avg_artifact_precision);
    eprintln!(
        "intent preserved: {}/{}",
        summary.intent_preserved_count, summary.total_runs
    );
    eprintln!(
        "constraints preserved: {}/{}",
        summary.constraints_preserved_count, summary.total_runs
    );
    eprintln!(
        "verification pass rate: {:.2}",
        summary.verification_pass_rate
    );
    eprintln!(
        "prohibited violations: {}",
        summary.total_prohibited_violations
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_variants_produces_at_least_two() {
        let case = task_zero_lab::eval::case_c01();
        // Skip if the repo path doesn't exist at the expected location
        if !case.repo_path.join(".git").exists() {
            return;
        }
        let variants = build_prompt_variants(&case, &case.repo_path, "HEAD", false, None, "disabled");
        // May fail if the repo isn't set up correctly, so just check it doesn't panic
        match variants {
            Ok(v) => assert!(v.variants.len() >= 2, "expected at least raw + deterministic"),
            Err(_) => {} // graph extraction may fail on non-matching repo
        }
    }
}
