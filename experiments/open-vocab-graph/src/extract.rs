//! Sequential carryover extraction loop (Epic 013 Task 3): split a source
//! into word-budget blocks, walk them in order one LLM call at a time, and
//! carry a running summary plus a growable open vocabulary from call to call.
//!
//! Evidence validation reuses Epic 011/012's approach, reimplemented here to
//! keep the crate independent of `daftprompt-indexer`: a citation is valid
//! only when its text equals the real snapshot lines exactly after
//! whitespace normalization. There is deliberately no prefix-forgiveness or
//! best-effort re-anchoring: a failed citation invalidates its claim, and a
//! failed claim is recorded, never silently dropped. Running context is not
//! evidence — every claim must cite lines inside the current block, and a
//! valid citation is not judged for whether it actually supports the claim
//! (that is Task 4's separate support review).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::consolidate::{
    ProposedEntityKind, ProposedPredicate, SeedEntityKind, SeedPredicate, SeedVocabulary,
};

pub const EXTRACTION_MODEL: &str = llm_sdk::models::xiaomi::MIMO_V2_5_PRO;

pub(crate) const EXTRACTION_MAX_COMPLETION_TOKENS: u32 = 4096;

/// Open-vocabulary extraction prompt. Unlike the fixed-vocabulary graph CLI
/// contract, no predicate or entity-kind list is hard-coded here: the
/// vocabulary travels in the user payload and may grow via mints. The
/// evidence rules mirror Epic 011/012's contract (complete verbatim lines,
/// copy the "N: " number, current block only).
pub const EXTRACTION_SYSTEM_PROMPT: &str = r#"You are extracting a knowledge graph from a document, one block at a time, to power user-facing search.

Each call supplies:
- "block": the current text block. Every physical line is prefixed with its absolute one-based line number and ": ".
- "running_summary": a short summary of what earlier blocks contained. Use it to keep names and terminology consistent, but treat it as context only, never as evidence.
- "vocabulary": the accumulated graph vocabulary (entity kinds and predicates, each with a description and, for predicates, the expected subject and object entity kinds).

From the CURRENT BLOCK ONLY, extract relations:
- A relation is {"subject":{"kind":string,"name":string},"predicate":string,"object":{"kind":string,"name":string},"evidence":[...]}.
- Prefer reusing a vocabulary entity kind or predicate when one fits the text.
- When nothing in the vocabulary fits a relation the block clearly supports, mint a new entity kind or predicate and record it in "new_entity_kinds" / "new_predicates". A new predicate must state a one-line description plus the entity kinds expected for its subject and object; a new entity kind must state a one-line description. Mint only what the block actually needs.
- Do not mint a near-synonym of an existing entry; reuse the existing name instead.

Evidence rules:
- Every relation needs at least one evidence item: {"start_line":integer,"end_line":integer,"text":string} citing complete physical lines of the current block.
- Copy the leading line number from the "N: " prefix into start_line/end_line; never count lines yourself.
- Evidence text must NOT include the "N: " prefix: copy only the original line content after it, keeping its exact wording, whitespace, and punctuation.
- Evidence text must be the COMPLETE verbatim source line(s) for start_line through end_line: copy the first line, the last line, and every line between as complete physical lines. Do not paraphrase, shorten, normalize, or truncate.
- Cite only lines that appear in the current block. If the current block's own lines do not support a relation, omit that relation, even if the running summary mentions it.

Respond with exactly one JSON object of this shape:
{"relations":[{"subject":{"kind":string,"name":string},"predicate":string,"object":{"kind":string,"name":string},"evidence":[{"start_line":integer,"end_line":integer,"text":string}]}],"new_entity_kinds":[{"kind":string,"description":string}],"new_predicates":[{"predicate":string,"description":string,"subject_kind":string,"object_kind":string}],"running_summary":string}
- Use empty arrays when there is nothing to report.
- "running_summary" is a short updated summary of the document so far, including this block, carried into the next call. Keep entity names and predicate names consistent with earlier calls.
- Return JSON only."#;

/// One complete-line block. Paragraph units (runs of consecutive non-blank
/// physical lines) are packed greedily under a word budget, so a block
/// boundary always falls between paragraphs and a physical line is never
/// split; a single paragraph longer than the budget becomes its own
/// oversized block rather than being broken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractionBlock {
    pub id: String,
    pub source_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    pub word_count: usize,
}

/// Split content into blocks of at most `word_budget` words, preferring
/// paragraph boundaries and never breaking a physical line. Block ids are
/// sequential per call (`b1`, `b2`, ...); callers renumber across sources.
pub fn split_into_budget_blocks(
    source_path: &str,
    content: &str,
    word_budget: u32,
) -> anyhow::Result<Vec<ExtractionBlock>> {
    anyhow::ensure!(word_budget >= 1, "word budget must be at least 1");
    let budget = word_budget as usize;
    let lines: Vec<&str> = content.split_inclusive('\n').collect();

    let mut units: Vec<(usize, usize)> = Vec::new();
    let mut unit_start: Option<usize> = None;
    for (index, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            if let Some(start) = unit_start.take() {
                units.push((start, index));
            }
        } else if unit_start.is_none() {
            unit_start = Some(index);
        }
    }
    if let Some(start) = unit_start {
        units.push((start, lines.len()));
    }
    let unit_words: Vec<usize> = units
        .iter()
        .map(|(start, end)| {
            lines[*start..*end]
                .iter()
                .map(|line| line.split_whitespace().count())
                .sum()
        })
        .collect();

    let mut blocks = Vec::new();
    let mut current: Option<(usize, usize, usize)> = None;
    for (index, &(_, unit_end)) in units.iter().enumerate() {
        match current {
            Some((first, _, words)) if words + unit_words[index] <= budget => {
                current = Some((first, unit_end, words + unit_words[index]));
            }
            Some((first, end, _)) => {
                blocks.push(build_block(source_path, lines.as_slice(), units[first].0, end));
                current = Some((index, unit_end, unit_words[index]));
            }
            None => {
                current = Some((index, unit_end, unit_words[index]));
            }
        }
    }
    if let Some((first, end, _)) = current {
        blocks.push(build_block(source_path, lines.as_slice(), units[first].0, end));
    }
    for (offset, block) in blocks.iter_mut().enumerate() {
        block.id = format!("b{}", offset + 1);
    }
    Ok(blocks)
}

