//! Consolidation pass (Epic 013 Task 2): turn Task 1's raw proposed
//! vocabulary into a deduplicated seed vocabulary for the sequential
//! extraction phase.
//!
//! Approach: a second consolidation LLM call ("merge synonyms in this list,
//! keep distinct concepts separate") proposes merge groups; validating and
//! applying them is fully deterministic and testable offline. This was chosen
//! over a pure string/embedding-similarity merge because near-duplicates like
//! `uses`/`depends_on` are semantic synonyms that string similarity cannot
//! see, and embedding similarity would drag a model download into this
//! standalone crate. The model can only group existing entries — it cannot
//! rename, reword, invent, or drop them — and every applied or rejected
//! group is recorded in the consolidation log.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub const CONSOLIDATION_MODEL: &str = llm_sdk::models::xiaomi::MIMO_V2_5_PRO;

pub(crate) const CONSOLIDATION_MAX_COMPLETION_TOKENS: u32 = 4096;

/// Same generic framing as schema discovery: consolidation sees only the
/// proposed vocabulary, never project-specific context.
pub const CONSOLIDATION_SYSTEM_PROMPT: &str = r#"You are consolidating a proposed vocabulary for a knowledge graph that will power user-facing search over a collection of text documents.

You will receive a list of entity kinds and a list of predicates, each with a short description. Some entries may be near-duplicates: different names for the same concept, or predicates that describe the same relationship with the same subject and object kinds.

Group near-duplicate entity kinds and near-duplicate predicates so that each distinct concept and each distinct relationship shape is represented exactly once:

1. entity_kind_merges: groups of entity kinds that are synonyms for the same concept.
2. predicate_merges: groups of predicates that are synonyms for the same relationship shape (same meaning, same subject kind, same object kind).

Rules:
- Keep genuinely distinct concepts separate, even when their names look similar; a shared word alone does not make two entries synonyms.
- Do not invent, rename, reword, reorder, or drop any entry. You only group entries that already exist in the supplied vocabulary.
- Respond with exactly one JSON object of this shape:
{"entity_kind_merges":[{"canonical":string,"members":[string],"rationale":string}],"predicate_merges":[{"canonical":string,"members":[string],"rationale":string}]}
- "members" must list at least two names exactly as they appear in the supplied vocabulary, and "canonical" must be one of those member names, exactly.
- "rationale" must state in one line why these entries describe the same concept or relationship shape.
- Leave an array empty when nothing needs merging. When in doubt, do not merge.
- Return JSON only."#;

