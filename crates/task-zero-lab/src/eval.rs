//! Epic 014 Task 6: replay, baselines, ablations, and prompt assessment.
//!
//! This module defines the evaluation framework for comparing prompt variants
//! (raw human, deterministic generated, helper-refined) against a fixed
//! capable-model configuration, scoring both prompt quality and task outcome
//! independently (Design Constraint 7).
//!
//! ## Design
//!
//! An [`EvalCase`] pairs a request with its known practical relevance set
//! (from `manifest.md` §3). A [`PromptVariant`] selects how the prompt is
//! generated. An [`EvalRun`] records one agent execution in a disposable
//! worktree. An [`EvalScore`] measures both prompt quality and task outcome.
//!
//! The orchestrator flow is:
//! 1. Create a disposable worktree at the case's pinned revision
//! 2. Generate the prompt (variant-dependent)
//! 3. Hand the prompt to a coding agent
//! 4. Capture the resulting diff
//! 5. Score against the practical relevance set
//! 6. Repeat (each case-variant pair runs more than once)

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::prompt::MutationBoundary;

pub const EVAL_SCHEMA_VERSION: &str = "task-zero-eval-v2";

// ── Practical relevance set ────────────────────────────────────────────

/// A file path that the coding agent is expected to touch (or not touch)
/// for a given case, with optional expected blob hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedArtifact {
    /// Repository-relative path.
    pub path: String,
    /// Whether this file should be modified (`true`) or must remain
    /// untouched (`false` — a negative constraint).
    pub should_change: bool,
    /// Optional expected blob SHA after the change (None = any change is OK).
    pub expected_blob_hash: Option<String>,
    /// Human-readable note about why this artifact is in the set.
    pub note: String,
}

/// The known practical relevance set for an eval case, established by
/// historical `git diff`/`git show` comparison (manifest.md §3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PracticalRelevanceSet {
    /// Files expected to change (positive evidence).
    pub expected_changes: Vec<ExpectedArtifact>,
    /// Files that must NOT change (negative constraints).
    pub prohibited_changes: Vec<ExpectedArtifact>,
    /// Verification commands derived from exact repository instructions
    /// (not language-name guessing). E.g. `["cargo check --workspace"]`.
    pub verification_commands: Vec<String>,
    /// The commit(s) whose diff established this relevance set.
    pub evidence_commits: Vec<String>,
    /// Source experiment that established this set.
    pub evidence_source: String,
}

// ── Eval case ──────────────────────────────────────────────────────────

/// A single evaluation case, pairing a request with its relevance set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    /// Case identifier (e.g. "C01", "C07").
    pub id: String,
    /// Repository local path.
    pub repo_path: PathBuf,
    /// Immutable revision to check out for the worktree.
    pub revision: String,
    /// The expert rendering of the request.
    pub expert_request: String,
    /// The non-expert rendering of the request (NEW, from manifest §3).
    pub non_expert_request: Option<String>,
    /// Negative constraints from the manifest.
    pub negative_constraints: Vec<String>,
    /// Mutation boundary.
    pub mutation_boundary: MutationBoundary,
    /// Epics to load for graph extraction.
    pub epics: Vec<u32>,
    /// Known practical relevance set.
    pub relevance: PracticalRelevanceSet,
    /// Rendering identity for provenance.
    pub rendering_id: String,
}

// ── Prompt variant ─────────────────────────────────────────────────────

/// How the prompt handed to the coding agent is generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptVariantKind {
    /// The raw human request, no graph/packet/prompt rendering.
    RawHuman,
    /// The Task 4 deterministic baseline prompt (graph → packet → render).
    DeterministicBaseline,
    /// The Task 5 helper-refined prompt (graph → packet → render → helper).
    HelperRefined,
}

/// A prompt variant with its rendered text and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptVariant {
    pub kind: PromptVariantKind,
    pub text: String,
    pub byte_count: usize,
    pub token_estimate: usize,
    /// For helper-refined: the helper run report.
    pub helper_report_json: Option<String>,
}

// ── Agent result ───────────────────────────────────────────────────────

/// The result of handing a prompt to a coding agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    /// The agent's text response.
    pub response_text: String,
    /// Files the agent touched (relative paths), if detectable.
    pub touched_files: Vec<String>,
    /// Unified diff produced by the agent, if available.
    pub diff: Option<String>,
    /// Input tokens consumed.
    pub input_tokens: u64,
    /// Output tokens produced.
    pub output_tokens: u64,
    /// Wall-clock elapsed time in milliseconds.
    pub elapsed_ms: u128,
    /// Stop reason from the model.
    pub stop_reason: Option<String>,
    /// Adapter/provider diagnostics.
    pub diagnostics: Vec<String>,
}

// ── Scoring ────────────────────────────────────────────────────────────