fn build_block(
    source_path: &str,
    lines: &[&str],
    start_index: usize,
    end_index_exclusive: usize,
) -> ExtractionBlock {
    let text = lines[start_index..end_index_exclusive].concat();
    let word_count = text.split_whitespace().count();
    ExtractionBlock {
        id: String::new(),
        source_path: source_path.to_owned(),
        start_line: start_index as u32 + 1,
        end_line: end_index_exclusive as u32,
        text,
        word_count,
    }
}

/// Render a block's lines with absolute one-based "N: " prefixes, mirroring
/// the graph CLI's passage format so the model can copy line numbers into
/// citations instead of counting them.
pub fn render_labeled_block_text(block: &ExtractionBlock, snapshot: &str) -> String {
    let lines: Vec<&str> = snapshot.split_inclusive('\n').collect();
    let start = (block.start_line - 1) as usize;
    let end = block.end_line as usize;
    (start..end)
        .map(|offset| {
            let content = lines[offset].trim_end_matches(['\r', '\n']);
            format!("{}: {content}", offset + 1)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Return the exact bytes represented by a one-based inclusive line range of
/// the snapshot. Mirrors the graph CLI's snapshot line lookup.
pub fn snapshot_lines(snapshot: &str, start_line: u32, end_line: u32) -> Option<String> {
    if start_line == 0 || end_line < start_line {
        return None;
    }
    let lines: Vec<&str> = snapshot.split_inclusive('\n').collect();
    let start = usize::try_from(start_line - 1).ok()?;
    let end = usize::try_from(end_line).ok()?;
    if end > lines.len() {
        return None;
    }
    Some(lines[start..end].concat())
}

/// Strict evidence matching, reimplemented from the graph CLI's approach:
/// the proposed text must equal the snapshot text as an exact sequence of
/// whitespace-separated words. Whitespace runs (including line breaks) are
/// the only normalization; wording, punctuation, and truncation are not
/// forgiven — a partial quote is a mismatch, and there is no prefix
/// correction or fuzzy re-anchoring.
pub fn evidence_text_matches_snapshot(proposed: &str, snapshot: &str) -> bool {
    !proposed.trim().is_empty() && proposed.split_whitespace().eq(snapshot.split_whitespace())
}

/// The model's response for one block. Everything is optional at the serde
/// level so a partially malformed response still parses far enough to be
/// recorded and validated; anything missing simply fails validation.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct ExtractionResponse {
    #[serde(default)]
    pub relations: Vec<ExtractedRelation>,
    #[serde(default)]
    pub new_entity_kinds: Vec<ProposedEntityKind>,
    #[serde(default)]
    pub new_predicates: Vec<ProposedPredicate>,
    #[serde(default)]
    pub running_summary: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ExtractedRelation {
    pub subject: ExtractedEndpoint,
    pub predicate: String,
    pub object: ExtractedEndpoint,
    #[serde(default)]
    pub evidence: Vec<ExtractedEvidence>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ExtractedEndpoint {
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ExtractedEvidence {
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
}

/// A claim that passed both schema and evidence validation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidClaim {
    pub block_id: String,
    pub source_path: String,
    pub subject: ExtractedEndpoint,
    pub predicate: String,
    pub object: ExtractedEndpoint,
    pub evidence: Vec<ExtractedEvidence>,
}

/// A claim that failed validation, kept verbatim with its failure reasons —
/// never silently dropped.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InvalidClaim {
    pub block_id: String,
    pub source_path: String,
    pub relation: ExtractedRelation,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MintTarget {
    EntityKind,
    Predicate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MintRejection {
    pub target: MintTarget,
    pub name: String,
    pub reason: String,
}

/// One block's extraction outcome, including exactly which entries were
/// minted into the running vocabulary and which mint attempts were rejected.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BlockExtractionOutcome {
    pub block_id: String,
    pub parse_error: Option<String>,
    pub valid_claims: Vec<ValidClaim>,
    pub invalid_claims: Vec<InvalidClaim>,
    pub minted_entity_kinds: Vec<SeedEntityKind>,
    pub minted_predicates: Vec<SeedPredicate>,
    pub rejected_mints: Vec<MintRejection>,
    pub running_summary: String,
}

/// The accumulated open vocabulary for one run: Task 2's seed plus every
/// entry minted so far. This is a soft prior — the prompt biases the model
/// toward reuse but permits minting, and validation only requires that
/// whatever the model used is declared with a recorded meaning and shape.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VocabularyState {
    pub entity_kinds: Vec<SeedEntityKind>,
    pub predicates: Vec<SeedPredicate>,
}

impl VocabularyState {
    pub fn from_seed(seed: &SeedVocabulary) -> Self {
        Self {
            entity_kinds: seed.entity_kinds.clone(),
            predicates: seed.predicates.clone(),
        }
    }

    pub fn kind(&self, name: &str) -> Option<&SeedEntityKind> {
        self.entity_kinds.iter().find(|entry| entry.kind == name)
    }

    pub fn predicate(&self, name: &str) -> Option<&SeedPredicate> {
        self.predicates.iter().find(|entry| entry.predicate == name)
    }

    pub fn kind_names(&self) -> Vec<String> {
        self.entity_kinds.iter().map(|entry| entry.kind.clone()).collect()
    }

    pub fn predicate_names(&self) -> Vec<String> {
        self.predicates
            .iter()
            .map(|entry| entry.predicate.clone())
            .collect()
    }
}

/// Same rules as consolidation's vocabulary validation, applied to the seed
/// before a run starts: unique non-empty names and predicate shapes that
/// reference declared entity kinds.
pub fn validate_seed_vocabulary(seed: &SeedVocabulary) -> anyhow::Result<()> {
    anyhow::ensure!(
        !seed.entity_kinds.is_empty(),
        "seed vocabulary contains no entity kinds"
    );
    let kinds: BTreeSet<&str> = seed.entity_kinds.iter().map(|entry| entry.kind.as_str()).collect();
    anyhow::ensure!(
        kinds.len() == seed.entity_kinds.len(),
        "seed vocabulary declares an entity kind more than once"
    );
    let mut predicates = BTreeSet::new();
    for predicate in &seed.predicates {
        anyhow::ensure!(
            predicates.insert(predicate.predicate.as_str()),
            "seed vocabulary declares predicate {:?} more than once",
            predicate.predicate
        );
        for shape in [&predicate.subject_kind, &predicate.object_kind] {
            anyhow::ensure!(
                kinds.contains(shape.as_str()),
                "seed predicate {:?} references undeclared entity kind {shape:?}",
                predicate.predicate
            );
        }
    }
    Ok(())
}

/// User payload for one extraction call: the labeled block, the running
/// summary, and the current accumulated vocabulary.
pub fn build_extraction_payload(
    block: &ExtractionBlock,
    source_path: &str,
    snapshot: &str,
    running_summary: &str,
    vocabulary: &VocabularyState,
) -> serde_json::Value {
    serde_json::json!({
        "block": {
            "id": block.id,
            "source": source_path,
            "start_line": block.start_line,
            "end_line": block.end_line,
            "text": render_labeled_block_text(block, snapshot),
        },
        "running_summary": running_summary,
        "vocabulary": {
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
        },
    })
}

/// Parse one raw response, apply its mints to the running vocabulary, and
/// validate every relation against the open vocabulary and the real
/// snapshot. The caller must have recorded `raw_response` to disk before
/// calling this so raw output predates any validation decision.
pub fn process_block_response(
    block: &ExtractionBlock,
    snapshot: &str,
    vocabulary: &mut VocabularyState,
    raw_response: &str,
) -> BlockExtractionOutcome {
    let mut outcome = BlockExtractionOutcome {
        block_id: block.id.clone(),
        ..Default::default()
    };
    let response = match serde_json::from_str::<ExtractionResponse>(raw_response.trim()) {
        Ok(response) => response,
        Err(error) => {
            outcome.parse_error =
                Some(format!("response is not a valid extraction JSON object: {error}"));
            return outcome;
        }
    };

    // Kinds mint first so a minted predicate's shape can reference them.
    for mint in &response.new_entity_kinds {
        let name = mint.kind.trim();
        if name.is_empty() {
            outcome.rejected_mints.push(MintRejection {
                target: MintTarget::EntityKind,
                name: mint.kind.clone(),
                reason: "entity kind name is empty".to_owned(),
            });
        } else if mint.description.trim().is_empty() {
            outcome.rejected_mints.push(MintRejection {
                target: MintTarget::EntityKind,
                name: name.to_owned(),
                reason: "entity kind has no recorded meaning (empty description)".to_owned(),
            });
        } else if vocabulary.kind(name).is_some() {
            // Re-declaration of an existing kind: reuse, not a mint.
        } else {
            let entry = SeedEntityKind {
                kind: name.to_owned(),
                description: mint.description.trim().to_owned(),
                merged_from: Vec::new(),
            };
            vocabulary.entity_kinds.push(entry.clone());
            outcome.minted_entity_kinds.push(entry);
        }
    }
    for mint in &response.new_predicates {
        let name = mint.predicate.trim();
        if name.is_empty() {
            outcome.rejected_mints.push(MintRejection {
                target: MintTarget::Predicate,
                name: mint.predicate.clone(),
                reason: "predicate name is empty".to_owned(),
            });
            continue;
        }
        if mint.description.trim().is_empty() {
            outcome.rejected_mints.push(MintRejection {
                target: MintTarget::Predicate,
                name: name.to_owned(),
                reason: "predicate has no recorded meaning (empty description)".to_owned(),
            });
            continue;
        }
        let mut shape_errors = Vec::new();
        for shape in [&mint.subject_kind, &mint.object_kind] {
            if vocabulary.kind(shape.trim()).is_none() {
                shape_errors.push(format!(
                    "shape references undeclared entity kind {shape:?}"
                ));
            }
        }
        if !shape_errors.is_empty() {
            outcome.rejected_mints.push(MintRejection {
                target: MintTarget::Predicate,
                name: name.to_owned(),
                reason: shape_errors.join("; "),
            });
            continue;
        }
        if vocabulary.predicate(name).is_some() {
            // Re-declaration of an existing predicate: reuse, not a mint.
            continue;
        }
        let entry = SeedPredicate {
            predicate: name.to_owned(),
            description: mint.description.trim().to_owned(),
            subject_kind: mint.subject_kind.trim().to_owned(),
            object_kind: mint.object_kind.trim().to_owned(),
            merged_from: Vec::new(),
        };
        vocabulary.predicates.push(entry.clone());
        outcome.minted_predicates.push(entry);
    }

    for relation in &response.relations {
        match validate_relation(relation, vocabulary, block, snapshot) {
            Ok(evidence) => outcome.valid_claims.push(ValidClaim {
                block_id: block.id.clone(),
                source_path: block.source_path.clone(),
                subject: relation.subject.clone(),
                predicate: relation.predicate.trim().to_owned(),
                object: relation.object.clone(),
                evidence,
            }),
            Err(reasons) => outcome.invalid_claims.push(InvalidClaim {
                block_id: block.id.clone(),
                source_path: block.source_path.clone(),
                relation: relation.clone(),
                reasons,
            }),
        }
    }
    outcome.running_summary = response.running_summary.trim().to_owned();
    outcome
}

/// Open-vocabulary schema + evidence validation for one relation. All
/// failures are collected so the recorded reasons give a complete picture.
fn validate_relation(
    relation: &ExtractedRelation,
    vocabulary: &VocabularyState,
    block: &ExtractionBlock,
    snapshot: &str,
) -> Result<Vec<ExtractedEvidence>, Vec<String>> {
    let mut reasons = Vec::new();
    let predicate_name = relation.predicate.trim();
    if predicate_name.is_empty() {
        reasons.push("predicate is empty".to_owned());
    } else {
        match vocabulary.predicate(predicate_name) {
            None => reasons.push(format!(
                "predicate {predicate_name:?} is not in the vocabulary and was not minted in this response"
            )),
            Some(declared) => {
                if relation.subject.kind.trim() != declared.subject_kind {
                    reasons.push(format!(
                        "predicate {predicate_name:?} expects subject kind {:?}, got {:?}",
                        declared.subject_kind, relation.subject.kind
                    ));
                }
                if relation.object.kind.trim() != declared.object_kind {
                    reasons.push(format!(
                        "predicate {predicate_name:?} expects object kind {:?}, got {:?}",
                        declared.object_kind, relation.object.kind
                    ));
                }
            }
        }
    }
    if relation.subject.name.trim().is_empty() {
        reasons.push("subject name is empty".to_owned());
    }
    if relation.object.name.trim().is_empty() {
        reasons.push("object name is empty".to_owned());
    }
    for (position, kind) in [("subject", relation.subject.kind.trim()), ("object", relation.object.kind.trim())] {
        if vocabulary.kind(kind).is_none() {
            reasons.push(format!("{position} entity kind {kind:?} is not declared in the vocabulary"));
        }
    }
    if relation.evidence.is_empty() {
        reasons.push("claim has no evidence".to_owned());
    }
    let mut validated = Vec::new();
    for (index, item) in relation.evidence.iter().enumerate() {
        let label = format!("evidence[{index}]");
        if item.start_line == 0 || item.end_line < item.start_line {
            reasons.push(format!(
                "{label} cites an invalid line range {}-{}",
                item.start_line, item.end_line
            ));
            continue;
        }
        // Current block only: a carried summary is context, never evidence.
        if item.start_line < block.start_line || item.end_line > block.end_line {
            reasons.push(format!(
                "{label} cites lines {}-{} outside the current block ({}-{}); a carried summary is not evidence",
                item.start_line, item.end_line, block.start_line, block.end_line
            ));
            continue;
        }
        let Some(expected) = snapshot_lines(snapshot, item.start_line, item.end_line) else {
            reasons.push(format!(
                "{label} cites lines {}-{} beyond the end of the source",
                item.start_line, item.end_line
            ));
            continue;
        };
        if !evidence_text_matches_snapshot(&item.text, &expected) {
            reasons.push(format!(
                "{label} text does not match snapshot lines {}-{} (strict exact match after whitespace normalization required)",
                item.start_line, item.end_line
            ));
            continue;
        }
        validated.push(item.clone());
    }
    if reasons.is_empty() {
        Ok(validated)
    } else {
        Err(reasons)
    }
}

/// Observable vocabulary-reuse measurements for one run, computed from the
/// validated claims: which predicates a run actually reused across blocks,
/// which seed predicates survived contact with the text, and which minted
/// predicates earned at least one valid relation.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ReuseReport {
    /// predicate -> sorted block ids where a valid relation used it.
    pub predicate_block_usage: BTreeMap<String, Vec<String>>,
    /// Predicates with valid relations in more than one block.
    pub reused_across_blocks: Vec<String>,
    /// The subset of `reused_across_blocks` that came from the seed.
    pub reused_seed_predicates: Vec<String>,
    /// All predicates minted during the run.
    pub minted_predicates: Vec<String>,
    /// Minted predicates with at least one valid relation.
    pub minted_predicates_with_valid_use: Vec<String>,
    /// predicate -> distinct subject-kind -> object-kind shapes among valid
    /// relations, so "same predicate, same repeated relation shape" is
    /// checkable rather than asserted.
    pub predicate_shapes: BTreeMap<String, Vec<String>>,
}

pub fn reuse_report(
    valid_claims: &[ValidClaim],
    seed_predicates: &[String],
    minted_predicates: &[String],
) -> ReuseReport {
    let mut usage: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut shapes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for claim in valid_claims {
        let blocks = usage.entry(claim.predicate.clone()).or_default();
        if !blocks.contains(&claim.block_id) {
            blocks.push(claim.block_id.clone());
        }
        shapes
            .entry(claim.predicate.clone())
            .or_default()
            .insert(format!("{} -> {}", claim.subject.kind, claim.object.kind));
    }
    for blocks in usage.values_mut() {
        blocks.sort();
    }
    let reused_across_blocks: Vec<String> = usage
        .iter()
        .filter(|(_, blocks)| blocks.len() > 1)
        .map(|(predicate, _)| predicate.clone())
        .collect();
    let reused_seed_predicates: Vec<String> = reused_across_blocks
        .iter()
        .filter(|name| seed_predicates.contains(name))
        .cloned()
        .collect();
    let minted_predicates_with_valid_use: Vec<String> = minted_predicates
        .iter()
        .filter(|name| usage.contains_key(name.as_str()))
        .cloned()
        .collect();
    ReuseReport {
        predicate_shapes: shapes
            .into_iter()
            .map(|(predicate, set)| (predicate, set.into_iter().collect()))
            .collect(),
        reused_across_blocks,
        reused_seed_predicates,
        predicate_block_usage: usage,
        minted_predicates: minted_predicates.to_owned(),
        minted_predicates_with_valid_use,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document_with_paragraphs(paragraphs: &[&str]) -> String {
        paragraphs
            .iter()
            .map(|paragraph| format!("{paragraph}\n\n"))
            .collect()
    }

    fn seed_vocabulary() -> SeedVocabulary {
        serde_json::from_str(
            r#"{
            "entity_kinds": [
                {"kind": "CLI", "description": "A command-line tool or interface."},
                {"kind": "Database", "description": "A storage system for indexed data."},
                {"kind": "SearchIndex", "description": "A specific indexing technology."}
            ],
            "predicates": [
                {"predicate": "uses", "description": "A CLI utilizes a database.", "subject_kind": "CLI", "object_kind": "Database"},
                {"predicate": "degrades_to", "description": "A fallback when a primary index fails.", "subject_kind": "SearchIndex", "object_kind": "SearchIndex"}
            ],
            "example_relations": []
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn blocks_respect_the_word_budget_without_breaking_lines() {
        let content = &document_with_paragraphs(&[
            "first paragraph with five words here now",
            "second paragraph also with exactly seven words in total",
            "third",
        ]);
        for budget in [4u32, 7, 12, 100] {
            let blocks = split_into_budget_blocks("doc.md", content, budget).unwrap();
            let lines: Vec<&str> = content.split_inclusive('\n').collect();
            for block in &blocks {
                // The block text is an exact slice of physical source lines.
                let expected: String = lines
                    [(block.start_line - 1) as usize..block.end_line as usize]
                    .concat();
                assert_eq!(block.text, expected, "budget {budget}");
                if block.word_count > budget as usize {
                    // Only a single oversized paragraph may exceed the budget.
                    assert_eq!(block.start_line, block.end_line, "budget {budget}");
                }
            }
            // Every non-blank line lands in exactly one block.
            let covered: usize = blocks.iter().map(|b| b.word_count).sum();
            assert_eq!(covered, content.split_whitespace().count(), "budget {budget}");
        }
    }

    #[test]
    fn blocks_prefer_paragraph_boundaries_and_stay_whole() {
        let content = &document_with_paragraphs(&[
            "alpha one two three four five six seven eight",
            "beta one two three four five six seven eight",
            "gamma one two",
        ]);
        let blocks = split_into_budget_blocks("doc.md", content, 10).unwrap();
        assert!(blocks.len() >= 2);
        for block in &blocks {
            // No block ends mid-paragraph: its last physical line is either
            // followed by a blank line or is the file's final line.
            let last_line = block.text.lines().last().unwrap();
            assert!(!last_line.trim().is_empty());
        }
        // alpha and beta never share a block (18 words > 10).
        let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
        assert!(texts.iter().any(|t| t.contains("alpha") && !t.contains("beta")));
        assert!(texts.iter().any(|t| t.contains("beta")));
    }

    #[test]
    fn oversized_single_paragraph_becomes_its_own_block() {
        let content = &document_with_paragraphs(&["one two three", "four five six seven eight nine ten eleven twelve"]);
        let blocks = split_into_budget_blocks("doc.md", content, 5).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].word_count, 3);
        assert_eq!(blocks[1].word_count, 9);
        assert_eq!(blocks[1].start_line, 3);
        assert_eq!(blocks[1].end_line, 3);
    }

    #[test]
    fn zero_word_budget_is_rejected() {
        assert!(split_into_budget_blocks("doc.md", "text\n", 0).is_err());
    }

    #[test]
    fn labeled_block_text_carries_absolute_line_numbers() {
        let content = "first\n\nsecond\nthird\n";
        // Budget 2 keeps the one-word first paragraph and the two-word
        // second paragraph in separate blocks.
        let blocks = split_into_budget_blocks("doc.md", content, 2).unwrap();
        let second = &blocks[1];
        assert_eq!(render_labeled_block_text(second, content), "3: second\n4: third");
    }

    #[test]
    fn strict_evidence_matching_forgives_only_whitespace() {
        let snapshot = "alpha beta  gamma\n\ndelta\n";
        assert!(evidence_text_matches_snapshot("alpha beta  gamma\n\ndelta", snapshot));
        assert!(evidence_text_matches_snapshot(
            "alpha\n beta\tgamma\ndelta",
            snapshot
        ));
        assert!(evidence_text_matches_snapshot(
            "  alpha beta gamma delta  ",
            snapshot
        ));
        assert!(
            !evidence_text_matches_snapshot("alpha beta gamma", snapshot),
            "truncation is a mismatch"
        );
        assert!(!evidence_text_matches_snapshot("alpha beta gamma delta extra", snapshot));
        assert!(!evidence_text_matches_snapshot("alpha beta delta gamma", snapshot), "wrong order");
        assert!(!evidence_text_matches_snapshot("alpha beta delta", snapshot), "wrong words");
        assert!(!evidence_text_matches_snapshot("", snapshot));
        assert!(!evidence_text_matches_snapshot("   ", snapshot));
    }

    #[test]
    fn snapshot_lines_returns_exact_ranges_and_rejects_bad_ones() {
        let snapshot = "one\ntwo\nthree\n";
        assert_eq!(snapshot_lines(snapshot, 2, 3).unwrap(), "two\nthree\n");
        assert_eq!(snapshot_lines(snapshot, 3, 3).unwrap(), "three\n");
        assert!(snapshot_lines(snapshot, 0, 1).is_none());
        assert!(snapshot_lines(snapshot, 3, 2).is_none());
        assert!(snapshot_lines(snapshot, 1, 4).is_none());
    }

    fn relation(predicate: &str, subject_kind: &str, object_kind: &str, evidence: Vec<ExtractedEvidence>) -> ExtractedRelation {
        ExtractedRelation {
            subject: ExtractedEndpoint { kind: subject_kind.to_owned(), name: "subject".to_owned() },
            predicate: predicate.to_owned(),
            object: ExtractedEndpoint { kind: object_kind.to_owned(), name: "object".to_owned() },
            evidence,
        }
    }

    fn block(id: &str, start: u32, end: u32) -> ExtractionBlock {
        ExtractionBlock {
            id: id.to_owned(),
            source_path: "doc.md".to_owned(),
            start_line: start,
            end_line: end,
            text: String::new(),
            word_count: 0,
        }
    }

    fn evidence_item(start: u32, end: u32, text: &str) -> ExtractedEvidence {
        ExtractedEvidence {
            start_line: start,
            end_line: end,
            text: text.to_owned(),
        }
    }

    const SNAPSHOT: &str = "block one line a\nblock one line b\n\nblock two line a\nblock two line b\n";

    #[test]
    fn valid_response_yields_valid_claims_and_records_running_summary() {
        let mut vocabulary = VocabularyState::from_seed(&seed_vocabulary());
        let raw = serde_json::to_string(&serde_json::json!({
            "relations": [{
                "subject": {"kind": "CLI", "name": "tool"},
                "predicate": "uses",
                "object": {"kind": "Database", "name": "store"},
                "evidence": [{"start_line": 1, "end_line": 2, "text": "block one line a\nblock one line b"}]
            }],
            "new_entity_kinds": [],
            "new_predicates": [],
            "running_summary": "block one covered"
        }))
        .unwrap();
        let outcome = process_block_response(&block("b1", 1, 2), SNAPSHOT, &mut vocabulary, &raw);
        assert!(outcome.parse_error.is_none());
        assert_eq!(outcome.valid_claims.len(), 1);
        assert!(outcome.invalid_claims.is_empty());
        assert_eq!(outcome.valid_claims[0].evidence.len(), 1);
        assert_eq!(outcome.running_summary, "block one covered");
    }

    #[test]
    fn shape_violations_are_reported_invalid_not_silently_dropped() {
        let mut vocabulary = VocabularyState::from_seed(&seed_vocabulary());
        // uses expects CLI -> Database; the claim has CLI -> SearchIndex.
        let raw = serde_json::to_string(&serde_json::json!({
            "relations": [{
                "subject": {"kind": "CLI", "name": "tool"},
                "predicate": "uses",
                "object": {"kind": "SearchIndex", "name": "store"},
                "evidence": [{"start_line": 1, "end_line": 1, "text": "block one line a"}]
            }],
            "new_entity_kinds": [],
            "new_predicates": [],
            "running_summary": ""
        }))
        .unwrap();
        let outcome = process_block_response(&block("b1", 1, 2), SNAPSHOT, &mut vocabulary, &raw);
        assert!(outcome.valid_claims.is_empty());
        assert_eq!(outcome.invalid_claims.len(), 1);
        let reasons = outcome.invalid_claims[0].reasons.join("; ");
        assert!(reasons.contains("expects object kind"), "{reasons}");
        // The invalid claim keeps the full relation verbatim.
        assert_eq!(outcome.invalid_claims[0].relation.predicate, "uses");
    }

    #[test]
    fn undeclared_predicate_without_a_mint_is_invalid_and_recorded() {
        let mut vocabulary = VocabularyState::from_seed(&seed_vocabulary());
        let raw = serde_json::to_string(&serde_json::json!({
            "relations": [{
                "subject": {"kind": "CLI", "name": "tool"},
                "predicate": "indexes",
                "object": {"kind": "Database", "name": "store"},
                "evidence": [{"start_line": 1, "end_line": 1, "text": "block one line a"}]
            }],
            "new_entity_kinds": [],
            "new_predicates": [],
            "running_summary": ""
        }))
        .unwrap();
        let outcome = process_block_response(&block("b1", 1, 2), SNAPSHOT, &mut vocabulary, &raw);
        assert!(outcome.valid_claims.is_empty());
        let reasons = outcome.invalid_claims[0].reasons.join("; ");
        assert!(reasons.contains("not in the vocabulary and was not minted"), "{reasons}");
    }

    #[test]
    fn genuinely_new_predicate_is_minted_and_then_validates() {
        let mut vocabulary = VocabularyState::from_seed(&seed_vocabulary());
        let raw = serde_json::to_string(&serde_json::json!({
            "relations": [{
                "subject": {"kind": "Command", "name": "cargo check"},
                "predicate": "serves_as",
                "object": {"kind": "Process", "name": "correctness gate"},
                "evidence": [{"start_line": 1, "end_line": 1, "text": "block one line a"}]
            }],
            "new_entity_kinds": [
                {"kind": "Command", "description": "A runnable command."},
                {"kind": "Process", "description": "A project process or role."}
            ],
            "new_predicates": [
                {"predicate": "serves_as", "description": "Fulfills a role.", "subject_kind": "Command", "object_kind": "Process"}
            ],
            "running_summary": ""
        }))
        .unwrap();
        let outcome = process_block_response(&block("b1", 1, 2), SNAPSHOT, &mut vocabulary, &raw);
        assert_eq!(outcome.minted_entity_kinds.len(), 2);
        assert_eq!(outcome.minted_predicates.len(), 1);
        assert_eq!(outcome.minted_predicates[0].predicate, "serves_as");
        assert_eq!(outcome.minted_predicates[0].subject_kind, "Command");
        assert_eq!(outcome.minted_predicates[0].object_kind, "Process");
        assert_eq!(outcome.valid_claims.len(), 1);
        // The mint is durable: a later block can reuse it without re-minting.
        let later = serde_json::to_string(&serde_json::json!({
            "relations": [{
                "subject": {"kind": "Command", "name": "cargo test"},
                "predicate": "serves_as",
                "object": {"kind": "Process", "name": "regression gate"},
                "evidence": [{"start_line": 4, "end_line": 4, "text": "block two line a"}]
            }],
            "new_entity_kinds": [],
            "new_predicates": [],
            "running_summary": ""
        }))
        .unwrap();
        let outcome = process_block_response(&block("b2", 4, 5), SNAPSHOT, &mut vocabulary, &later);
        assert_eq!(outcome.valid_claims.len(), 1);
        assert!(outcome.minted_predicates.is_empty(), "reuse must not re-mint");
        assert_eq!(vocabulary.predicate_names().len(), 3);
    }

    #[test]
    fn mints_without_meaning_or_with_dangling_shapes_are_rejected_and_recorded() {
        let mut vocabulary = VocabularyState::from_seed(&seed_vocabulary());
        let raw = serde_json::to_string(&serde_json::json!({
            "relations": [],
            "new_entity_kinds": [{"kind": "Ghost", "description": ""}],
            "new_predicates": [
                {"predicate": "relates", "description": "Vague.", "subject_kind": "Ghost", "object_kind": "CLI"},
                {"predicate": "links", "description": "No description beyond this.", "subject_kind": "", "object_kind": "CLI"}
            ],
            "running_summary": ""
        }))
        .unwrap();
        let outcome = process_block_response(&block("b1", 1, 2), SNAPSHOT, &mut vocabulary, &raw);
        assert!(outcome.minted_entity_kinds.is_empty());
        assert!(outcome.minted_predicates.is_empty());
        assert_eq!(outcome.rejected_mints.len(), 3);
        assert!(outcome.rejected_mints[0].reason.contains("no recorded meaning"));
        assert!(outcome.rejected_mints[1].reason.contains("undeclared entity kind"));
        assert!(outcome.rejected_mints[2].reason.contains("undeclared entity kind"));
    }

    #[test]
    fn claims_citing_outside_the_current_block_are_invalid() {
        let mut vocabulary = VocabularyState::from_seed(&seed_vocabulary());
        // Valid in block one...
        let first = serde_json::to_string(&serde_json::json!({
            "relations": [{
                "subject": {"kind": "CLI", "name": "tool"},
                "predicate": "uses",
                "object": {"kind": "Database", "name": "store"},
                "evidence": [{"start_line": 1, "end_line": 1, "text": "block one line a"}]
            }],
            "new_entity_kinds": [],
            "new_predicates": [],
            "running_summary": "tool uses store"
        }))
        .unwrap();
        let outcome = process_block_response(&block("b1", 1, 2), SNAPSHOT, &mut vocabulary, &first);
        assert_eq!(outcome.valid_claims.len(), 1);

        // ...but the same relation in block two citing block one's lines is
        // carried-summary evidence and must be rejected.
        let second = serde_json::to_string(&serde_json::json!({
            "relations": [{
                "subject": {"kind": "CLI", "name": "tool"},
                "predicate": "uses",
                "object": {"kind": "Database", "name": "store"},
                "evidence": [{"start_line": 1, "end_line": 1, "text": "block one line a"}]
            }],
            "new_entity_kinds": [],
            "new_predicates": [],
            "running_summary": ""
        }))
        .unwrap();
        let outcome = process_block_response(&block("b2", 4, 5), SNAPSHOT, &mut vocabulary, &second);
        assert!(outcome.valid_claims.is_empty());
        let reasons = outcome.invalid_claims[0].reasons.join("; ");
        assert!(reasons.contains("outside the current block"), "{reasons}");
        assert!(reasons.contains("not evidence"), "{reasons}");
    }

    #[test]
    fn truncated_or_prefixed_evidence_text_fails_strict_matching() {
        let mut vocabulary = VocabularyState::from_seed(&seed_vocabulary());
        let raw = serde_json::to_string(&serde_json::json!({
            "relations": [
                {
                    "subject": {"kind": "CLI", "name": "tool"},
                    "predicate": "uses",
                    "object": {"kind": "Database", "name": "store"},
                    "evidence": [{"start_line": 1, "end_line": 2, "text": "block one line a"}]
                },
                {
                    "subject": {"kind": "CLI", "name": "tool"},
                    "predicate": "uses",
                    "object": {"kind": "Database", "name": "store"},
                    "evidence": [{"start_line": 1, "end_line": 1, "text": "1: block one line a"}]
                },
                {
                    "subject": {"kind": "CLI", "name": "tool"},
                    "predicate": "uses",
                    "object": {"kind": "Database", "name": "store"},
                    "evidence": [{"start_line": 9, "end_line": 9, "text": "beyond"}]
                }
            ],
            "new_entity_kinds": [],
            "new_predicates": [],
            "running_summary": ""
        }))
        .unwrap();
        let outcome = process_block_response(&block("b1", 1, 2), SNAPSHOT, &mut vocabulary, &raw);
        assert!(outcome.valid_claims.is_empty());
        assert_eq!(outcome.invalid_claims.len(), 3);
        let joined = outcome
            .invalid_claims
            .iter()
            .flat_map(|claim| claim.reasons.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        assert!(joined.contains("does not match snapshot lines 1-2"), "{joined}");
        assert!(joined.contains("does not match snapshot lines 1-1"), "{joined}");
        // A line outside the current block fails the current-block rule
        // before any snapshot lookup happens.
        assert!(joined.contains("outside the current block"), "{joined}");
    }

    #[test]
    fn unparseable_response_is_recorded_as_a_parse_error_with_no_claims() {
        let mut vocabulary = VocabularyState::from_seed(&seed_vocabulary());
        let outcome = process_block_response(&block("b1", 1, 2), SNAPSHOT, &mut vocabulary, "not json");
        assert!(outcome.parse_error.is_some());
        assert!(outcome.valid_claims.is_empty() && outcome.invalid_claims.is_empty());
        assert!(outcome.running_summary.is_empty());
    }

    #[test]
    fn seed_vocabulary_validation_rejects_dangling_shapes_and_duplicates() {
        let mut seed = seed_vocabulary();
        assert!(validate_seed_vocabulary(&seed).is_ok());
        seed.predicates[0].object_kind = "Ghost".to_owned();
        assert!(validate_seed_vocabulary(&seed)
            .unwrap_err()
            .to_string()
            .contains("undeclared entity kind"));
        let mut seed = seed_vocabulary();
        seed.entity_kinds.push(seed.entity_kinds[0].clone());
        assert!(validate_seed_vocabulary(&seed)
            .unwrap_err()
            .to_string()
            .contains("more than once"));
    }

    #[test]
    fn reuse_report_detects_cross_block_reuse_and_minted_use() {
        let valid = vec![
            ValidClaim {
                block_id: "b1".to_owned(),
                source_path: "doc.md".to_owned(),
                subject: ExtractedEndpoint { kind: "CLI".to_owned(), name: "a".to_owned() },
                predicate: "uses".to_owned(),
                object: ExtractedEndpoint { kind: "Database".to_owned(), name: "b".to_owned() },
                evidence: Vec::new(),
            },
            ValidClaim {
                block_id: "b3".to_owned(),
                source_path: "doc.md".to_owned(),
                subject: ExtractedEndpoint { kind: "CLI".to_owned(), name: "c".to_owned() },
                predicate: "uses".to_owned(),
                object: ExtractedEndpoint { kind: "Database".to_owned(), name: "d".to_owned() },
                evidence: Vec::new(),
            },
            ValidClaim {
                block_id: "b2".to_owned(),
                source_path: "doc.md".to_owned(),
                subject: ExtractedEndpoint { kind: "Command".to_owned(), name: "check".to_owned() },
                predicate: "serves_as".to_owned(),
                object: ExtractedEndpoint { kind: "Process".to_owned(), name: "gate".to_owned() },
                evidence: Vec::new(),
            },
        ];
        let report = reuse_report(&valid, &["uses".to_owned()], &["serves_as".to_owned()]);
        assert_eq!(report.reused_across_blocks, vec!["uses"]);
        assert_eq!(report.reused_seed_predicates, vec!["uses"]);
        assert_eq!(report.predicate_block_usage["uses"], vec!["b1", "b3"]);
        assert_eq!(report.minted_predicates_with_valid_use, vec!["serves_as"]);
        assert_eq!(report.predicate_shapes["uses"], vec!["CLI -> Database"]);
    }

    #[test]
    fn extraction_payload_carries_block_labels_summary_and_vocabulary() {
        let content = "one\n\ntwo three\n";
        let blocks = split_into_budget_blocks("doc.md", content, 2).unwrap();
        let vocabulary = VocabularyState::from_seed(&seed_vocabulary());
        let payload = build_extraction_payload(&blocks[1], "doc.md", content, "so far", &vocabulary);
        let serialized = payload.to_string();
        assert!(serialized.contains("3: two three"), "{serialized}");
        assert!(serialized.contains("\"running_summary\":\"so far\""));
        assert!(serialized.contains("\"subject_kind\":\"CLI\""));
        assert!(serialized.contains("\"description\":\"A command-line tool or interface.\""));
    }
}