/// Raw proposed vocabulary exactly as Task 1 saved it: the schema-discovery
/// model's response, one JSON object with `entity_kinds`, `predicates`, and
/// `example_relations`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedVocabulary {
    #[serde(default)]
    pub entity_kinds: Vec<ProposedEntityKind>,
    #[serde(default)]
    pub predicates: Vec<ProposedPredicate>,
    #[serde(default)]
    pub example_relations: Vec<RelationExample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedEntityKind {
    pub kind: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedPredicate {
    pub predicate: String,
    pub description: String,
    pub subject_kind: String,
    pub object_kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationExample {
    pub subject: RelationEndpoint,
    pub predicate: String,
    pub object: RelationEndpoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationEndpoint {
    pub kind: String,
    pub name: String,
}

/// The consolidation model's response: merge groups only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsolidationProposal {
    #[serde(default)]
    pub entity_kind_merges: Vec<MergeGroup>,
    #[serde(default)]
    pub predicate_merges: Vec<MergeGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergeGroup {
    pub canonical: String,
    pub members: Vec<String>,
    pub rationale: String,
}

/// Consolidated seed vocabulary for Task 3. It keeps the proposed
/// vocabulary's base shape so one set of types loads both; entries that
/// absorbed a synonym additionally record the names folded into them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeedVocabulary {
    pub entity_kinds: Vec<SeedEntityKind>,
    pub predicates: Vec<SeedPredicate>,
    pub example_relations: Vec<RelationExample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeedEntityKind {
    pub kind: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merged_from: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeedPredicate {
    pub predicate: String,
    pub description: String,
    pub subject_kind: String,
    pub object_kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merged_from: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeTarget {
    EntityKind,
    Predicate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DecisionStatus {
    Applied,
    Rejected { reason: String },
}

/// One inspected merge decision: what the model proposed, whether it was
/// applied or rejected (and why), and what each merged-away entry carried so
/// no meaning is lost when the seed drops it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MergeDecisionRecord {
    pub target: MergeTarget,
    pub canonical: String,
    /// Member names exactly as the model proposed them.
    pub members: Vec<String>,
    pub rationale: String,
    #[serde(flatten)]
    pub status: DecisionStatus,
    /// Details of the members merged away (applied decisions only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub merged_members: Vec<MergedMemberDetail>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MergedMemberDetail {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_kind: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VocabularyCounts {
    pub entity_kinds: usize,
    pub predicates: usize,
    pub example_relations: usize,
}

/// Merge-decisions log written next to the seed vocabulary. It records the
/// method, the exact raw model response (before any validation), the input
/// and output counts, and every applied or rejected decision. When nothing
/// was merged, `no_merge_rationale` records that finding explicitly instead
/// of leaving the empty decision list to speak for itself.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConsolidationLog {
    pub method: String,
    pub model: String,
    pub raw_response: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub input_counts: VocabularyCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_counts: Option<VocabularyCounts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_merge_rationale: Option<String>,
    pub decisions: Vec<MergeDecisionRecord>,
}

/// Outcome of one consolidation run. `seed` is `None` only when the raw
/// response could not be parsed or an integrity guard tripped; the log is
/// always populated so the raw response stays inspectable.
pub struct ConsolidationReport {
    pub seed: Option<SeedVocabulary>,
    pub log: ConsolidationLog,
}

fn method_note() -> String {
    "Second LLM call proposes synonym merge groups (\"merge synonyms in this list, keep distinct \
     concepts separate\"); a deterministic validator then applies only safe groups (members exist \
     verbatim in the proposed vocabulary, each entry assigned at most once, predicates share one \
     subject/object shape after entity-kind canonicalization) and records every applied and \
     rejected decision with the model's rationale. String/embedding similarity was rejected as the \
     primary mechanism: near-duplicates like uses/depends_on are semantic synonyms invisible to \
     string similarity, and embedding models would add a model download to this standalone crate. \
     The consolidation model can only group existing entries, never rename, reword, invent, or \
     drop them."
        .to_owned()
}

/// Validate a proposed vocabulary before consolidation. Consolidation assumes
/// well-formed shape info: unique names, at least one entity kind, and every
/// predicate shape referencing a declared entity kind.
pub fn validate_vocabulary(vocabulary: &ProposedVocabulary) -> anyhow::Result<()> {
    anyhow::ensure!(
        !vocabulary.entity_kinds.is_empty(),
        "proposed vocabulary contains no entity kinds"
    );
    let mut seen_kinds = BTreeSet::new();
    for entry in &vocabulary.entity_kinds {
        anyhow::ensure!(
            seen_kinds.insert(entry.kind.as_str()),
            "proposed vocabulary declares entity kind {:?} more than once",
            entry.kind
        );
    }
    let mut seen_predicates = BTreeSet::new();
    for entry in &vocabulary.predicates {
        anyhow::ensure!(
            seen_predicates.insert(entry.predicate.as_str()),
            "proposed vocabulary declares predicate {:?} more than once",
            entry.predicate
        );
        for shape in [&entry.subject_kind, &entry.object_kind] {
            anyhow::ensure!(
                seen_kinds.contains(shape.as_str()),
                "predicate {:?} references undeclared entity kind {shape:?}",
                entry.predicate
            );
        }
    }
    Ok(())
}

/// Build the consolidation user payload: the proposed entity kinds and
/// predicates with their descriptions and shapes. Example relations are not
/// needed to judge synonymy and stay out of the request.
pub fn build_consolidation_payload(vocabulary: &ProposedVocabulary) -> serde_json::Value {
    serde_json::json!({
        "entity_kinds": vocabulary
            .entity_kinds
            .iter()
            .map(|entry| serde_json::json!({
                "kind": entry.kind,
                "description": entry.description,
            }))
            .collect::<Vec<_>>(),
        "predicates": vocabulary
            .predicates
            .iter()
            .map(|entry| serde_json::json!({
                "predicate": entry.predicate,
                "description": entry.description,
                "subject_kind": entry.subject_kind,
                "object_kind": entry.object_kind,
            }))
            .collect::<Vec<_>>(),
    })
}

/// Run one consolidation pass over a validated proposed vocabulary using the
/// consolidation model's raw response. Never fails: parse failures and
/// integrity-guard trips are recorded in the log (and the command exits
/// non-zero) so the raw response stays inspectable.
pub fn consolidate_vocabulary(
    input: &ProposedVocabulary,
    raw_response: &str,
) -> ConsolidationReport {
    if let Err(error) = validate_vocabulary(input) {
        return failure_report(input, raw_response, &error.to_string());
    }
    let proposal = match serde_json::from_str::<ConsolidationProposal>(raw_response.trim()) {
        Ok(proposal) => proposal,
        Err(error) => {
            return failure_report(
                input,
                raw_response,
                &format!(
                    "consolidation response is not a valid merge-proposal JSON object: {error}"
                ),
            )
        }
    };
    let (decisions, seed) = apply_proposal(input, &proposal);
    if let Err(error) = check_seed_integrity(input, &seed) {
        return failure_report(input, raw_response, &error.to_string());
    }
    let no_merge_rationale = if decisions
        .iter()
        .any(|decision| decision.status == DecisionStatus::Applied)
    {
        None
    } else {
        Some(no_merge_finding(input))
    };
    ConsolidationReport {
        seed: Some(seed.clone()),
        log: build_log(input, Some(&seed), decisions, raw_response, None).with_no_merge(no_merge_rationale),
    }
}

/// Explicit rationale for a run in which nothing was merged: either the
/// consolidation model proposed no groups, or every group it proposed was
/// rejected by the validator.
fn no_merge_finding(input: &ProposedVocabulary) -> String {
    let shapes: Vec<String> = input
        .predicates
        .iter()
        .map(|predicate| {
            format!(
                "{} ({} -> {})",
                predicate.predicate, predicate.subject_kind, predicate.object_kind
            )
        })
        .collect();
    format!(
        "No merges required: no merge group survived validation, so every entry is distinct. \
         Entity kinds: [{}]. Predicate relationship shapes: [{}]. \
         Keeping these separate is a valid outcome; the sequential phase may still mint new entries.",
        input
            .entity_kinds
            .iter()
            .map(|kind| kind.kind.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        shapes.join(", "),
    )
}

fn failure_report(
    input: &ProposedVocabulary,
    raw_response: &str,
    error: &str,
) -> ConsolidationReport {
    ConsolidationReport {
        seed: None,
        log: build_log(
            input,
            None,
            Vec::new(),
            raw_response,
            Some(error.to_owned()),
        ),
    }
}

fn build_log(
    input: &ProposedVocabulary,
    output: Option<&SeedVocabulary>,
    decisions: Vec<MergeDecisionRecord>,
    raw_response: &str,
    error: Option<String>,
) -> ConsolidationLog {
    ConsolidationLog {
        method: method_note(),
        model: CONSOLIDATION_MODEL.to_owned(),
        raw_response: raw_response.to_owned(),
        error,
        input_counts: counts_of(
            input.entity_kinds.len(),
            input.predicates.len(),
            input.example_relations.len(),
        ),
        output_counts: output.map(|seed| {
            counts_of(
                seed.entity_kinds.len(),
                seed.predicates.len(),
                seed.example_relations.len(),
            )
        }),
        no_merge_rationale: None,
        decisions,
    }
}

impl ConsolidationLog {
    fn with_no_merge(mut self, rationale: Option<String>) -> Self {
        self.no_merge_rationale = rationale;
        self
    }
}

fn counts_of(
    entity_kinds: usize,
    predicates: usize,
    example_relations: usize,
) -> VocabularyCounts {
    VocabularyCounts {
        entity_kinds,
        predicates,
        example_relations,
    }
}

/// Defensive post-apply guards: consolidation must never grow the vocabulary
/// or leave a seed predicate pointing at a kind that no longer exists.
fn check_seed_integrity(input: &ProposedVocabulary, seed: &SeedVocabulary) -> anyhow::Result<()> {
    anyhow::ensure!(
        seed.entity_kinds.len() <= input.entity_kinds.len()
            && seed.predicates.len() <= input.predicates.len(),
        "consolidation grew the vocabulary (input {} kinds / {} predicates, output {} / {})",
        input.entity_kinds.len(),
        input.predicates.len(),
        seed.entity_kinds.len(),
        seed.predicates.len(),
    );
    for predicate in &seed.predicates {
        for shape in [&predicate.subject_kind, &predicate.object_kind] {
            anyhow::ensure!(
                seed.entity_kinds.iter().any(|kind| &kind.kind == shape),
                "seed predicate {:?} references entity kind {shape:?} missing from the seed",
                predicate.predicate
            );
        }
    }
    Ok(())
}

/// Deterministically validate and apply the model's merge groups. Invalid
/// groups are recorded as rejected decisions and leave the vocabulary
/// untouched; applied groups fold non-canonical members into the canonical
/// one, which keeps its own name and description.
fn apply_proposal(
    input: &ProposedVocabulary,
    proposal: &ConsolidationProposal,
) -> (Vec<MergeDecisionRecord>, SeedVocabulary) {
    let kind_entries: BTreeMap<&str, &ProposedEntityKind> = input
        .entity_kinds
        .iter()
        .map(|entry| (entry.kind.as_str(), entry))
        .collect();
    let kind_names: BTreeSet<String> = input
        .entity_kinds
        .iter()
        .map(|entry| entry.kind.clone())
        .collect();
    let predicate_entries: BTreeMap<&str, &ProposedPredicate> = input
        .predicates
        .iter()
        .map(|entry| (entry.predicate.as_str(), entry))
        .collect();
    let predicate_names: BTreeSet<String> = input
        .predicates
        .iter()
        .map(|entry| entry.predicate.clone())
        .collect();

    let mut decisions = Vec::new();
    let mut assigned_kinds: BTreeSet<String> = BTreeSet::new();
    // merged-away kind name -> canonical kind name
    let mut merged_kinds: BTreeMap<String, String> = BTreeMap::new();

    for group in &proposal.entity_kind_merges {
        let mut record = base_record(MergeTarget::EntityKind, group);
        match check_group(group, &kind_names, &assigned_kinds) {
            Ok(merged) => {
                for member in &merged {
                    assigned_kinds.insert(member.clone());
                    merged_kinds.insert(member.clone(), group.canonical.clone());
                }
                assigned_kinds.insert(group.canonical.clone());
                record.status = DecisionStatus::Applied;
                record.merged_members = merged
                    .iter()
                    .map(|name| entity_member_detail(kind_entries[name.as_str()]))
                    .collect();
            }
            Err(reason) => record.status = DecisionStatus::Rejected { reason },
        }
        decisions.push(record);
    }

    let mut assigned_predicates: BTreeSet<String> = BTreeSet::new();
    // merged-away predicate name -> canonical predicate name
    let mut merged_predicates: BTreeMap<String, String> = BTreeMap::new();

    for group in &proposal.predicate_merges {
        let mut record = base_record(MergeTarget::Predicate, group);
        let outcome = check_group(group, &predicate_names, &assigned_predicates).and_then(|merged| {
            // All members must describe one relationship shape, compared
            // after entity-kind canonicalization so a kind merge cannot make
            // equal shapes look different.
            let shapes: BTreeMap<&str, (String, String)> = group
                .members
                .iter()
                .map(|name| {
                    let predicate = predicate_entries[name.as_str()];
                    (
                        name.as_str(),
                        (
                            canonical_kind(&merged_kinds, &predicate.subject_kind),
                            canonical_kind(&merged_kinds, &predicate.object_kind),
                        ),
                    )
                })
                .collect();
            let reference = shapes[group.canonical.as_str()].clone();
            let divergent: Vec<String> = shapes
                .into_iter()
                .filter(|(_, shape)| *shape != reference)
                .map(|(name, (subject, object))| format!("{name} ({subject} -> {object})"))
                .collect();
            if divergent.is_empty() {
                Ok(merged)
            } else {
                Err(format!(
                    "merge group mixes relationship shapes; canonical {} is ({} -> {}), but {} describe a different shape",
                    group.canonical,
                    reference.0,
                    reference.1,
                    divergent.join(", ")
                ))
            }
        });
        match outcome {
            Ok(merged) => {
                for member in &merged {
                    assigned_predicates.insert(member.clone());
                    merged_predicates.insert(member.clone(), group.canonical.clone());
                }
                assigned_predicates.insert(group.canonical.clone());
                record.status = DecisionStatus::Applied;
                record.merged_members = merged
                    .iter()
                    .map(|name| predicate_member_detail(predicate_entries[name.as_str()]))
                    .collect();
            }
            Err(reason) => record.status = DecisionStatus::Rejected { reason },
        }
        decisions.push(record);
    }

    let seed = build_seed(input, &merged_kinds, &merged_predicates);
    (decisions, seed)
}

/// The surviving name for `kind`: itself, unless a merge folded it into a
/// canonical kind.
fn canonical_kind(merged_kinds: &BTreeMap<String, String>, kind: &str) -> String {
    merged_kinds
        .get(kind)
        .map(String::as_str)
        .unwrap_or(kind)
        .to_owned()
}

fn base_record(target: MergeTarget, group: &MergeGroup) -> MergeDecisionRecord {
    MergeDecisionRecord {
        target,
        canonical: group.canonical.clone(),
        members: group.members.clone(),
        rationale: group.rationale.clone(),
        status: DecisionStatus::Rejected {
            reason: "validation did not run".to_owned(),
        },
        merged_members: Vec::new(),
    }
}

/// Shared group checks: at least two distinct verbatim members that exist in
/// the vocabulary, canonical among them, and no member already consumed by an
/// earlier applied group. Returns the members to merge away.
fn check_group(
    group: &MergeGroup,
    valid_names: &BTreeSet<String>,
    assigned: &BTreeSet<String>,
) -> Result<Vec<String>, String> {
    if group.members.len() < 2 {
        return Err(format!(
            "merge group must list at least two members, got {:?}",
            group.members
        ));
    }
    let mut seen = BTreeSet::new();
    for member in &group.members {
        if !seen.insert(member.as_str()) {
            return Err(format!("merge group lists {member:?} more than once"));
        }
    }
    if !group.members.iter().any(|member| member == &group.canonical) {
        return Err(format!(
            "canonical {:?} is not one of the group members {group:?}",
            group.canonical
        ));
    }
    let unknown: Vec<String> = group
        .members
        .iter()
        .filter(|member| !valid_names.contains(member.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "members not present in the proposed vocabulary: {unknown:?}"
        ));
    }
    let already_assigned: Vec<String> = group
        .members
        .iter()
        .filter(|member| assigned.contains(member.as_str()))
        .cloned()
        .collect();
    if !already_assigned.is_empty() {
        return Err(format!(
            "members already assigned to another merge group: {already_assigned:?}"
        ));
    }
    Ok(group
        .members
        .iter()
        .filter(|member| *member != &group.canonical)
        .cloned()
        .collect())
}

fn entity_member_detail(entry: &ProposedEntityKind) -> MergedMemberDetail {
    MergedMemberDetail {
        name: entry.kind.clone(),
        description: entry.description.clone(),
        subject_kind: None,
        object_kind: None,
    }
}

fn predicate_member_detail(entry: &ProposedPredicate) -> MergedMemberDetail {
    MergedMemberDetail {
        name: entry.predicate.clone(),
        description: entry.description.clone(),
        subject_kind: Some(entry.subject_kind.clone()),
        object_kind: Some(entry.object_kind.clone()),
    }
}

fn build_seed(
    input: &ProposedVocabulary,
    merged_kinds: &BTreeMap<String, String>,
    merged_predicates: &BTreeMap<String, String>,
) -> SeedVocabulary {
    let canonical = |kind: &str| canonical_kind(merged_kinds, kind);
    let entity_kinds = input        .entity_kinds
        .iter()
        .filter(|entry| !merged_kinds.contains_key(entry.kind.as_str()))
        .map(|entry| SeedEntityKind {
            kind: entry.kind.clone(),
            description: entry.description.clone(),
            merged_from: merged_kinds
                .iter()
                .filter(|(_, canonical)| canonical.as_str() == entry.kind.as_str())
                .map(|(merged, _)| merged.clone())
                .collect(),
        })
        .collect();
    let predicates = input
        .predicates
        .iter()
        .filter(|entry| !merged_predicates.contains_key(entry.predicate.as_str()))
        .map(|entry| SeedPredicate {
            predicate: entry.predicate.clone(),
            description: entry.description.clone(),
            subject_kind: canonical(&entry.subject_kind),
            object_kind: canonical(&entry.object_kind),
            merged_from: merged_predicates
                .iter()
                .filter(|(_, canonical)| canonical.as_str() == entry.predicate.as_str())
                .map(|(merged, _)| merged.clone())
                .collect(),
        })
        .collect();
    let example_relations = input
        .example_relations
        .iter()
        .map(|relation| RelationExample {
            subject: rewrite_endpoint(&relation.subject, &canonical),
            predicate: merged_predicates
                .get(relation.predicate.as_str())
                .cloned()
                .unwrap_or_else(|| relation.predicate.clone()),
            object: rewrite_endpoint(&relation.object, &canonical),
        })
        .collect();
    SeedVocabulary {
        entity_kinds,
        predicates,
        example_relations,
    }
}

fn rewrite_endpoint(
    endpoint: &RelationEndpoint,
    canonical_kind: &impl Fn(&str) -> String,
) -> RelationEndpoint {
    RelationEndpoint {
        kind: canonical_kind(&endpoint.kind),
        name: endpoint.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic vocabulary with genuine near-duplicates (`uses` /
    /// `depends_on` describe the same CLI -> Database relation shape) plus
    /// concepts that must stay separate.
    fn synthetic_vocabulary() -> ProposedVocabulary {
        serde_json::from_str(
            r#"{
            "entity_kinds": [
                {"kind": "CLI", "description": "A command-line tool or interface."},
                {"kind": "Database", "description": "A storage system for indexed data."},
                {"kind": "Model", "description": "A machine learning model."},
                {"kind": "SearchIndex", "description": "A specific indexing technology."},
                {"kind": "DataSource", "description": "A source of information that can be searched."}
            ],
            "predicates": [
                {"predicate": "uses", "description": "A CLI tool or system utilizes a database.", "subject_kind": "CLI", "object_kind": "Database"},
                {"predicate": "depends_on", "description": "A CLI tool relies on a database to function.", "subject_kind": "CLI", "object_kind": "Database"},
                {"predicate": "requires", "description": "A CLI tool needs a particular model.", "subject_kind": "CLI", "object_kind": "Model"},
                {"predicate": "searches", "description": "A search interface queries data sources.", "subject_kind": "CLI", "object_kind": "DataSource"},
                {"predicate": "degrades_to", "description": "A fallback when a primary index fails.", "subject_kind": "SearchIndex", "object_kind": "SearchIndex"}
            ],
            "example_relations": [
                {"subject": {"kind": "CLI", "name": "graph CLI"}, "predicate": "uses", "object": {"kind": "Database", "name": "per-repo SQLite database"}},
                {"subject": {"kind": "CLI", "name": "graph CLI"}, "predicate": "depends_on", "object": {"kind": "Database", "name": "SQLite"}},
                {"subject": {"kind": "SearchIndex", "name": "FTS5"}, "predicate": "degrades_to", "object": {"kind": "SearchIndex", "name": "substring search"}}
            ]
        }"#,
        )
        .unwrap()
    }

    fn applied_groups(report: &ConsolidationReport) -> usize {
        report
            .log
            .decisions
            .iter()
            .filter(|decision| decision.status == DecisionStatus::Applied)
            .count()
    }

    #[test]
    fn near_duplicate_predicates_are_merged_into_one() {
        let input = synthetic_vocabulary();
        let raw = r#"{"entity_kind_merges":[],"predicate_merges":[{"canonical":"uses","members":["uses","depends_on"],"rationale":"Both state that a CLI relies on a storage component with the same shape."}]}"#;
        let report = consolidate_vocabulary(&input, raw);
        let seed = report.seed.expect("valid proposal must produce a seed");
        assert!(report.log.no_merge_rationale.is_none());

        assert_eq!(seed.predicates.len(), 4);
        let uses = seed
            .predicates
            .iter()
            .find(|predicate| predicate.predicate == "uses")
            .unwrap();
        assert_eq!(uses.merged_from, vec!["depends_on"]);
        assert_eq!(uses.subject_kind, "CLI");
        assert_eq!(uses.object_kind, "Database");
        assert_eq!(uses.description, input.predicates[0].description);
        assert!(seed
            .predicates
            .iter()
            .all(|predicate| predicate.predicate != "depends_on"));

        // Example relations mentioning the merged-away name are rewritten.
        assert!(seed
            .example_relations
            .iter()
            .all(|relation| relation.predicate != "depends_on"));
        assert_eq!(seed.example_relations.len(), input.example_relations.len());

        // The log records what was merged into what, why, and what the
        // merged-away entry carried.
        let decision = report
            .log
            .decisions
            .iter()
            .find(|decision| decision.target == MergeTarget::Predicate)
            .unwrap();
        assert_eq!(decision.status, DecisionStatus::Applied);
        assert_eq!(decision.canonical, "uses");
        assert_eq!(decision.members, vec!["uses", "depends_on"]);
        assert_eq!(decision.merged_members.len(), 1);
        assert_eq!(decision.merged_members[0].name, "depends_on");
        assert_eq!(
            decision.merged_members[0].subject_kind.as_deref(),
            Some("CLI")
        );
        assert_eq!(
            decision.merged_members[0].object_kind.as_deref(),
            Some("Database")
        );
        assert!(decision.rationale.contains("storage component"));
    }

    #[test]
    fn distinct_concepts_are_kept_separate() {
        let input = synthetic_vocabulary();
        let raw = r#"{"entity_kind_merges":[],"predicate_merges":[]}"#;
        let report = consolidate_vocabulary(&input, raw);
        let seed = report.seed.expect("empty proposal must produce a seed");

        assert!(report.log.decisions.is_empty());
        // A zero-merge outcome is recorded explicitly, with the vocabulary's
        // actual shapes as the rationale, not left implicit.
        let rationale = report.log.no_merge_rationale.expect("zero-merge run records a rationale");
        assert!(rationale.contains("No merges required"), "{rationale}");
        assert!(rationale.contains("uses (CLI -> Database)"), "{rationale}");
        assert_eq!(seed.entity_kinds.len(), input.entity_kinds.len());
        assert_eq!(seed.predicates.len(), input.predicates.len());
        for (seeded, original) in seed.predicates.iter().zip(&input.predicates) {
            assert_eq!(seeded.predicate, original.predicate);
            assert_eq!(seeded.description, original.description);
            assert_eq!(seeded.subject_kind, original.subject_kind);
            assert_eq!(seeded.object_kind, original.object_kind);
            assert!(seeded.merged_from.is_empty());
        }
        for (seeded, original) in seed.entity_kinds.iter().zip(&input.entity_kinds) {
            assert_eq!(seeded.kind, original.kind);
            assert_eq!(seeded.description, original.description);
            assert!(seeded.merged_from.is_empty());
        }
        assert_eq!(seed.example_relations, input.example_relations);
    }

    #[test]
    fn consolidated_counts_are_never_larger_than_input() {
        let input = synthetic_vocabulary();

        // No merges: equal counts.
        let report = consolidate_vocabulary(&input, r#"{"entity_kind_merges":[],"predicate_merges":[]}"#);
        let seed = report.seed.unwrap();
        assert_eq!(seed.entity_kinds.len(), input.entity_kinds.len());
        assert_eq!(seed.predicates.len(), input.predicates.len());

        // One predicate merge: strictly smaller.
        let report = consolidate_vocabulary(
            &input,
            r#"{"entity_kind_merges":[],"predicate_merges":[{"canonical":"uses","members":["uses","depends_on"],"rationale":"synonyms"}]}"#,
        );
        let seed = report.seed.unwrap();
        assert_eq!(seed.predicates.len(), input.predicates.len() - 1);

        // Merges in both lists: strictly smaller in both.
        let report = consolidate_vocabulary(
            &input,
            r#"{"entity_kind_merges":[{"canonical":"Database","members":["Database","SearchIndex"],"rationale":"both storage"}],"predicate_merges":[{"canonical":"uses","members":["uses","depends_on"],"rationale":"synonyms"}]}"#,
        );
        let seed = report.seed.as_ref().unwrap();
        assert_eq!(seed.entity_kinds.len(), input.entity_kinds.len() - 1);
        assert_eq!(seed.predicates.len(), input.predicates.len() - 1);
        assert_eq!(applied_groups(&report), 2);
    }

    #[test]
    fn unsafe_merge_proposals_are_rejected_and_recorded() {
        let input = synthetic_vocabulary();

        // Shape mismatch: uses (CLI -> Database) vs requires (CLI -> Model).
        let report = consolidate_vocabulary(
            &input,
            r#"{"entity_kind_merges":[],"predicate_merges":[{"canonical":"uses","members":["uses","requires"],"rationale":"both connect a CLI to something"}]}"#,
        );
        assert!(report.seed.unwrap().predicates.len() == input.predicates.len());
        let decision = &report.log.decisions[0];
        let reason = match &decision.status {
            DecisionStatus::Rejected { reason } => reason.clone(),
            DecisionStatus::Applied => panic!("shape mismatch must be rejected"),
        };
        assert!(reason.contains("mixes relationship shapes"), "{reason}");
        assert!(reason.contains("requires (CLI -> Model)"), "{reason}");

        // Unknown member name.
        let report = consolidate_vocabulary(
            &input,
            r#"{"entity_kind_merges":[],"predicate_merges":[{"canonical":"uses","members":["uses","relies_on"],"rationale":"synonyms"}]}"#,
        );
        assert!(matches!(
            report.log.decisions[0].status,
            DecisionStatus::Rejected { .. }
        ));
        assert_eq!(report.seed.unwrap().predicates.len(), input.predicates.len());

        // Single-member group.
        let report = consolidate_vocabulary(
            &input,
            r#"{"entity_kind_merges":[],"predicate_merges":[{"canonical":"uses","members":["uses"],"rationale":"nothing to merge"}]}"#,
        );
        assert!(matches!(
            report.log.decisions[0].status,
            DecisionStatus::Rejected { .. }
        ));

        // Canonical not among the members.
        let report = consolidate_vocabulary(
            &input,
            r#"{"entity_kind_merges":[],"predicate_merges":[{"canonical":"requires","members":["uses","depends_on"],"rationale":"synonyms"}]}"#,
        );
        assert!(matches!(
            report.log.decisions[0].status,
            DecisionStatus::Rejected { .. }
        ));

        // Double assignment: the second overlapping group is rejected, the
        // first still applies.
        let report = consolidate_vocabulary(
            &input,
            r#"{"entity_kind_merges":[],"predicate_merges":[
                {"canonical":"uses","members":["uses","depends_on"],"rationale":"synonyms"},
                {"canonical":"depends_on","members":["depends_on","uses"],"rationale":"synonyms again"}
            ]}"#,
        );
        assert_eq!(applied_groups(&report), 1);
        let second = &report.log.decisions[1];
        let reason = match &second.status {
            DecisionStatus::Rejected { reason } => reason.clone(),
            DecisionStatus::Applied => panic!("overlapping group must be rejected"),
        };
        assert!(reason.contains("already assigned"), "{reason}");
        let seed = report.seed.unwrap();
        assert_eq!(seed.predicates.len(), input.predicates.len() - 1);
        assert!(seed
            .predicates
            .iter()
            .all(|predicate| predicate.predicate != "depends_on"));
    }

    #[test]
    fn merged_entity_kinds_rewrite_predicate_shapes_and_examples() {
        let input = synthetic_vocabulary();
        let raw = r#"{"entity_kind_merges":[{"canonical":"Database","members":["Database","SearchIndex"],"rationale":"Both are storage layers for indexed data."}],"predicate_merges":[]}"#;
        let report = consolidate_vocabulary(&input, raw);
        let seed = report.seed.expect("valid proposal must produce a seed");

        assert_eq!(seed.entity_kinds.len(), input.entity_kinds.len() - 1);
        let database = seed
            .entity_kinds
            .iter()
            .find(|kind| kind.kind == "Database")
            .unwrap();
        assert_eq!(database.merged_from, vec!["SearchIndex"]);
        assert!(seed.entity_kinds.iter().all(|kind| kind.kind != "SearchIndex"));

        // degrades_to was SearchIndex -> SearchIndex; its shape is rewritten
        // to the surviving kind.
        let degrades_to = seed
            .predicates
            .iter()
            .find(|predicate| predicate.predicate == "degrades_to")
            .unwrap();
        assert_eq!(degrades_to.subject_kind, "Database");
        assert_eq!(degrades_to.object_kind, "Database");

        // Example relations referencing the merged-away kind are rewritten.
        let degraded_example = seed
            .example_relations
            .iter()
            .find(|relation| relation.predicate == "degrades_to")
            .unwrap();
        assert_eq!(degraded_example.subject.kind, "Database");
        assert_eq!(degraded_example.object.kind, "Database");

        // The log carries the merged-away kind's description.
        let decision = &report.log.decisions[0];
        assert_eq!(decision.target, MergeTarget::EntityKind);
        assert_eq!(decision.merged_members[0].name, "SearchIndex");
        assert_eq!(
            decision.merged_members[0].description,
            input.entity_kinds[3].description
        );
    }

    #[test]
    fn unparseable_consolidation_response_is_recorded_not_applied() {
        let raw = "not json at all";
        let report = consolidate_vocabulary(&synthetic_vocabulary(), raw);
        assert!(report.seed.is_none());
        assert!(report.log.decisions.is_empty());
        assert!(report
            .log
            .error
            .as_deref()
            .unwrap()
            .contains("not a valid merge-proposal JSON object"));
        assert_eq!(report.log.raw_response, raw);
        assert!(report.log.output_counts.is_none());
    }

    #[test]
    fn malformed_input_vocabulary_is_rejected_before_consolidation() {
        let duplicate_kinds: ProposedVocabulary = serde_json::from_str(
            r#"{
            "entity_kinds": [
                {"kind": "CLI", "description": "A command-line tool."},
                {"kind": "CLI", "description": "A command-line tool again."}
            ],
            "predicates": [],
            "example_relations": []
        }"#,
        )
        .unwrap();
        assert!(validate_vocabulary(&duplicate_kinds)
            .unwrap_err()
            .to_string()
            .contains("more than once"));

        let dangling_kind: ProposedVocabulary = serde_json::from_str(
            r#"{
            "entity_kinds": [{"kind": "CLI", "description": "A command-line tool."}],
            "predicates": [{"predicate": "uses", "description": "utilizes", "subject_kind": "CLI", "object_kind": "Database"}],
            "example_relations": []
        }"#,
        )
        .unwrap();
        assert!(validate_vocabulary(&dangling_kind)
            .unwrap_err()
            .to_string()
            .contains("undeclared entity kind"));

        let mut empty = synthetic_vocabulary();
        empty.entity_kinds.clear();
        assert!(validate_vocabulary(&empty).is_err());

        assert!(validate_vocabulary(&synthetic_vocabulary()).is_ok());

        // The full run also refuses malformed input, keeping the raw response
        // out of the picture entirely.
        let report = consolidate_vocabulary(&dangling_kind, r#"{"entity_kind_merges":[],"predicate_merges":[]}"#);
        assert!(report.seed.is_none());
        assert!(report.log.error.is_some());
    }

    #[test]
    fn seed_json_keeps_the_proposed_shape_and_omits_empty_provenance() {
        let input = synthetic_vocabulary();
        let report = consolidate_vocabulary(
            &input,
            r#"{"entity_kind_merges":[],"predicate_merges":[{"canonical":"uses","members":["uses","depends_on"],"rationale":"synonyms"}]}"#,
        );
        let seed = report.seed.unwrap();
        let json = serde_json::to_value(&seed).unwrap();
        // Same base keys as the proposed vocabulary, so Task 3 can load both
        // with one set of types.
        for key in ["entity_kinds", "predicates", "example_relations"] {
            assert!(json.get(key).and_then(|value| value.as_array()).is_some());
        }
        // merged_from appears only on merged entries.
        assert!(json["predicates"][0].get("merged_from").is_some());
        assert!(json["predicates"][1].get("merged_from").is_none());
        assert!(json["entity_kinds"][0].get("merged_from").is_none());
        // And the seed file round-trips through the seed types.
        let parsed: SeedVocabulary = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, seed);
    }

    #[test]
    fn consolidation_payload_carries_descriptions_and_shapes_only() {
        let payload = build_consolidation_payload(&synthetic_vocabulary());
        let serialized = payload.to_string();
        assert!(serialized.contains("\"subject_kind\""));
        assert!(serialized.contains("\"description\""));
        assert!(!serialized.contains("example_relations"));
        assert!(!serialized.contains("per-repo SQLite database"));
    }

    #[test]
    fn consolidation_prompt_has_only_generic_graph_framing() {
        let prompt = CONSOLIDATION_SYSTEM_PROMPT.to_ascii_lowercase();
        for token in [
            "daftprompt",
            "git",
            "commit",
            "epic",
            "index",
            "akar",
            "wgpu",
            "taffy",
            "glyphon",
            "sqlite",
            "embedding",
            "tree-sitter",
            "rust",
            "cargo",
            "xiaomi",
            "mimo",
            "repo",
            "code",
            "card",
            "canvas",
        ] {
            assert!(!prompt.contains(token), "prompt must not contain {token:?}");
        }
        for token in [
            "graph",
            "search",
            "entity_kind_merges",
            "predicate_merges",
            "canonical",
            "rationale",
        ] {
            assert!(prompt.contains(token), "prompt must mention {token:?}");
        }
    }
}