/// Prompt-quality score (checkable from prompt text alone, Design
/// Constraint 7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptQualityScore {
    /// Whether original intent text survives in the prompt.
    pub intent_preserved: bool,
    /// Whether negative constraints survive in the prompt.
    pub constraints_preserved: bool,
    /// Whether provenance labels are present.
    pub has_provenance_labels: bool,
    /// Whether coverage/gap disclosure is present.
    pub has_gap_disclosure: bool,
    /// Prompt byte count.
    pub prompt_bytes: usize,
    /// Estimated token count.
    pub token_estimate: usize,
    /// Context items omitted due to budget.
    pub omitted_context: Vec<String>,
}

/// Task-outcome score (measured from the agent's diff in the worktree,
/// independent of prompt quality).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutcomeScore {
    /// Fraction of expected-changed files actually touched by the agent.
    pub artifact_recall: f64,
    /// Fraction of touched files that are in the expected set.
    pub artifact_precision: f64,
    /// Whether the agent touched any prohibited files.
    pub violated_prohibitions: Vec<String>,
    /// Whether verification commands pass in the worktree.
    pub verification_passed: Option<bool>,
    /// Which verification commands were run.
    pub verification_commands_run: Vec<String>,
    /// Stdout/stderr diagnostics from verification command execution.
    pub verification_diagnostics: Vec<String>,
    /// Unsupported claims: statements in the agent response not traceable
    /// to established evidence. (Manual or heuristic count.)
    pub unsupported_claims: usize,
    /// Whether stated negative constraints are respected in the actual diff.
    pub negative_constraints_respected: bool,
}

/// Combined score for one eval run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalScore {
    pub prompt_quality: PromptQualityScore,
    pub task_outcome: TaskOutcomeScore,
}

// ── Eval run ───────────────────────────────────────────────────────────

/// One complete eval run: case + variant + agent execution + scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalRun {
    pub schema_version: String,
    pub case_id: String,
    pub rendering_id: String,
    pub variant: PromptVariant,
    pub agent_adapter: String,
    pub agent_model: String,
    pub repetition: u32,
    pub worktree_commit: String,
    pub worktree_path: Option<PathBuf>,
    /// Ground-truth unified diff read from the worktree via
    /// `EvalWorktree::capture_diff()` before the worktree guard removed it.
    /// `None` in prompt-only mode (no worktree was created) or if the diff
    /// capture itself failed. This is the diff actually on disk, not the
    /// model's self-reported `agent_result.diff`.
    pub worktree_diff: Option<String>,
    /// Ground-truth changed-file list read from the worktree via
    /// `EvalWorktree::changed_files()` before the worktree guard removed it.
    /// Empty in prompt-only mode. This is what `score.task_outcome`'s
    /// artifact recall/precision/violated_prohibitions are computed from —
    /// not `agent_result.touched_files`, which is a model self-report.
    pub worktree_changed_files: Vec<String>,
    pub agent_result: AgentResult,
    pub score: EvalScore,
    pub graph_hash: String,
    pub packet_hash: String,
    pub prompt_template_version: String,
    pub eval_timestamp: String,
}

// ── Report ─────────────────────────────────────────────────────────────

/// Comparison report across all runs for one case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseReport {
    pub case_id: String,
    pub runs: Vec<EvalRun>,
}

/// Full evaluation report across all cases and variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub schema_version: String,
    pub agent_adapter: String,
    pub agent_model: String,
    pub cases: Vec<CaseReport>,
    pub generated_at: String,
}

impl EvalReport {
    /// Summarize average scores across runs.
    pub fn summary(&self) -> EvalSummary {
        let mut total_runs = 0usize;
        let mut total_recall = 0.0f64;
        let mut total_precision = 0.0f64;
        let mut total_intent = 0usize;
        let mut total_constraints = 0usize;
        let mut total_verification_pass = 0usize;
        let mut total_verification_runs = 0usize;
        let mut total_prohibited_violations = 0usize;

        for case in &self.cases {
            for run in &case.runs {
                total_runs += 1;
                total_recall += run.score.task_outcome.artifact_recall;
                total_precision += run.score.task_outcome.artifact_precision;
                if run.score.prompt_quality.intent_preserved {
                    total_intent += 1;
                }
                if run.score.prompt_quality.constraints_preserved {
                    total_constraints += 1;
                }
                if let Some(passed) = run.score.task_outcome.verification_passed {
                    total_verification_runs += 1;
                    if passed {
                        total_verification_pass += 1;
                    }
                }
                total_prohibited_violations +=
                    run.score.task_outcome.violated_prohibitions.len();
            }
        }

        EvalSummary {
            total_runs,
            avg_artifact_recall: if total_runs > 0 {
                total_recall / total_runs as f64
            } else {
                0.0
            },
            avg_artifact_precision: if total_runs > 0 {
                total_precision / total_runs as f64
            } else {
                0.0
            },
            intent_preserved_count: total_intent,
            constraints_preserved_count: total_constraints,
            verification_pass_rate: if total_verification_runs > 0 {
                total_verification_pass as f64 / total_verification_runs as f64
            } else {
                0.0
            },
            total_prohibited_violations,
        }
    }
}

