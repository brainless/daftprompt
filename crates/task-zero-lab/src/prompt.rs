//! Epic 014 Task 4: deterministic, model-free coding-agent prompt renderer.
//!
//! This is an export/evaluation surface, not an authorization or dispatch
//! mechanism. It consumes only the original request and Task 3 packet; it
//! performs no repository reads, network calls, model calls, or tool calls.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::packet::{Packet, PacketItem};

pub const PROMPT_TEMPLATE_VERSION: &str = "task-zero-baseline-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationBoundary {
    ReadOnly,
    RepositoryChangesOnlyWhenExplicitlyRequested,
}

impl MutationBoundary {
    fn text(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read-only: do not modify repository files or external state.",
            Self::RepositoryChangesOnlyWhenExplicitlyRequested => {
                "Repository changes are authorized only when the original request explicitly asks for them. Never dispatch another coding agent or mutate external state."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptRequest {
    pub original: String,
    pub clarified_objective: Option<String>,
    pub negative_constraints: Vec<String>,
    pub mutation_boundary: MutationBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptBudget {
    pub max_bytes: usize,
    pub bytes_per_token_estimate: usize,
}

impl Default for PromptBudget {
    fn default() -> Self {
        Self {
            max_bytes: 24_000,
            bytes_per_token_estimate: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedPrompt {
    pub text: String,
    pub byte_count: usize,
    pub token_estimate: usize,
    pub omitted_context: Vec<String>,
}

fn provenance(item: &PacketItem, prefix: char, index: usize) -> String {
    format!(
        "[{prefix}{index}] {} @ {} ({})",
        item.source_path.as_deref().unwrap_or("no-source-path"),
        item.source_version,
        item.locator
    )
}

fn push_section(out: &mut String, title: &str, body: &str) {
    out.push_str("## ");
    out.push_str(title);
    out.push_str("\n\n");
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
}

fn add_optional(out: &mut String, addition: &str, max_bytes: usize) -> bool {
    if out.len() + addition.len() <= max_bytes {
        out.push_str(addition);
        true
    } else {
        false
    }
}

fn established_admission_order(packet: &Packet) -> Vec<(usize, &PacketItem)> {
    let mut buckets: BTreeMap<String, Vec<(usize, &PacketItem)>> = BTreeMap::new();
    for (index, item) in packet.established.iter().enumerate() {
        let bucket = item
            .locator
            .strip_prefix("epic:")
            .and_then(|rest| rest.split('/').next())
            .map(|epic| format!("epic:{epic}"))
            .unwrap_or_else(|| format!("source:{:?}:{}", item.category, item.source_path.as_deref().unwrap_or("none")));
        buckets.entry(bucket).or_default().push((index, item));
    }
    for items in buckets.values_mut() {
        items.sort_by_key(|(_, item)| {
            let task_zero = item.locator.contains("/task:0");
            let rank = match item.kind {
                crate::graph::NodeKind::Task if task_zero => 0,
                crate::graph::NodeKind::AcceptanceCriterion if task_zero => 1,
                crate::graph::NodeKind::UnresolvedGap if task_zero => 2,
                crate::graph::NodeKind::ResearchArtifact => 3,
                crate::graph::NodeKind::DesignConstraint => 4,
                _ => 5,
            };
            (rank, item.locator.as_str())
        });
    }
    let mut ordered = Vec::new();
    let max_bucket = buckets.values().map(Vec::len).max().unwrap_or(0);
    for round in 0..max_bucket {
        for items in buckets.values() {
            if let Some(item) = items.get(round) {
                ordered.push(*item);
            }
        }
    }
    ordered
}

/// Task 2 intentionally extracts command-looking fenced lines broadly. A
/// prompt may expose a line as verification only when it also has a
/// conservative executable shape; diagrams, options, and prose remain packet
/// evidence but never become commands.
fn is_executable_verification_command(command: &str) -> bool {
    let command = command.trim();
    if command.is_empty()
        || command.contains('\n')
        || command.starts_with('-')
        || command.starts_with("->")
        || command.contains('→')
        || command.ends_with(':')
        || [" from ", " via ", " should ", " then "]
            .iter()
            .any(|prose| command.to_ascii_lowercase().contains(prose))
    {
        return false;
    }
    let mut words = command.strip_prefix("$ ").unwrap_or(command).split_whitespace();
    let mut executable = words.next().unwrap_or_default();
    while executable.contains('=') && !executable.starts_with("./") {
        let Some(next) = words.next() else { return false };
        executable = next;
    }
    let basename = executable.rsplit('/').next().unwrap_or(executable);
    matches!(
        basename,
        "cargo" | "git" | "rg" | "rustfmt" | "npm" | "pnpm" | "yarn" | "bun" | "npx" | "node" | "deno"
            | "python" | "python3" | "pytest" | "make" | "cmake" | "just" | "bash" | "sh" | "zsh"
    ) || executable.starts_with("./")
}

/// Render a byte-stable prompt. Lower-ranked packet items are admitted one at
/// a time and named in the omissions section when the hard budget excludes
/// them. The fixed envelope must fit; silently truncating intent or coverage
/// would violate the prompt contract.
pub fn render_prompt(request: &PromptRequest, packet: &Packet, budget: PromptBudget) -> anyhow::Result<RenderedPrompt> {
    anyhow::ensure!(budget.max_bytes > 0, "prompt max_bytes must be greater than zero");
    anyhow::ensure!(
        budget.bytes_per_token_estimate > 0,
        "bytes_per_token_estimate must be greater than zero"
    );

    let mut out = format!("# Coding-agent handoff\n\nTemplate: `{PROMPT_TEMPLATE_VERSION}`\n\n");
    let immutable = format!(
        "The text below is immutable human input. Do not rewrite it or infer additional authority.\n\n### Original request (verbatim)\n\n```text\n{}\n```\n\n### Negative constraints (verbatim)\n\n{}\n\n### Mutation boundary\n\n{}\n",
        request.original,
        if request.negative_constraints.is_empty() { "- None supplied.".to_string() } else { request.negative_constraints.iter().map(|c| format!("- `{c}`")).collect::<Vec<_>>().join("\n") },
        request.mutation_boundary.text()
    );
    push_section(&mut out, "Immutable intent and boundaries", &immutable);
    let objective = request.clarified_objective.as_deref().unwrap_or(&request.original);
    push_section(
        &mut out,
        "Clarified objective",
        &format!("{objective}\n\nVerify this interpretation against the immutable request before acting."),
    );

    let mut coverage = format!(
        "- [RUN] Repository: packet-bound snapshot (canonical path is intentionally not re-read by the renderer).\n- [RUN] Resolved commit: `{}`.\n- [RUN] Lab version: `{}`; prompt template: `{PROMPT_TEMPLATE_VERSION}`.\n- [RUN] Index available: `{}`; canonical identifiers considered: `{}`.\n- [RUN] Queried source categories: `{:?}`.\n- [RUN] Unavailable source categories: `{:?}`.\n",
        packet.coverage.graph_resolved_commit, packet.coverage.lab_version,
        packet.coverage.index_available, packet.coverage.index_identifiers_considered,
        packet.coverage.queried_sources, packet.coverage.unavailable_sources
    );
    if packet.coverage.index_available {
        coverage.push_str("- Staleness warning: the packet does not prove that the index was built from the resolved commit; treat index matches as coverage hints, not pinned repository truth.\n");
    } else {
        coverage.push_str("- Coverage warning: no index was available; indexed code/document coverage is missing.\n");
    }
    push_section(&mut out, "Repository snapshot and coverage", &coverage);

    push_section(&mut out, "Relevant epic, task, and shared constraints", "The established evidence below is the bounded packet selection. Checked state is shown when recorded; absence from this packet is not evidence of irrelevance.");
    let mut omitted = Vec::new();
    // Reserve room for the remaining mandatory contract sections. Context
    // is the only rankable/omittable part of the artifact.
    let context_limit = budget.max_bytes.saturating_sub(9_500);
    for (i, item) in established_admission_order(packet) {
        let block = format!(
            "- **{}** `{:?}` — {} Checked: `{:?}`. Excerpt: {:?}\n",
            provenance(item, 'E', i + 1),
            item.kind,
            item.label,
            item.checked,
            item.excerpt
        );
        if !add_optional(&mut out, &block, context_limit) {
            omitted.push(format!("{}: established item omitted by prompt byte budget", item.locator));
        }
    }
    out.push('\n');
    push_section(
        &mut out,
        "Candidate context (not established)",
        "The following are retrieval candidates, not repository facts. Verify them at the pinned revision before relying on them.",
    );
    for (i, item) in packet.candidates.iter().enumerate() {
        let block = format!(
            "- **{}** suggestion/candidate only — label {:?}; excerpt {:?}\n",
            provenance(item, 'C', i + 1),
            item.label,
            item.excerpt
        );
        if !add_optional(&mut out, &block, context_limit) {
            omitted.push(format!("{}: candidate item omitted by prompt byte budget", item.locator));
        }
    }
    out.push('\n');

    let gaps = if packet.coverage.remaining_gaps.is_empty() {
        "- No host-recorded reachable gaps; this does not prove completeness.".to_string()
    } else {
        let mut shown = packet
            .coverage
            .remaining_gaps
            .iter()
            .take(30)
            .map(|g| format!("- `{g}` (host-recorded unresolved gap)"))
            .collect::<Vec<_>>();
        if packet.coverage.remaining_gaps.len() > shown.len() {
            shown.push(format!(
                "- … {} additional host-recorded gaps omitted from prose by the prompt budget.",
                packet.coverage.remaining_gaps.len() - shown.len()
            ));
        }
        shown.join("\n")
    };
    push_section(&mut out, "Unresolved Task 0 criteria, gaps, and human decisions", &gaps);
    push_section(&mut out, "Suggested actions and dependencies", "These are bounded suggestions, not repository claims or expanded authorization:\n\n1. Re-open cited sources at the resolved commit and verify every assumption.\n2. Address unchecked criteria in dependency order while preserving the immutable constraints.\n3. Stop and ask the human when evidence leaves a product or authorization decision unresolved.");

    let paths: BTreeSet<_> = packet.established.iter().filter_map(|i| i.source_path.as_deref()).collect();
    let surface = if paths.is_empty() {
        "- No evidence-backed change surface was identified. Do not invent file ownership.".to_string()
    } else {
        paths.iter().take(20).filter_map(|p| {
            packet.established.iter().enumerate().find(|(_, item)| item.source_path.as_deref() == Some(*p))
                .map(|(index, item)| format!("- `{p}` — likely relevance only; no ownership claim. {}", provenance(item, 'E', index + 1)))
        })
            .collect::<Vec<_>>()
            .join("\n")
    };
    push_section(&mut out, "Likely change surface", &surface);
    push_section(&mut out, "Required artifacts and output contract", "Return a focused result that maps work to the requested criteria, reports files changed, lists verification performed and failures, and separates completed work from remaining gaps. Prompt quality is not task-outcome evidence; downstream correctness must be assessed independently.");

    let mut commands = BTreeSet::new();
    // Candidate items are deliberately not established repository evidence.
    // Even an executable-looking command from a candidate must remain in the
    // candidate section until its relevance is verified at the pinned revision.
    for item in &packet.established {
        for command in &item.verification_commands {
            if is_executable_verification_command(command) {
                commands.insert((command, item));
            }
        }
    }
    let verify = if commands.is_empty() {
        "- No exact verification command survived in packet evidence. Do not guess from the language; inspect repository instructions/configuration and report this gap.".to_string()
    } else {
        commands
            .into_iter()
            .take(20)
            .map(|(c, item)| format!("- `{c}` — derived exactly from {}", provenance(item, 'V', 1)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    push_section(&mut out, "Project-derived verification", &verify);

    omitted.extend(packet.coverage.budget_omissions.iter().map(|o| format!("packet omission: {o}")));
    omitted.extend(
        packet
            .coverage
            .unavailable_sources
            .iter()
            .map(|s| format!("unavailable source category: {s:?}")),
    );
    omitted.sort();
    omitted.dedup();
    let omissions = if omitted.is_empty() {
        "- None recorded.".to_string()
    } else {
        let mut shown = omitted.iter().take(30).map(|o| format!("- {o}")).collect::<Vec<_>>();
        if omitted.len() > shown.len() {
            shown.push(format!(
                "- … {} additional deterministic omissions are recorded in the returned `RenderedPrompt.omitted_context` metadata.",
                omitted.len() - shown.len()
            ));
        }
        shown.join("\n")
    };
    let footer_reserve = 160 + omissions.len();
    anyhow::ensure!(
        out.len() + footer_reserve <= budget.max_bytes,
        "prompt budget {} bytes cannot fit immutable intent and required disclosure envelope (needs at least {})",
        budget.max_bytes,
        out.len() + footer_reserve
    );
    push_section(&mut out, "Omissions, stale sources, and unsupported capabilities", &format!("{omissions}\n- No model, credentials, network, semantic retrieval, repository mutation, or coding-agent dispatch was used by this renderer."));
    out.push_str(&format!(
        "Prompt budget: at most {} bytes; token estimate uses {} bytes/token.\n",
        budget.max_bytes, budget.bytes_per_token_estimate
    ));
    anyhow::ensure!(out.len() <= budget.max_bytes, "internal prompt budget accounting error");
    let byte_count = out.len();
    Ok(RenderedPrompt {
        text: out,
        byte_count,
        token_estimate: byte_count.div_ceil(budget.bytes_per_token_estimate),
        omitted_context: omitted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::NodeKind;
    use crate::packet::{PacketBudget, PacketCoverage, PacketItem, SourceCategory};

    fn item(locator: &str, label: &str, command: Option<&str>) -> PacketItem {
        PacketItem {
            locator: locator.into(),
            kind: NodeKind::Task,
            label: label.into(),
            source_path: Some("epics/014.md".into()),
            source_version: "abc123".into(),
            checked: Some(false),
            category: Some(SourceCategory::EpicsAndResearch),
            excerpt: label.into(),
            verification_commands: command.into_iter().map(str::to_string).collect(),
            found_via: vec![],
            merged_from: vec![],
        }
    }
    fn packet() -> Packet {
        Packet {
            request: "continue closing Task 0 blockers".into(),
            budget: PacketBudget::default(),
            seeds: vec![],
            established: vec![item("epic:011/task:0", "Task 0 criterion", Some("cargo test --workspace"))],
            candidates: vec![item("candidate:x", "possible file", None)],
            rejected_helper_interpretations: vec![],
            helper_gap_notes: vec![],
            coverage: PacketCoverage {
                queried_sources: vec![SourceCategory::EpicsAndResearch],
                unavailable_sources: vec![SourceCategory::Code],
                lab_version: "0.1.0".into(),
                graph_resolved_commit: "abc123".into(),
                index_available: true,
                index_identifiers_considered: 12,
                budget_omissions: vec![],
                remaining_gaps: vec!["gap:review".into()],
            },
        }
    }
    fn request(boundary: MutationBoundary) -> PromptRequest {
        PromptRequest {
            original: "Review only; do not edit.".into(),
            clarified_objective: Some("Review Task 0 evidence".into()),
            negative_constraints: vec!["do not edit".into()],
            mutation_boundary: boundary,
        }
    }

    #[test]
    fn deterministic_and_contract_complete() {
        let a = render_prompt(&request(MutationBoundary::ReadOnly), &packet(), PromptBudget::default()).unwrap();
        let b = render_prompt(&request(MutationBoundary::ReadOnly), &packet(), PromptBudget::default()).unwrap();
        assert_eq!(a, b);
        for heading in [
            "Immutable intent",
            "Clarified objective",
            "Repository snapshot",
            "Relevant epic",
            "Candidate context",
            "Unresolved Task 0",
            "Suggested actions",
            "Likely change surface",
            "Required artifacts",
            "Project-derived verification",
            "Omissions",
        ] {
            assert!(a.text.contains(heading), "missing {heading}");
        }
        assert!(a.text.contains("Review only; do not edit."));
        assert!(a.text.contains("[E1]"));
        assert!(a.text.contains("suggestion/candidate only"));
        assert!(a.text.contains("Staleness warning"));
        assert!(a.text.contains("`cargo test --workspace`"));
    }

    #[test]
    fn tight_budget_discloses_lower_ranked_omissions_or_errors_honestly() {
        let full = render_prompt(&request(MutationBoundary::ReadOnly), &packet(), PromptBudget::default()).unwrap();
        let budget = PromptBudget {
            max_bytes: full.byte_count - 100,
            bytes_per_token_estimate: 4,
        };
        match render_prompt(&request(MutationBoundary::ReadOnly), &packet(), budget) {
            Ok(rendered) => {
                assert!(rendered.byte_count <= budget.max_bytes);
                assert!(!rendered.omitted_context.is_empty());
            }
            Err(e) => assert!(e.to_string().contains("cannot fit immutable intent")),
        }
    }

    #[test]
    fn verification_is_never_language_guessed() {
        let mut p = packet();
        p.established[0].verification_commands.clear();
        let rendered = render_prompt(&request(MutationBoundary::ReadOnly), &p, PromptBudget::default()).unwrap();
        assert!(rendered.text.contains("Do not guess from the language"));
        assert!(!rendered.text.contains("`cargo test --workspace`"));
    }

    #[test]
    fn candidate_commands_never_become_authoritative_verification() {
        let mut p = packet();
        p.established[0].verification_commands.clear();
        p.candidates[0].verification_commands = vec!["cargo test --workspace".into()];

        let rendered = render_prompt(&request(MutationBoundary::ReadOnly), &p, PromptBudget::default()).unwrap();
        let verification = rendered
            .text
            .split("## Project-derived verification")
            .nth(1)
            .unwrap()
            .split("## Omissions")
            .next()
            .unwrap();

        assert!(verification.contains("No exact verification command survived in packet evidence"));
        assert!(!verification.contains("`cargo test --workspace`"));
    }

    #[test]
    fn diagram_and_option_false_positives_never_render_as_verification_commands() {
        let mut p = packet();
        p.established[0].verification_commands = vec![
            "-> helper request refinement".into(),
            "--changes-with-copies-source".into(),
            "request -> packet -> prompt".into(),
            "pytest from pyproject.toml".into(),
            "cargo check --workspace".into(),
        ];
        let rendered = render_prompt(&request(MutationBoundary::ReadOnly), &p, PromptBudget::default()).unwrap();
        let verification = rendered.text.split("## Project-derived verification").nth(1).unwrap().split("## Omissions").next().unwrap();
        assert!(verification.contains("`cargo check --workspace`"));
        assert!(!verification.contains("helper request refinement"));
        assert!(!verification.contains("--changes-with-copies-source"));
        assert!(!verification.contains("request -> packet"));
        assert!(!verification.contains("pytest from pyproject.toml"));
    }

    #[test]
    fn established_budget_admission_is_stratified_across_epics() {
        let mut p = packet();
        p.established.clear();
        for epic in ["011", "012", "013"] {
            let mut task = item(&format!("epic:{epic}/task:0"), &format!("Epic {epic} Task 0"), None);
            task.source_path = Some(format!("epics/{epic}.md"));
            p.established.push(task);
        }
        // A large Epic 011 tail used to starve all later Epic 012/013 items
        // because packet locator order was also prompt admission order.
        for criterion in 1..20 {
            let mut item = item(
                &format!("epic:011/task:0/criterion:{criterion}"),
                &format!("Epic 011 lower-ranked criterion {criterion} {}", "x".repeat(500)),
                None,
            );
            item.kind = NodeKind::AcceptanceCriterion;
            item.source_path = Some("epics/011.md".into());
            p.established.push(item);
        }
        let rendered = render_prompt(
            &request(MutationBoundary::ReadOnly),
            &p,
            PromptBudget { max_bytes: 16_000, bytes_per_token_estimate: 4 },
        ).unwrap();
        assert!(rendered.text.contains("Epic 011 Task 0"));
        assert!(rendered.text.contains("Epic 012 Task 0"));
        assert!(rendered.text.contains("Epic 013 Task 0"));
        assert!(!rendered.omitted_context.is_empty());
    }

    #[test]
    fn golden_request_classes() {
        #[derive(Deserialize)]
        struct Case {
            name: String,
            original: String,
            read_only: bool,
            constraints: Vec<String>,
            expected_fragments: Vec<String>,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!("../fixtures/prompts/cases.json")).unwrap();
        assert_eq!(cases.len(), 6);
        for case in cases {
            let r = PromptRequest {
                original: case.original.clone(),
                clarified_objective: None,
                negative_constraints: case.constraints,
                mutation_boundary: if case.read_only {
                    MutationBoundary::ReadOnly
                } else {
                    MutationBoundary::RepositoryChangesOnlyWhenExplicitlyRequested
                },
            };
            let got = render_prompt(&r, &packet(), PromptBudget::default()).unwrap().text;
            assert_eq!(got, render_prompt(&r, &packet(), PromptBudget::default()).unwrap().text);
            for fragment in case.expected_fragments {
                assert!(got.contains(&fragment), "golden {} missing {:?}", case.name, fragment);
            }
        }
    }
}