/// Aggregate summary across all eval runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSummary {
    pub total_runs: usize,
    pub avg_artifact_recall: f64,
    pub avg_artifact_precision: f64,
    pub intent_preserved_count: usize,
    pub constraints_preserved_count: usize,
    pub verification_pass_rate: f64,
    pub total_prohibited_violations: usize,
}

// ── Case fixtures ──────────────────────────────────────────────────────

/// Build the C01 eval case from manifest §3.
pub fn case_c01() -> EvalCase {
    EvalCase {
        id: "C01".into(),
        // akar repo path — the case's pinned revision and expected artifact
        // (Epic 020) live in akar, not daftprompt; see manifest.md "C01 —
        // akar: cross-model review of Epic 020" and the 2026-08-14
        // Experiment Note for the corrected class of this bug (mirrors the
        // case_c07() fix).
        repo_path: std::path::PathBuf::from("/Users/brainless/Projects/akar"),
        revision: "febfa42e747ee6f5b64f7c2f0549f9b82d1babaa".into(),
        expert_request: "Analyze Epic 020 and suggest changes before implementation.".into(),
        non_expert_request: Some("Can someone check epic 020 is okay before we build it?".into()),
        negative_constraints: vec![
            "Review phase is read-only; revision phase is explicitly authorized by a follow-up request, not implied by the review.".into(),
        ],
        mutation_boundary: MutationBoundary::ReadOnly,
        epics: vec![11, 12, 13, 14],
        relevance: PracticalRelevanceSet {
            expected_changes: vec![
                ExpectedArtifact {
                    path: "epics/020-*.md".into(),
                    should_change: true,
                    expected_blob_hash: Some(
                        "49878280500dfdbb61a707ae4879723369c13848".into(),
                    ),
                    note: "Epic 020 revised blob incorporating review findings".into(),
                },
            ],
            prohibited_changes: vec![],
            verification_commands: vec!["cargo check --workspace".into()],
            evidence_commits: vec!["9334431bbb37dd0db20f4b80c660387c62cf34bc".into()],
            evidence_source: "011 Experiment 1, deterministic evidence review".into(),
        },
        rendering_id: "manifest:C01:expert".into(),
    }
}

/// Build the C02 eval case from manifest §3 (Keystone LOG-019, sanitized).
/// Note: Keystone is a private repository. This fixture uses sanitized
/// behavioral content only — no private raw content is included.
pub fn case_c02() -> EvalCase {
    EvalCase {
        id: "C02".into(),
        // Keystone repo path — may not exist locally; fixture designed for
        // scripted/deterministic testing when the repo is unavailable.
        repo_path: std::path::PathBuf::from("/Users/brainless/Projects/Keystone"),
        revision: "5f4562688fd7dcf2101b88cf05c7d6e4bf0d22a4".into(),
        expert_request:
            "Examine LOG-019: when an admin filters the review queue, exporting produces an empty file. \
             Expected the export to contain the filtered rows. Cross-check the PRD and earlier decisions, \
             determine relevant implementation and tests, and decide whether to implement."
                .into(),
        non_expert_request: Some(
            "Export doesn't work right on the filtered queue, can you look into LOG-019?".into(),
        ),
        negative_constraints: vec![
            "Read-only investigation; no mutation is authorized until a human/product decision selects an implementation path.".into(),
        ],
        mutation_boundary: MutationBoundary::ReadOnly,
        epics: vec![11, 12, 13, 14],
        relevance: PracticalRelevanceSet {
            expected_changes: vec![],
            prohibited_changes: vec![],
            verification_commands: vec![],
            evidence_commits: vec![
                "91ab196".into(),
                "c06a7ad".into(),
                "ce96e3a".into(),
            ],
            evidence_source: "011 Experiment 2, Real Keystone LOG-019 replay".into(),
        },
        rendering_id: "manifest:C02:expert".into(),
    }
}

/// Build the C07 eval case from manifest §3.
pub fn case_c07() -> EvalCase {
    EvalCase {
        id: "C07".into(),
        // dwata repo path — may not exist locally; fixture designed for
        // scripted/deterministic testing when the repo is unavailable.
        repo_path: std::path::PathBuf::from("/Users/brainless/Projects/dwata"),
        // dwata has no single recorded "pre-session" checkout SHA (manifest
        // §2: reconstructed pre-session state), so `11d98e0` — the direct
        // parent of fix commit `8165cd2` — is used as the pre-fix revision
        // to check the worktree out at. It is a reasonable reconstruction,
        // not a uniquely canonical one: it is simply the last commit before
        // the fix landed. `8165cd2` itself is the fix and must not be used
        // as the checkout revision, or there would be nothing left to fix.
        revision: "11d98e0".into(),
        expert_request:
            "Fix the Unicode byte-boundary panic in email ranking at `email_ranking/mod.rs::contains_date`."
                .into(),
        non_expert_request: Some(
            "The email ranking thing crashes on some emails, can you fix it?".into(),
        ),
        negative_constraints: vec![
            "Fix plus regression test; scope explicitly excludes an unrelated GUI date-request change that shares the same commit.".into(),
        ],
        mutation_boundary: MutationBoundary::RepositoryChangesOnlyWhenExplicitlyRequested,
        epics: vec![11, 12, 13, 14],
        relevance: PracticalRelevanceSet {
            expected_changes: vec![
                ExpectedArtifact {
                    path: "dwata-api/src/email_ranking/mod.rs".into(),
                    should_change: true,
                    expected_blob_hash: None,
                    note: "Replace byte slicing with character iteration at reported byte offsets".into(),
                },
            ],
            prohibited_changes: vec![
                ExpectedArtifact {
                    path: "gui/".into(),
                    should_change: false,
                    expected_blob_hash: None,
                    note: "Unrelated GUI date-request change in the same commit must not be touched".into(),
                },
            ],
            // dwata's nightly pin (`rust-toolchain.toml`) is an untracked
            // local file on the machine this fixture was authored on — git
            // worktrees never inherit untracked files, so any disposable
            // worktree of dwata needs nightly requested explicitly or the
            // machine's global `~/.cargo/config.toml` (`codegen-backend =
            // "cranelift"`, an unstable/nightly-only feature) fails the
            // build regardless of patch quality. Verified directly: `cargo
            // +nightly check --workspace` succeeds cleanly at `11d98e0` in a
            // real sibling worktree; plain `cargo check --workspace` does
            // not.
            verification_commands: vec!["cargo +nightly check --workspace".into()],
            evidence_commits: vec!["8165cd2".into()],
            evidence_source: "011 Experiment 4D".into(),
        },
        rendering_id: "manifest:C07:expert".into(),
    }
}

/// All available eval cases.
pub fn all_cases() -> Vec<EvalCase> {
    vec![case_c01(), case_c02(), case_c07()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_schema_version_is_set() {
        assert_eq!(EVAL_SCHEMA_VERSION, "task-zero-eval-v2");
    }

    #[test]
    fn c01_case_fixture_has_required_fields() {
        let c = case_c01();
        assert_eq!(c.id, "C01");
        assert!(!c.expert_request.is_empty());
        assert!(c.non_expert_request.is_some());
        assert!(!c.negative_constraints.is_empty());
        assert_eq!(c.mutation_boundary, MutationBoundary::ReadOnly);
        assert!(!c.relevance.expected_changes.is_empty());
        assert!(!c.relevance.evidence_commits.is_empty());
    }

    #[test]
    fn c07_case_fixture_has_required_fields() {
        let c = case_c07();
        assert_eq!(c.id, "C07");
        assert!(c.non_expert_request.is_some());
        assert!(!c.relevance.prohibited_changes.is_empty());
        assert_eq!(
            c.mutation_boundary,
            MutationBoundary::RepositoryChangesOnlyWhenExplicitlyRequested
        );
    }

    #[test]
    fn c02_case_fixture_has_required_fields() {
        let c = case_c02();
        assert_eq!(c.id, "C02");
        assert!(!c.expert_request.is_empty());
        assert!(c.non_expert_request.is_some());
        assert!(!c.negative_constraints.is_empty());
        assert_eq!(c.mutation_boundary, MutationBoundary::ReadOnly);
        // C02 is read-only investigation, no expected changes
        assert!(c.relevance.expected_changes.is_empty());
        assert!(!c.relevance.evidence_commits.is_empty());
    }

    #[test]
    fn prompt_variant_kinds_cover_required_spectrum() {
        let kinds = [
            PromptVariantKind::RawHuman,
            PromptVariantKind::DeterministicBaseline,
            PromptVariantKind::HelperRefined,
        ];
        assert_eq!(kinds.len(), 3);
    }

    #[test]
    fn eval_report_summary_handles_empty() {
        let report = EvalReport {
            schema_version: EVAL_SCHEMA_VERSION.into(),
            agent_adapter: "test".into(),
            agent_model: "test".into(),
            cases: vec![],
            generated_at: "2026-01-01T00:00:00Z".into(),
        };
        let summary = report.summary();
        assert_eq!(summary.total_runs, 0);
        assert_eq!(summary.avg_artifact_recall, 0.0);
    }
}
