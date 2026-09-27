//! Access to provenance graph storage in the repository's shared database.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

use crate::{repository_db_path, IndexerConfig};

static NEXT_BUILD_ID: AtomicU64 = AtomicU64::new(0);

pub mod model;

/// A source selected for a graph build, read from the current working tree.
/// `bytes` preserves the exact input for hashing and snapshotting; `text` is
/// its validated UTF-8 representation for extraction.
#[derive(Debug, Clone)]
pub struct SelectedSource {
    pub path: String,
    pub bytes: Vec<u8>,
    pub text: String,
}

/// A heading-aware, immutable passage that an extractor can cite.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourcePassage {
    pub id: String,
    pub source_version_id: i64,
    pub heading_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
}

/// A selected source tied to the exact persisted snapshot used for extraction.
#[derive(Debug, Clone)]
pub struct SnapshottedSource {
    pub path: String,
    pub content_hash: String,
    pub source_version_id: i64,
    pub git_head: Option<String>,
    pub passages: Vec<SourcePassage>,
}

/// A claim eligible for ordinary graph search in a completed build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCandidate {
    pub claim_id: String,
    pub claim_text: String,
    pub source_path: String,
    pub source_version_id: i64,
}

/// Interpreted question and FTS candidates, all constrained to one completed
/// build and its selected human-maintained source set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPreparation {
    pub build_id: String,
    pub interpretation: model::InterpretationResponse,
    /// Resolved canonical node keys for interpreted entities.
    pub resolved_entity_keys: Vec<String>,
    pub candidates: Vec<SearchCandidate>,
}

/// A positive graph proof. `claim_ids` contains the source claims that prove
/// the answer; `evidence_ids` contains every stored passage supporting them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositiveFinding {
    pub outcome: &'static str,
    pub claim_ids: Vec<String>,
    pub evidence_ids: Vec<String>,
}

/// A negative proof is either a current exclusion claim or an omission from a
/// current exhaustive language enumeration. The enumeration span is returned
/// alongside its claims so callers can cite the actual closed-set statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegativeFinding {
    pub outcome: &'static str,
    pub claim_ids: Vec<String>,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceResolution {
    Yes(PositiveFinding),
    No(NegativeFinding),
    Unknown {
        reason: String,
        claim_ids: Vec<String>,
        evidence_ids: Vec<String>,
    },
}

/// A citation rendered exclusively from a span in the selected build's
/// immutable source snapshot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RenderedCitation {
    pub citation: String,
    pub source_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    pub claim_ids: Vec<String>,
    pub span_id: String,
}

/// Stable text/JSON response shape for CLI and downstream callers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SearchAnswer {
    pub outcome: String,
    pub answer: String,
    pub citations: Vec<RenderedCitation>,
    pub claim_ids: Vec<String>,
    pub build_id: String,
}

/// Convert resolver IDs into a response, rejecting IDs that do not belong to
/// the selected build and verifying each stored span against its snapshot.
pub fn render_answer(
    db: &Connection,
    prepared: &SearchPreparation,
    resolution: EvidenceResolution,
) -> Result<SearchAnswer> {
    let (outcome, reason, mut claim_ids, mut evidence_ids) = match resolution {
        EvidenceResolution::Yes(f) => ("yes", None, f.claim_ids, f.evidence_ids),
        EvidenceResolution::No(f) => ("no", None, f.claim_ids, f.evidence_ids),
        EvidenceResolution::Unknown {
            reason,
            claim_ids,
            evidence_ids,
        } => ("unknown", Some(reason), claim_ids, evidence_ids),
    };
    let supported_list = if outcome == "yes"
        && prepared.interpretation.intent == model::QuestionIntent::ListCodeSearchLanguages
    {
        complete_supported_code_set(db, &prepared.build_id)?
    } else {
        None
    };
    if let Some((_, list_claims, list_evidence)) = &supported_list {
        claim_ids.extend(list_claims.iter().cloned());
        evidence_ids.extend(list_evidence.iter().cloned());
    }
    claim_ids.sort();
    claim_ids.dedup();
    evidence_ids.sort();
    evidence_ids.dedup();
    let answer = match (outcome, reason) {
        ("yes", _) => supported_list
            .as_ref()
            .map(|(names, _, _)| format!("Yes. Code search supports: {}.", names.join(", ")))
            .unwrap_or_else(|| {
                "Yes. The selected graph claims support this conclusion.".to_owned()
            }),
        ("no", _) => "No. The selected graph evidence supports this conclusion.".to_owned(),
        (_, Some(reason)) => format!("Unknown. Missing or unresolved evidence: {reason}."),
        _ => "Unknown. Missing evidence: no applicable current claim or exhaustive set was found."
            .to_owned(),
    };

    // An empty proof is not a citation, and every returned ID must resolve
    // within the completed build's explicit source membership.
    let mut citations = Vec::new();
    for span_id in evidence_ids {
        let mut stmt = db.prepare(
            "SELECT s.path, sp.start_line, sp.end_line, sp.text, sv.snapshot, c.id
             FROM graph_spans sp
             JOIN graph_source_versions sv ON sv.id=sp.source_version_id
             JOIN graph_sources s ON s.id=sv.source_id
             JOIN graph_build_sources bs ON bs.source_version_id=sv.id AND bs.source_id=s.id
             JOIN graph_claims c ON c.active=1
             WHERE sp.id=?1 AND bs.build_id=?2 AND EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=c.id AND bc.build_id=?2) AND c.active=1
               AND s.active=1 AND s.human_maintained=1
               AND (EXISTS (SELECT 1 FROM graph_claim_evidence ce
                            WHERE ce.span_id=sp.id AND ce.claim_id=c.id)
                    OR EXISTS (SELECT 1 FROM graph_enumerations en
                               JOIN graph_build_enumeration_members em ON em.enumeration_id=en.id AND em.build_id=?2
                               WHERE en.supporting_span_id=sp.id AND EXISTS (SELECT 1 FROM graph_build_enumerations be WHERE be.enumeration_id=en.id AND be.build_id=?2)
                                 AND em.claim_id=c.id))
             ORDER BY c.id",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![span_id, prepared.build_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, u32>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        anyhow::ensure!(
            !rows.is_empty(),
            "resolver returned citation span {span_id} outside selected completed build {}",
            prepared.build_id
        );
        let (path, start_line, end_line, text, snapshot, _) = rows[0].clone();
        let snapshot_text = snapshot_lines(&snapshot, start_line, end_line).ok_or_else(|| {
            anyhow::anyhow!("citation span {span_id} is outside its source snapshot")
        })?;
        anyhow::ensure!(
            snapshot_text == text,
            "stored citation span {span_id} does not match source snapshot"
        );
        let mut cited_claims = rows
            .into_iter()
            .map(|row| row.5)
            .filter(|id| claim_ids.contains(id))
            .collect::<Vec<_>>();
        cited_claims.sort();
        cited_claims.dedup();
        anyhow::ensure!(
            !cited_claims.is_empty(),
            "resolver returned span {span_id} without a selected claim ID"
        );
        citations.push(RenderedCitation {
            citation: format!("{path}:{start_line}-{end_line}"),
            source_path: path,
            start_line,
            end_line,
            text,
            claim_ids: cited_claims,
            span_id,
        });
    }
    for claim_id in &claim_ids {
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM graph_claims c JOIN graph_build_sources bs
             ON bs.build_id=?2 AND bs.source_version_id=c.source_version_id
             JOIN graph_sources s ON s.id=bs.source_id
             WHERE c.id=?1 AND EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=c.id AND bc.build_id=?2) AND c.active=1 AND s.active=1 AND s.human_maintained=1)",
            rusqlite::params![claim_id, prepared.build_id], |r| r.get(0),
        )?;
        anyhow::ensure!(
            exists,
            "resolver returned claim ID {claim_id} outside selected completed build {}",
            prepared.build_id
        );
        anyhow::ensure!(
            citations
                .iter()
                .any(|citation| citation.claim_ids.contains(claim_id)),
            "claim ID {claim_id} has no rendered evidence citation"
        );
    }
    citations.sort_by(|a, b| {
        (&a.source_path, a.start_line, a.end_line, &a.span_id).cmp(&(
            &b.source_path,
            b.start_line,
            b.end_line,
            &b.span_id,
        ))
    });
    Ok(SearchAnswer {
        outcome: outcome.to_owned(),
        answer,
        citations,
        claim_ids,
        build_id: prepared.build_id.clone(),
    })
}

/// Render a useful language answer only when the active project graph contains
/// explicit exhaustive language and dialect enumerations for an index currently
/// searched by code search. Their member claims and list spans become citations.
fn complete_supported_code_set(
    db: &Connection,
    build_id: &str,
) -> Result<Option<(Vec<String>, Vec<String>, Vec<String>)>> {
    let mut stmt = db.prepare(
        "SELECT DISTINCT CAST(index_node.id AS TEXT), enumeration.id, enumeration.predicate,
                member.id, member_evidence.span_id, enumeration.supporting_span_id,
                object.display_name, search.id, search_evidence.span_id
         FROM graph_enumerations enumeration
         JOIN graph_nodes index_node ON index_node.id=enumeration.subject_node_id AND index_node.kind='index'
         JOIN graph_claims search ON search.object_node_id=index_node.id
         JOIN graph_nodes feature ON feature.id=search.subject_node_id AND feature.canonical_key='feature:codesearch'
         JOIN graph_claim_evidence search_evidence ON search_evidence.claim_id=search.id
         JOIN graph_build_sources search_build ON search_build.build_id=?1 AND search_build.source_version_id=search.source_version_id
         JOIN graph_sources search_source ON search_source.id=search_build.source_id
         JOIN graph_build_sources enumeration_build ON enumeration_build.build_id=?1 AND enumeration_build.source_version_id=enumeration.source_version_id
         JOIN graph_sources enumeration_source ON enumeration_source.id=enumeration_build.source_id
         JOIN graph_build_enumeration_members member_link ON member_link.enumeration_id=enumeration.id AND member_link.build_id=?1
         JOIN graph_claims member ON member.id=member_link.claim_id AND member.subject_node_id=index_node.id AND member.predicate=enumeration.predicate AND member.scope=enumeration.scope
         JOIN graph_nodes object ON object.id=member.object_node_id AND ((enumeration.predicate='indexes_language' AND object.kind='language') OR (enumeration.predicate='indexes_dialect' AND object.kind='dialect'))
         JOIN graph_claim_evidence member_evidence ON member_evidence.claim_id=member.id
         JOIN graph_build_sources member_build ON member_build.build_id=?1 AND member_build.source_version_id=member.source_version_id
         JOIN graph_sources member_source ON member_source.id=member_build.source_id
         WHERE EXISTS (SELECT 1 FROM graph_build_enumerations be WHERE be.enumeration_id=enumeration.id AND be.build_id=?1 AND be.exhaustive=1)
           AND enumeration.predicate IN ('indexes_language','indexes_dialect')
           AND enumeration.scope IN ('project','feature')
           AND enumeration_source.active=1 AND enumeration_source.human_maintained=1
           AND EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=search.id AND bc.build_id=?1) AND search.active=1 AND search.status='current'
           AND search.polarity='positive' AND search.predicate='searches_index'
           AND search.scope IN ('project','feature') AND search_source.active=1 AND search_source.human_maintained=1
           AND EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=member.id AND bc.build_id=?1) AND member.active=1 AND member.status='current'
           AND member.polarity='positive' AND member_source.active=1 AND member_source.human_maintained=1
         ORDER BY index_node.id, enumeration.id, object.display_name COLLATE NOCASE, member.id"
    )?;
    let rows = stmt
        .query_map([build_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut by_index = std::collections::BTreeMap::<String, Vec<_>>::new();
    for row in rows {
        by_index.entry(row.0.clone()).or_default().push(row);
    }
    if by_index.len() != 1 {
        return Ok(None);
    }
    let Some((_, rows)) = by_index.into_iter().next() else {
        return Ok(None);
    };
    let mut names = Vec::new();
    let mut claims = Vec::new();
    let mut spans = Vec::new();
    let mut enumerations = std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for (_, enumeration_id, predicate, claim_id, member_span, list_span, name, search_claim, search_span) in rows {
        enumerations.entry(predicate).or_default().insert(enumeration_id);
        if !names.contains(&name) {
            names.push(name);
        }
        if !claims.contains(&claim_id) {
            claims.push(claim_id);
        }
        if !claims.contains(&search_claim) {
            claims.push(search_claim);
        }
        for span in [member_span, list_span, search_span] {
            if !spans.contains(&span) {
                spans.push(span);
            }
        }
    }
    if names.is_empty() || enumerations.len() != 2 || enumerations.values().any(|ids| ids.len() != 1) {
        Ok(None)
    } else {
        names.sort_by_key(|name| name.to_ascii_lowercase());
        Ok(Some((names, claims, spans)))
    }
}

/// Produce a resolver result from a prepared interpretation. Language
/// questions use the proof rules; other supported relations use direct claims.
pub fn resolve_prepared(
    db: &Connection,
    prepared: &SearchPreparation,
) -> Result<EvidenceResolution> {
    if prepared.interpretation.intent == model::QuestionIntent::ListCodeSearchLanguages {
        if !prepared.resolved_entity_keys.iter().any(|key| key == "feature:codesearch")
            || !["searches_index", "indexes_language", "indexes_dialect"]
                .iter()
                .all(|predicate| prepared.interpretation.predicates.iter().any(|p| p == predicate))
        {
            return Ok(EvidenceResolution::Unknown {
                reason: "the code-search feature or required list relationships are missing".into(),
                claim_ids: Vec::new(),
                evidence_ids: Vec::new(),
            });
        }
        return Ok(match complete_supported_code_set(db, &prepared.build_id)? {
            Some((_, claim_ids, evidence_ids)) => EvidenceResolution::Yes(PositiveFinding {
                outcome: "yes", claim_ids, evidence_ids,
            }),
            None => EvidenceResolution::Unknown {
                reason: "no validated complete language and dialect lists for the active code-search index were found".into(),
                claim_ids: Vec::new(), evidence_ids: Vec::new(),
            },
        });
    }
    if prepared.interpretation.intent == model::QuestionIntent::CodeSearchLanguage {
        let targets = prepared
            .resolved_entity_keys
            .iter()
            .filter(|key| key.starts_with("language:"))
            .collect::<Vec<_>>();
        let [target] = targets.as_slice() else {
            return Ok(EvidenceResolution::Unknown {
                reason: "the code-search question has no unambiguous language target".into(),
                claim_ids: Vec::new(),
                evidence_ids: Vec::new(),
            });
        };
        return resolve_language(db, prepared, target);
    }
    if let Some(positive) = derive_positive(db, prepared)? {
        return Ok(EvidenceResolution::Yes(positive));
    }
    Ok(EvidenceResolution::Unknown {
        reason: "no matching current claim was found for the requested relation".into(),
        claim_ids: Vec::new(),
        evidence_ids: Vec::new(),
    })
}

pub fn render_text(answer: &SearchAnswer) -> String {
    let mut output = format!(
        "{}\nBuild: {}\nClaims: {}",
        answer.answer,
        answer.build_id,
        if answer.claim_ids.is_empty() {
            "none".to_owned()
        } else {
            answer.claim_ids.join(", ")
        }
    );
    for citation in &answer.citations {
        output.push_str(&format!(
            "\n{} [{}]: {}",
            citation.citation,
            citation.claim_ids.join(", "),
            citation.text
        ));
    }
    output
}

/// Resolve a target language without treating missing or illustrative evidence
/// as a negative. Epic/task claims are intentionally excluded from project
/// capability conclusions; they remain useful history, but do not define the
/// project's current supported set.
pub fn resolve_language(
    db: &Connection,
    prepared: &SearchPreparation,
    target_key: &str,
) -> Result<EvidenceResolution> {
    let positive = derive_positive(db, prepared)?;
    let (negative_claims, negative_spans) = {
        let mut stmt = db.prepare(
            "SELECT DISTINCT c.id, e.span_id, search.id, search_evidence.span_id
             FROM graph_claims c
             JOIN graph_build_sources bs ON bs.build_id=?1 AND bs.source_version_id=c.source_version_id
             JOIN graph_sources s ON s.id=bs.source_id
             JOIN graph_nodes obj ON obj.id=c.object_node_id
             JOIN graph_claim_evidence e ON e.claim_id=c.id
             JOIN graph_claims search ON search.object_node_id=c.subject_node_id
             JOIN graph_nodes feature ON feature.id=search.subject_node_id AND feature.kind='feature'
             JOIN graph_claim_evidence search_evidence ON search_evidence.claim_id=search.id
             JOIN graph_build_sources search_bs ON search_bs.build_id=?1 AND search_bs.source_version_id=search.source_version_id
             JOIN graph_sources search_source ON search_source.id=search_bs.source_id
             WHERE EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=c.id AND bc.build_id=?1) AND c.active=1 AND s.active=1 AND s.human_maintained=1
               AND c.status='current' AND c.scope IN ('project','feature')
               AND c.predicate='indexes_language' AND obj.canonical_key=?2
               AND c.polarity='negative'
               AND EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=search.id AND bc.build_id=?1)
               AND search.active=1 AND search.predicate='searches_index' AND search.polarity='positive'
               AND search.status='current' AND search.scope IN ('project','feature')
               AND feature.canonical_key IN (SELECT value FROM json_each(?3))
               AND search_source.active=1 AND search_source.human_maintained=1
             ORDER BY c.id, e.span_id, search.id, search_evidence.span_id"
        )?;
        let resolved_keys = serde_json::to_string(&prepared.resolved_entity_keys)?;
        let rows = stmt
            .query_map(
                rusqlite::params![prepared.build_id, target_key, resolved_keys],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut claims = rows
            .iter()
            .flat_map(|r| [r.0.clone(), r.2.clone()])
            .collect::<Vec<_>>();
        claims.sort();
        claims.dedup();
        let mut spans = rows
            .into_iter()
            .flat_map(|r| [r.1, r.3])
            .collect::<Vec<_>>();
        spans.sort();
        spans.dedup();
        (claims, spans)
    };

    // Only a declared exhaustive list for an index currently searched by a
    // project feature can prove absence. Membership comes solely from that
    // enumeration's explicit members, never from other language mentions.
    let (omission_claims, omission_spans) = {
        let mut stmt = db.prepare(
            "SELECT DISTINCT member.id, en.supporting_span_id, search.id, search_evidence.span_id
             FROM graph_enumerations en
             JOIN graph_build_sources bs ON bs.build_id=?1 AND bs.source_version_id=en.source_version_id
             JOIN graph_sources s ON s.id=bs.source_id
             JOIN graph_nodes idx ON idx.id=en.subject_node_id AND idx.kind='index'
             JOIN graph_claims search ON search.object_node_id=idx.id
             JOIN graph_claim_evidence search_evidence ON search_evidence.claim_id=search.id
             JOIN graph_build_sources search_bs ON search_bs.build_id=?1 AND search_bs.source_version_id=search.source_version_id
             JOIN graph_sources search_source ON search_source.id=search_bs.source_id
             JOIN graph_nodes feature ON feature.id=search.subject_node_id AND feature.kind='feature'
             JOIN graph_build_enumeration_members em ON em.enumeration_id=en.id AND em.build_id=?1
             JOIN graph_claims member ON member.id=em.claim_id
             JOIN graph_nodes lang ON lang.id=member.object_node_id
             WHERE EXISTS (SELECT 1 FROM graph_build_enumerations be WHERE be.enumeration_id=en.id AND be.build_id=?1 AND be.exhaustive=1) AND en.predicate='indexes_language'
               AND en.scope IN ('project','feature') AND s.active=1 AND s.human_maintained=1
               AND EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=search.id AND bc.build_id=?1) AND search.active=1 AND search.predicate='searches_index'
               AND search.polarity='positive' AND search.status='current' AND search.scope IN ('project','feature')
               AND feature.canonical_key IN (SELECT value FROM json_each(?3))
               AND search_source.active=1 AND search_source.human_maintained=1
               AND EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=member.id AND bc.build_id=?1) AND member.active=1 AND member.status='current'
               AND member.polarity='positive' AND lang.canonical_key<>?2
               AND NOT EXISTS (
                 SELECT 1 FROM graph_build_enumeration_members target_member
                 JOIN graph_claims target_claim ON target_claim.id=target_member.claim_id
                 JOIN graph_nodes target_lang ON target_lang.id=target_claim.object_node_id
                 WHERE target_member.enumeration_id=en.id AND target_member.build_id=?1 AND target_claim.active=1
                   AND target_claim.status='current' AND target_claim.polarity='positive'
                   AND target_lang.canonical_key=?2
               )
               AND NOT EXISTS (
                 SELECT 1 FROM graph_claims newer
                 JOIN graph_nodes newer_obj ON newer_obj.id=newer.object_node_id
                 WHERE EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=newer.id AND bc.build_id=?1) AND newer.active=1 AND newer.predicate='indexes_language'
                   AND newer_obj.canonical_key=?2 AND newer.polarity='positive' AND newer.status='current'
                   AND newer.scope IN ('project','feature')
               )
             ORDER BY member.id, en.supporting_span_id, search.id, search_evidence.span_id"
        )?;
        let resolved_keys = serde_json::to_string(&prepared.resolved_entity_keys)?;
        let rows = stmt
            .query_map(
                rusqlite::params![prepared.build_id, target_key, resolved_keys],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut claims = rows
            .iter()
            .flat_map(|r| [r.0.clone(), r.2.clone()])
            .collect::<Vec<_>>();
        claims.sort();
        claims.dedup();
        let mut spans = rows
            .into_iter()
            .flat_map(|r| [r.1, r.3])
            .collect::<Vec<_>>();
        spans.sort();
        spans.dedup();
        (claims, spans)
    };

    let has_negative = !negative_claims.is_empty() || !omission_claims.is_empty();
    if let Some(yes) = positive {
        if has_negative {
            let mut claims = yes.claim_ids;
            claims.extend(negative_claims);
            claims.extend(omission_claims);
            claims.sort();
            claims.dedup();
            let mut spans = yes.evidence_ids;
            spans.extend(negative_spans);
            spans.extend(omission_spans);
            spans.sort();
            spans.dedup();
            return Ok(EvidenceResolution::Unknown {
                reason: "current positive and negative evidence conflict".into(),
                claim_ids: claims,
                evidence_ids: spans,
            });
        }
        return Ok(EvidenceResolution::Yes(yes));
    }
    if !negative_claims.is_empty() {
        if !omission_claims.is_empty() {
            let mut claims = negative_claims;
            claims.extend(omission_claims);
            let mut spans = negative_spans;
            spans.extend(omission_spans);
            claims.sort();
            claims.dedup();
            spans.sort();
            spans.dedup();
            return Ok(EvidenceResolution::No(NegativeFinding {
                outcome: "no",
                claim_ids: claims,
                evidence_ids: spans,
            }));
        }
        return Ok(EvidenceResolution::No(NegativeFinding {
            outcome: "no",
            claim_ids: negative_claims,
            evidence_ids: negative_spans,
        }));
    }
    if !omission_claims.is_empty() {
        return Ok(EvidenceResolution::No(NegativeFinding {
            outcome: "no",
            claim_ids: omission_claims,
            evidence_ids: omission_spans,
        }));
    }
    Ok(EvidenceResolution::Unknown { reason: "no current explicit exclusion or applicable exhaustive supported-language set proves the answer".into(), claim_ids: Vec::new(), evidence_ids: Vec::new() })
}

/// Apply the bounded positive rules supported by graph search. Model output is
/// never treated as a relation: direct matches must name a stored predicate,
/// and the only multi-edge rule is searches_index + indexes_language.
pub fn derive_positive(
    db: &Connection,
    prepared: &SearchPreparation,
) -> Result<Option<PositiveFinding>> {
    let mut claims = Vec::<String>::new();
    let mut evidence = Vec::<String>::new();
    let resolved = &prepared.resolved_entity_keys;
    let code_search_language =
        prepared.interpretation.intent == model::QuestionIntent::CodeSearchLanguage;

    // Each named endpoint must occupy its predicate's corresponding slot.
    // A parser shared by unrelated indexes cannot prove a relation about a
    // particular index.
    const DIRECT_PREDICATES: &[(&str, &str, &str)] = &[
        ("indexes_language", "index", "language"),
        ("indexes_dialect", "index", "dialect"),
        ("indexes_extension", "index", "extension"),
        ("searches_index", "feature", "index"),
        ("uses_parser", "index", "parser"),
    ];
    for predicate in &prepared.interpretation.predicates {
        if code_search_language {
            break;
        }
        let Some((_, subject_kind, object_kind)) =
            DIRECT_PREDICATES.iter().find(|(p, _, _)| *p == predicate)
        else {
            continue;
        };
        if resolved.iter().any(|key| {
            !key.starts_with(&format!("{subject_kind}:"))
                && !key.starts_with(&format!("{object_kind}:"))
        }) {
            continue;
        }
        let subject_keys = resolved
            .iter()
            .filter(|key| key.starts_with(&format!("{subject_kind}:")))
            .collect::<Vec<_>>();
        let object_keys = resolved
            .iter()
            .filter(|key| key.starts_with(&format!("{object_kind}:")))
            .collect::<Vec<_>>();
        if subject_keys.is_empty() && object_keys.is_empty() {
            continue;
        }
        // An epic can state a current relation about a specifically named
        // index, but its scope cannot establish a general project capability.
        // Require both endpoints for this narrower direct proof.
        let allow_epic_scope = prepared.interpretation.intent == model::QuestionIntent::DirectRelation
            && !subject_keys.is_empty()
            && !object_keys.is_empty();
        let mut stmt = db.prepare(
            "SELECT DISTINCT c.id, e.span_id
             FROM graph_claims c
             JOIN graph_build_sources bs ON bs.build_id=?1 AND bs.source_version_id=c.source_version_id
             JOIN graph_sources s ON s.id=bs.source_id
             JOIN graph_claim_evidence e ON e.claim_id=c.id
             JOIN graph_nodes subj ON subj.id=c.subject_node_id
             LEFT JOIN graph_nodes obj ON obj.id=c.object_node_id
             WHERE EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=c.id AND bc.build_id=?1) AND c.active=1 AND c.predicate=?2
               AND c.polarity='positive' AND c.status='current'
               AND (c.scope IN ('project','feature') OR (?5=1 AND c.scope='epic'))
               AND s.active=1 AND s.human_maintained=1
               AND (json_array_length(?3)=0 OR subj.canonical_key IN (SELECT value FROM json_each(?3)))
               AND (json_array_length(?4)=0 OR obj.canonical_key IN (SELECT value FROM json_each(?4)))
             ORDER BY c.id, e.span_id",
        )?;
        let subjects = serde_json::to_string(&subject_keys)?;
        let objects = serde_json::to_string(&object_keys)?;
        for row in stmt.query_map(
            rusqlite::params![prepared.build_id, predicate, subjects, objects, allow_epic_scope],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )? {
            let (claim, span) = row?;
            if !claims.contains(&claim) {
                claims.push(claim);
            }
            if !evidence.contains(&span) {
                evidence.push(span);
            }
        }
    }

    // Explicit two-premise rule: a searchable feature searches an index, and
    // that same index indexes the requested language. Join on the canonical
    // index node so unrelated claims cannot be combined.
    if code_search_language
        && prepared
            .interpretation
            .predicates
            .iter()
            .any(|p| p == "searches_index")
        && prepared
            .interpretation
            .predicates
            .iter()
            .any(|p| p == "indexes_language")
        && resolved.iter().any(|key| key.starts_with("language:"))
        && resolved.iter().any(|key| key.starts_with("feature:"))
    {
        let mut stmt = db.prepare(
            "SELECT DISTINCT search.id, language.id, se.span_id, le.span_id
             FROM graph_claims search
             JOIN graph_nodes feature ON feature.id=search.subject_node_id
             JOIN graph_nodes idx ON idx.id=search.object_node_id
             JOIN graph_claims language ON language.subject_node_id=idx.id
             JOIN graph_nodes lang ON lang.id=language.object_node_id
             JOIN graph_claim_evidence se ON se.claim_id=search.id
             JOIN graph_claim_evidence le ON le.claim_id=language.id
             JOIN graph_build_sources sbs ON sbs.build_id=?1 AND sbs.source_version_id=search.source_version_id
             JOIN graph_sources ss ON ss.id=sbs.source_id
             JOIN graph_build_sources lbs ON lbs.build_id=?1 AND lbs.source_version_id=language.source_version_id
             JOIN graph_sources ls ON ls.id=lbs.source_id
             WHERE EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=search.id AND bc.build_id=?1) AND EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=language.id AND bc.build_id=?1)
               AND search.active=1 AND language.active=1
               AND search.predicate='searches_index' AND language.predicate='indexes_language'
               AND search.polarity='positive' AND language.polarity='positive'
               AND search.status='current' AND language.status='current'
               AND search.scope IN ('project','feature')
               AND language.scope IN ('project','feature')
               AND feature.kind='feature' AND idx.kind='index' AND lang.kind='language'
               AND feature.canonical_key IN (SELECT value FROM json_each(?2))
               AND lang.canonical_key IN (SELECT value FROM json_each(?2))
               AND ss.active=1 AND ss.human_maintained=1 AND ls.active=1 AND ls.human_maintained=1
             ORDER BY search.id, language.id, se.span_id, le.span_id",
        )?;
        let keys = serde_json::to_string(resolved)?;
        for row in stmt.query_map(rusqlite::params![prepared.build_id, keys], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })? {
            let (first, second, first_span, second_span) = row?;
            for claim in [first, second] {
                if !claims.contains(&claim) {
                    claims.push(claim);
                }
            }
            for span in [first_span, second_span] {
                if !evidence.contains(&span) {
                    evidence.push(span);
                }
            }
        }
    }

    if claims.is_empty() {
        return Ok(None);
    }
    claims.sort();
    evidence.sort();
    Ok(Some(PositiveFinding {
        outcome: "yes",
        claim_ids: claims,
        evidence_ids: evidence,
    }))
}

/// Interpret a question and retrieve graph FTS candidates from the latest
/// completed build. Code/document index search is intentionally not consulted.
pub fn prepare_search(
    db: &Connection,
    question: &str,
    adapter: &dyn model::GraphModel,
    limit: usize,
) -> Result<SearchPreparation> {
    anyhow::ensure!(
        !question.trim().is_empty(),
        "graph search question is empty"
    );
    let build_id: String = db
        .query_row(
            "SELECT id FROM graph_builds WHERE state='completed' ORDER BY completed_at DESC, rowid DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow::anyhow!("no completed graph build exists"))?;

    // This query defines the only vocabulary exposed to the model: claims
    // active in the selected completed build and sourced from its authoritative
    // membership. Incomplete builds and deselected sources are never visible.
    let (candidate_entities, candidate_predicates) = {
        let mut stmt = db.prepare(
            "SELECT DISTINCT n.canonical_key, n.kind, n.display_name
             FROM graph_nodes n
             JOIN graph_claims c ON c.subject_node_id=n.id OR c.object_node_id=n.id
             JOIN graph_build_sources bs ON bs.build_id=?1 AND bs.source_version_id=c.source_version_id
             JOIN graph_sources s ON s.id=bs.source_id
             WHERE EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=c.id AND bc.build_id=?1) AND c.active=1 AND s.human_maintained=1 AND s.active=1
             ORDER BY n.canonical_key",
        )?;
        let entities = stmt
            .query_map([&build_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut entities_with_aliases = Vec::with_capacity(entities.len());
        for (key, kind, name) in entities {
            let mut alias_stmt = db.prepare(
                "SELECT DISTINCT a.alias FROM graph_aliases a
                 JOIN graph_claims_fts f ON f.alias_id=a.id
                 JOIN graph_claims c ON c.id=f.claim_id
                 JOIN graph_build_sources bs ON bs.build_id=?2 AND bs.source_version_id=c.source_version_id
                 JOIN graph_sources s ON s.id=bs.source_id
                 WHERE a.node_id=(SELECT id FROM graph_nodes WHERE canonical_key=?1)
                   AND EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=c.id AND bc.build_id=?2) AND c.active=1 AND s.human_maintained=1 AND s.active=1
                 ORDER BY a.alias",
            )?;
            let aliases = alias_stmt
                .query_map(rusqlite::params![&key, &build_id], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            entities_with_aliases.push((
                key,
                model::ClaimEntity {
                    kind,
                    name,
                    aliases,
                },
            ));
        }
        let mut pred_stmt = db.prepare(
            "SELECT DISTINCT c.predicate FROM graph_claims c
             JOIN graph_build_sources bs ON bs.build_id=?1 AND bs.source_version_id=c.source_version_id
             JOIN graph_sources s ON s.id=bs.source_id
             WHERE EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=c.id AND bc.build_id=?1) AND c.active=1 AND s.human_maintained=1 AND s.active=1
             ORDER BY c.predicate",
        )?;
        let predicates = pred_stmt
            .query_map([&build_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        (entities_with_aliases, predicates)
    };
    anyhow::ensure!(
        !candidate_predicates.is_empty(),
        "latest completed graph build has no active authoritative claims; rebuild with documents that state supported code indexing, search, parser, path exclusion, or work item status relations"
    );
    let request = model::InterpretationRequest {
        question: question.to_owned(),
        candidate_entities: candidate_entities
            .iter()
            .map(|(_, entity)| entity.clone())
            .collect(),
        candidate_predicates: candidate_predicates.clone(),
    };
    let interpretation = interpret_with_one_correction(adapter, &request)?;

    let mut resolved_entity_keys = Vec::new();
    for proposed in &interpretation.entities {
        anyhow::ensure!(
            !proposed.kind.trim().is_empty() && !proposed.name.trim().is_empty(),
            "graph model returned an entity with an empty kind or name"
        );
        let canonical = proposed.canonicalized();
        anyhow::ensure!(
            [
                "feature",
                "index",
                "language",
                "dialect",
                "extension",
                "parser",
                "work_item"
            ]
            .contains(&canonical.kind.as_str()),
            "graph model returned unsupported entity kind {:?}",
            proposed.kind
        );
        // A query may name an entity that has no node because the source set
        // never mentioned it (for example, Lua). Keep its typed canonical key
        // as the search target so an exhaustive supported-language list can
        // prove absence. It is never inserted into the graph.
        let found = resolve_interpreted_entity_key(proposed, &candidate_entities)?;
        if !resolved_entity_keys.contains(&found) {
            resolved_entity_keys.push(found);
        }
    }

    let fts_query = make_graph_fts_query(question, &interpretation);
    let mut candidates = Vec::new();
    if !fts_query.is_empty() {
        let mut stmt = db.prepare(
            "SELECT DISTINCT c.id, c.claim_text, s.path, c.source_version_id
             FROM graph_claims_fts f
             JOIN graph_claims c ON c.id=f.claim_id
             JOIN graph_build_sources bs ON bs.build_id=?2 AND bs.source_version_id=c.source_version_id
             JOIN graph_sources s ON s.id=bs.source_id
             WHERE graph_claims_fts MATCH ?1 AND EXISTS (SELECT 1 FROM graph_build_claims bc WHERE bc.claim_id=c.id AND bc.build_id=?2) AND c.active=1
               AND s.human_maintained=1 AND s.active=1
             ORDER BY c.id LIMIT ?3",
        )?;
        candidates = stmt
            .query_map(
                rusqlite::params![fts_query, build_id, limit as i64],
                |row| {
                    Ok(SearchCandidate {
                        claim_id: row.get(0)?,
                        claim_text: row.get(1)?,
                        source_path: row.get(2)?,
                        source_version_id: row.get(3)?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
    }
    if std::env::var("DAFTPROMPT_GRAPH_DEBUG_INTERPRETATION").as_deref() == Ok("1") {
        eprintln!(
            "graph search interpretation: intent={:?} predicates={:?} resolved_entity_keys={:?} active_entities={} candidates={}",
            interpretation.intent,
            interpretation.predicates,
            resolved_entity_keys,
            candidate_entities.len(),
            candidates.len(),
        );
    }
    Ok(SearchPreparation {
        build_id,
        interpretation,
        resolved_entity_keys,
        candidates,
    })
}

fn resolve_interpreted_entity_key(
    proposed: &model::ClaimEntity,
    candidates: &[(String, model::ClaimEntity)],
) -> Result<String> {
    let canonical = proposed.canonicalized();
    if let Some((key, _)) = candidates
        .iter()
        .find(|(key, _)| *key == canonical.canonical_key)
    {
        return Ok(key.clone());
    }
    let matches = candidates
        .iter()
        .filter(|(_, candidate)| {
            let known = candidate.canonicalized();
            known.kind == canonical.kind
                && known.aliases.iter().any(|known_alias| {
                    known_alias.kind == canonical.kind
                        && canonical.aliases.iter().any(|requested_alias| {
                            requested_alias.kind == canonical.kind
                                && requested_alias.normalized == known_alias.normalized
                        })
                })
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        matches.len() <= 1,
        "graph model entity {:?} has multiple active {} matches",
        proposed.name,
        canonical.kind
    );
    Ok(matches
        .into_iter()
        .next()
        .unwrap_or(canonical.canonical_key))
}

fn interpret_with_one_correction(
    adapter: &dyn model::GraphModel,
    request: &model::InterpretationRequest,
) -> Result<model::InterpretationResponse> {
    let interpretation = adapter
        .interpret_question(request)
        .map_err(|error| anyhow::anyhow!("interpreting graph search question: {error:#}"))?;
    let initial_error = match validate_interpretation(&interpretation, request) {
        Ok(()) => return Ok(interpretation),
        Err(error) => error,
    };
    let Some(retry) = adapter.retry_interpretation(request, &initial_error.to_string()) else {
        return Err(initial_error);
    };
    let corrected = retry.map_err(|error| {
        anyhow::anyhow!("{initial_error}; model correction also failed: {error:#}")
    })?;
    validate_interpretation(&corrected, request).map_err(|error| {
        anyhow::anyhow!("{initial_error}; model correction also failed: {error:#}")
    })?;
    Ok(corrected)
}

fn validate_interpretation(
    interpretation: &model::InterpretationResponse,
    request: &model::InterpretationRequest,
) -> Result<()> {
    anyhow::ensure!(
        interpretation
            .predicates
            .iter()
            .all(|predicate| request.candidate_predicates.contains(predicate)),
        "graph model returned a predicate outside the active graph vocabulary"
    );
    for entity in &interpretation.entities {
        anyhow::ensure!(
            !entity.kind.trim().is_empty() && !entity.name.trim().is_empty(),
            "graph model returned an entity with an empty kind or name"
        );
        anyhow::ensure!(
            ["feature", "index", "language", "dialect", "extension", "parser", "work_item"]
                .contains(&entity.canonicalized().kind.as_str()),
            "graph model returned unsupported entity kind {:?}",
            entity.kind
        );
    }
    if interpretation.intent == model::QuestionIntent::DirectRelation {
        let allowed_kinds = interpretation
            .predicates
            .iter()
            .flat_map(|predicate| match predicate.as_str() {
                "indexes_language" => &["index", "language"][..],
                "indexes_dialect" => &["index", "dialect"][..],
                "indexes_extension" => &["index", "extension"][..],
                "searches_index" => &["feature", "index"][..],
                "uses_parser" => &["feature", "index", "parser"][..],
                "excludes_path" => &["feature", "index", "work_item"][..],
                "work_item_status" => &["work_item"][..],
                _ => &[][..],
            })
            .collect::<Vec<_>>();
        anyhow::ensure!(
            interpretation.entities.iter().all(|entity| {
                allowed_kinds
                    .iter()
                    .any(|kind| **kind == entity.canonicalized().kind)
            }),
            "direct relation entities have kinds incompatible with the selected predicate; use the predicate's subject and object kinds"
        );
    }
    if interpretation.intent == model::QuestionIntent::CodeSearchLanguage {
        let targets = interpretation
            .entities
            .iter()
            .filter(|entity| entity.canonicalized().kind == "language")
            .collect::<Vec<_>>();
        anyhow::ensure!(
            targets.len() == 1,
            "code-search language interpretation needs exactly one language target named in the question"
        );
        let target = targets[0].canonicalized();
        let mentioned = request
            .candidate_entities
            .iter()
            .filter(|candidate| candidate.canonicalized().kind == "language")
            .filter(|candidate| {
                candidate
                    .canonicalized()
                    .aliases
                    .iter()
                    .any(|alias| {
                        alias.kind == "language"
                            && question_contains_surface(&request.question, &alias.surface)
                    })
            })
            .collect::<Vec<_>>();
        let mentioned_keys = mentioned
            .iter()
            .map(|candidate| candidate.canonicalized().canonical_key)
            .collect::<std::collections::BTreeSet<_>>();
        anyhow::ensure!(
            mentioned_keys.len() <= 1,
            "code-search question names multiple known languages; a single-language answer would be ambiguous"
        );
        if let Some(candidate) = mentioned.first() {
            let known = candidate.canonicalized();
            let target_name = targets[0].name.trim().to_lowercase();
            anyhow::ensure!(
                known.canonical_key == target.canonical_key
                    || known.aliases.iter().any(|alias| {
                        alias.kind == "language" && alias.normalized == target_name
                    }),
                "code-search language target does not match the language named in the question"
            );
        } else {
            // An absent language can still be disproved by an exhaustive set,
            // but the model must copy its name from the question itself.
            anyhow::ensure!(
                !is_generic_search_word(&targets[0].name)
                    && question_contains_surface(&request.question, &targets[0].name),
                "code-search language target is not named in the question"
            );
        }
        let question = request.question.to_ascii_lowercase();
        let names_an_index = request.candidate_entities.iter().any(|entity| {
            entity.canonicalized().kind == "index"
                && question.contains(&entity.name.to_ascii_lowercase())
        });
        anyhow::ensure!(
            !names_an_index
                || question.contains("search")
                || question.contains("find"),
            "question names an index relationship, not code search; use direct_relation"
        );
    }
    Ok(())
}

fn question_contains_surface(question: &str, surface: &str) -> bool {
    let question = question.to_lowercase();
    let surface = surface.trim().to_lowercase();
    if surface.is_empty() {
        return false;
    }
    question.match_indices(&surface).any(|(start, _)| {
        let end = start + surface.len();
        let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
        !question[..start].chars().next_back().is_some_and(is_word)
            && !question[end..].chars().next().is_some_and(is_word)
    })
}

fn is_generic_search_word(surface: &str) -> bool {
    matches!(
        surface.trim().to_ascii_lowercase().as_str(),
        "code" | "search" | "find" | "language" | "languages" | "index" | "indexed"
            | "supported" | "support" | "file" | "files"
    )
}

fn make_graph_fts_query(question: &str, interpretation: &model::InterpretationResponse) -> String {
    let mut terms = Vec::new();
    let mut add_terms = |text: &str| {
        for term in text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.') {
            let term = term.trim();
            if term.chars().count() >= 2
                && !terms
                    .iter()
                    .any(|seen: &String| seen.eq_ignore_ascii_case(term))
            {
                terms.push(term.to_owned());
            }
        }
    };
    add_terms(question);
    add_terms(match interpretation.intent {
        model::QuestionIntent::DirectRelation => "direct relation",
        model::QuestionIntent::CodeSearchLanguage => "code search language",
        model::QuestionIntent::ListCodeSearchLanguages => "code search languages dialects",
    });
    for entity in &interpretation.entities {
        add_terms(&entity.name);
        for alias in &entity.aliases {
            add_terms(alias);
        }
    }
    for predicate in &interpretation.predicates {
        add_terms(predicate);
    }
    terms
        .into_iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Extract every selected source first, then publish its complete claim set in
/// one transaction. Until that transaction commits, the previous completed
/// build remains the only published graph state.
pub fn build(
    db: &Connection,
    repo_path: &Path,
    sources: &[SelectedSource],
    adapter: &dyn model::GraphModel,
    extractor_version: &str,
) -> Result<String> {
    anyhow::ensure!(
        !sources.is_empty(),
        "a graph build requires at least one source"
    );
    let build_id = new_build_id();
    db.execute(
        "INSERT INTO graph_builds(id, started_at, state, extractor_model, extractor_version) VALUES (?1, ?2, 'building', ?3, ?4)",
        rusqlite::params![build_id, chrono::Utc::now().to_rfc3339(), adapter.model_name(), extractor_version],
    )?;

    let result = (|| -> Result<()> {
        let snapshots = snapshot_sources_with_limit(
            db,
            &build_id,
            repo_path,
            sources,
            extractor_version,
            adapter.extraction_batch_max_chars(),
        )?;
        let mut extractions = Vec::with_capacity(snapshots.len());
        let mut claim_count = 0usize;
        for (index, (source, snapshot)) in sources.iter().zip(&snapshots).enumerate() {
            eprintln!(
                "graph build: extracting source {}/{}: {}",
                index + 1,
                sources.len(),
                source.path
            );
            let checked = extract_source_in_batches(adapter, source, &snapshot.passages)?;
            claim_count += checked.claims.len();
            eprintln!(
                "graph build: completed source {}/{}: {} claims ({} total)",
                index + 1,
                sources.len(),
                checked.claims.len(),
                claim_count
            );
            extractions.push((snapshot.source_version_id, checked));
        }

        anyhow::ensure!(
            extractions.iter().any(|(_, extraction)| !extraction.claims.is_empty()),
            "graph build extracted no supported claims from the selected sources; the current graph supports code indexing, search, parser, path exclusion, and work item status relations"
        );

        let tx = db.unchecked_transaction()?;
        for (source_version_id, extraction) in &extractions {
            persist_extraction_tx(&tx, &build_id, *source_version_id, extraction)?;
        }
        derive_supersession_tx(&tx, &build_id)?;
        // Source activity and claim visibility move together with the build
        // pointer so an extraction or persistence error cannot expose a mix.
        tx.execute("UPDATE graph_claims SET active = 0", [])?;
        tx.execute(
            "UPDATE graph_claims SET active = 1 WHERE id IN
             (SELECT claim_id FROM graph_build_claims WHERE build_id = ?1)",
            [&build_id],
        )?;
        tx.execute("UPDATE graph_sources SET active = 0", [])?;
        tx.execute(
            "UPDATE graph_sources
             SET active = 1,
                 latest_content_hash = (
                    SELECT v.content_hash FROM graph_build_sources bs
                    JOIN graph_source_versions v ON v.id = bs.source_version_id
                    WHERE bs.build_id = ?1 AND bs.source_id = graph_sources.id
                 )
             WHERE id IN (SELECT source_id FROM graph_build_sources WHERE build_id = ?1)",
            [&build_id],
        )?;
        tx.execute(
            "UPDATE graph_builds SET state = 'completed', completed_at = ?1 WHERE id = ?2 AND state = 'building'",
            rusqlite::params![chrono::Utc::now().to_rfc3339(), build_id],
        )?;
        tx.commit()?;
        Ok(())
    })();

    if let Err(error) = result {
        let _ = db.execute(
            "UPDATE graph_builds SET state = 'failed' WHERE id = ?1 AND state = 'building'",
            [&build_id],
        );
        return Err(error);
    }
    Ok(build_id)
}

fn passage_batches(
    passages: &[SourcePassage],
    max_chars: Option<usize>,
    max_passages: Option<usize>,
) -> Vec<&[SourcePassage]> {
    if passages.is_empty() {
        return vec![passages];
    }
    let limit = max_chars.unwrap_or(usize::MAX).max(1);
    let passage_limit = max_passages.unwrap_or(usize::MAX).max(1);
    let mut batches = Vec::new();
    let mut start = 0;
    let mut chars = 0usize;
    for (index, passage) in passages.iter().enumerate() {
        let size = passage.text.len() + passage.heading_path.len();
        if index > start
            && (chars.saturating_add(size) > limit || index - start >= passage_limit)
        {
            batches.push(&passages[start..index]);
            start = index;
            chars = 0;
        }
        chars = chars.saturating_add(size);
    }
    batches.push(&passages[start..]);
    batches
}

fn extract_source_in_batches(
    adapter: &dyn model::GraphModel,
    source: &SelectedSource,
    passages: &[SourcePassage],
) -> Result<model::ExtractionResponse> {
    let concurrency = std::env::var("DAFTPROMPT_GRAPH_BATCH_CONCURRENCY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 4);
    extract_source_in_batches_with_concurrency(adapter, source, passages, concurrency)
}

fn extract_source_in_batches_with_concurrency(
    adapter: &dyn model::GraphModel,
    source: &SelectedSource,
    passages: &[SourcePassage],
    concurrency: usize,
) -> Result<model::ExtractionResponse> {
    let batches = passage_batches(
        passages,
        adapter.extraction_batch_max_chars(),
        adapter.extraction_batch_max_passages(),
    );
    let batch_count = batches.len();
    let concurrency = concurrency.clamp(1, 4);
    let mut combined = model::ExtractionResponse::default();
    for (window_index, window) in batches.chunks(concurrency).enumerate() {
        let first_index = window_index * concurrency;
        let checked_batches = if window.len() == 1 {
            vec![extract_batch(adapter, source, window[0], first_index, batch_count)?]
        } else {
            std::thread::scope(|scope| {
                let handles: Vec<_> = window
                    .iter()
                    .enumerate()
                    .map(|(offset, batch)| {
                        scope.spawn(move || {
                            extract_batch(adapter, source, batch, first_index + offset, batch_count)
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .map_err(|_| anyhow::anyhow!("graph extraction worker panicked"))?
                    })
                    .collect::<Result<Vec<_>>>()
            })?
        };
        for checked in checked_batches {
            let offset = combined.claims.len();
            combined.claims.extend(checked.claims);
            combined
                .enumerations
                .extend(checked.enumerations.into_iter().map(|mut enumeration| {
                    for member in &mut enumeration.member_claim_indexes {
                        *member += offset;
                    }
                    enumeration
                }));
        }
    }
    Ok(combined)
}

fn extract_batch(
    adapter: &dyn model::GraphModel,
    source: &SelectedSource,
    batch: &[SourcePassage],
    index: usize,
    batch_count: usize,
) -> Result<model::ExtractionResponse> {
    eprintln!(
        "graph build: {} batch {}/{} ({} passages, {} bytes)",
        source.path,
        index + 1,
        batch_count,
        batch.len(),
        batch
            .iter()
            .map(|passage| passage.text.len())
            .sum::<usize>()
    );
    let request = model::ExtractionRequest {
        source_path: source.path.clone(),
        passages: batch.to_vec(),
    };
    let response = adapter.extract_claims(&request).map_err(|error| {
        anyhow::anyhow!(
            "extracting claims for source {} batch {}/{}: {error:#}",
            source.path,
            index + 1,
            batch_count
        )
    })?;
    let previous_response = response.clone();
    let checked = match validate_extraction(&source.path, &source.text, batch, response) {
        Ok(checked) => checked,
        Err(initial_error) => {
            let Some(retry) = adapter.retry_extraction(
                &request,
                &previous_response,
                &initial_error.to_string(),
            ) else {
                anyhow::bail!(
                    "source {} batch {}/{}: {initial_error}",
                    source.path,
                    index + 1,
                    batch_count
                );
            };
            let corrected = retry.map_err(|error| {
                anyhow::anyhow!(
                    "source {} batch {}/{}: model correction failed after {initial_error}: {error:#}",
                    source.path,
                    index + 1,
                    batch_count
                )
            })?;
            validate_extraction(&source.path, &source.text, batch, corrected).map_err(|error| {
                anyhow::anyhow!(
                    "source {} batch {}/{}: {initial_error}; model correction also failed after: {error}",
                    source.path,
                    index + 1,
                    batch_count
                )
            })?
        }
    };
    Ok(checked)
}

/// Link a new claim to an older record only when it is a status update to the
/// same assertion within the same selected document section. The source path
/// and heading breadcrumb provide the identity of the project/epic/task/feature
/// scope; matching only the enum (for example `epic`) would conflate Epic 008
/// with Epic 009. Both records and their evidence remain stored unchanged.
fn derive_supersession_tx(tx: &rusqlite::Transaction<'_>, build_id: &str) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO graph_claim_supersession(new_claim_id, old_claim_id, reason)
         SELECT newer.id, older.id,
                'newer status claim in the same source, heading, and declared scope'
         FROM graph_claims newer
         JOIN graph_source_versions new_version ON new_version.id = newer.source_version_id
         JOIN graph_sources new_source ON new_source.id = new_version.source_id
         JOIN graph_claim_evidence new_evidence ON new_evidence.claim_id = newer.id
         JOIN graph_spans new_span ON new_span.id = new_evidence.span_id
         JOIN graph_claims older ON older.id <> newer.id
           AND older.subject_node_id = newer.subject_node_id
           AND older.predicate = newer.predicate
           AND older.scope = newer.scope
           AND older.source_version_id <> newer.source_version_id
           AND older.build_id <> newer.build_id
         JOIN graph_source_versions old_version ON old_version.id = older.source_version_id
         JOIN graph_sources old_source ON old_source.id = old_version.source_id
         JOIN graph_claim_evidence old_evidence ON old_evidence.claim_id = older.id
         JOIN graph_spans old_span ON old_span.id = old_evidence.span_id
         WHERE EXISTS (SELECT 1 FROM graph_build_claims membership
                       WHERE membership.claim_id=newer.id AND membership.build_id=?1)
           AND new_source.path = old_source.path
           AND new_span.heading_path = old_span.heading_path
           AND (
             (newer.object_node_id IS older.object_node_id
               AND newer.object_literal IS older.object_literal
               AND newer.polarity = older.polarity
               AND newer.status IN ('completed', 'rejected')
               AND older.status IN ('current', 'planned', 'deferred'))
             OR
             (newer.predicate = 'work_item_status'
               AND newer.object_literal IN ('completed', 'rejected')
               AND older.object_literal IN ('current', 'planned', 'deferred')
               AND newer.status = older.status)
           )",
        [build_id],
    )?;
    Ok(())
}

fn new_build_id() -> String {
    let timestamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let sequence = NEXT_BUILD_ID.fetch_add(1, Ordering::Relaxed);
    format!(
        "b{:016x}{:08x}{:08x}",
        timestamp as u64,
        std::process::id(),
        sequence as u32
    )
}

/// Validate a structured extraction response against the immutable source
/// snapshot and passages supplied to the model. Validation is intentionally
/// performed before any graph writes so a single malformed proposal cannot
/// become active evidence.
pub fn validate_extraction(
    source_path: &str,
    snapshot: &str,
    passages: &[SourcePassage],
    mut response: model::ExtractionResponse,
) -> Result<model::ExtractionResponse> {
    use model::{ClaimObject, ClaimScope, ClaimStatus};

    let mut passage_by_id = BTreeMap::new();
    for passage in passages {
        anyhow::ensure!(
            passage.start_line > 0 && passage.end_line >= passage.start_line,
            "source {source_path}: passage {} has invalid line bounds",
            passage.id
        );
        anyhow::ensure!(
            passage_by_id.insert(passage.id.as_str(), passage).is_none(),
            "source {source_path}: duplicate passage ID {}",
            passage.id
        );
        let expected =
            snapshot_lines(snapshot, passage.start_line, passage.end_line).ok_or_else(|| {
                anyhow::anyhow!(
                    "source {source_path}: passage {} is outside the snapshot",
                    passage.id
                )
            })?;
        anyhow::ensure!(
            expected == passage.text,
            "source {source_path}: passage {} does not match its snapshot lines",
            passage.id
        );
    }

    for (claim_index, claim) in response.claims.iter_mut().enumerate() {
        let claim_label = format!("source {source_path}: claim {}", claim_index + 1);
        canonicalize_claim_entity(&mut claim.subject);
        if let ClaimObject::Entity(entity) = &mut claim.object {
            canonicalize_claim_entity(entity);
        }
        let object_kind_for_predicate = match &claim.object {
            ClaimObject::Entity(entity) => entity.kind.as_str(),
            ClaimObject::Literal { value_kind, .. } => value_kind.as_str(),
        };
        if let Some(predicate) = model::normalize_predicate(
            &claim.predicate,
            &claim.subject.kind,
            object_kind_for_predicate,
        ) {
            claim.predicate = predicate.to_owned();
        }
        if let ClaimObject::Literal { value, value_kind } = &claim.object {
            let expected_kind = match claim.predicate.as_str() {
                "indexes_language" => Some("language"),
                "indexes_dialect" => Some("dialect"),
                "indexes_extension" => Some("extension"),
                _ => None,
            };
            if expected_kind == Some(value_kind.as_str()) {
                let surface = value.trim();
                if value_kind == "extension" {
                    let suffix = surface.strip_prefix('.').unwrap_or(surface);
                    anyhow::ensure!(
                        !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_alphanumeric()),
                        "{claim_label}: invalid extension literal {value:?}"
                    );
                }
                claim.object = ClaimObject::Entity(model::ClaimEntity {
                    kind: value_kind.clone(),
                    name: surface.to_owned(),
                    aliases: vec![surface.to_owned()],
                });
                if let ClaimObject::Entity(entity) = &mut claim.object {
                    canonicalize_claim_entity(entity);
                }
            }
        }
        // JSX and TSX are dialect nodes even when the model calls them
        // languages. Keep the claim, but use the relation matching its
        // canonical object type so language and dialect sets stay distinct.
        if claim.subject.kind == "index"
            && claim.predicate == "indexes_language"
            && matches!(&claim.object, ClaimObject::Entity(entity) if entity.kind == "dialect")
        {
            claim.predicate = "indexes_dialect".to_owned();
        }
        validate_entity(&claim_label, "subject", &claim.subject)?;
        let expected_types: &[(&str, &str)] = match claim.predicate.as_str() {
            "indexes_language" => &[("index", "language")],
            "indexes_dialect" => &[("index", "dialect")],
            "indexes_extension" => &[("index", "extension")],
            "searches_index" => &[("feature", "index")],
            "uses_parser" => &[("feature", "parser"), ("index", "parser")],
            "excludes_path" => &[
                ("feature", "path"),
                ("index", "path"),
                ("work_item", "path"),
            ],
            "work_item_status" => &[("work_item", "status")],
            other => anyhow::bail!("{claim_label}: unknown predicate {other:?}"),
        };
        let object_kind = match &mut claim.object {
            ClaimObject::Entity(entity) => {
                validate_entity(&claim_label, "object", entity)?;
                entity.kind.as_str()
            }
            ClaimObject::Literal { value, value_kind } => {
                anyhow::ensure!(
                    !value.trim().is_empty(),
                    "{claim_label}: literal object is empty"
                );
                if claim.predicate == "work_item_status"
                    && value_kind == "string"
                    && matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "current"
                            | "planned"
                            | "deferred"
                            | "completed"
                            | "rejected"
                            | "historical"
                    )
                {
                    *value = value.trim().to_ascii_lowercase();
                    *value_kind = "status".to_owned();
                }
                match value_kind.as_str() {
                    "path" if claim.predicate == "excludes_path" => "path",
                    "status" if claim.predicate == "work_item_status" => {
                        anyhow::ensure!(
                            matches!(
                                value.as_str(),
                                "current"
                                    | "planned"
                                    | "deferred"
                                    | "completed"
                                    | "rejected"
                                    | "historical"
                            ),
                            "{claim_label}: invalid work item status literal {value:?}"
                        );
                        "status"
                    }
                    _ => anyhow::bail!(
                        "{claim_label}: invalid literal type {value_kind:?} for {}",
                        claim.predicate
                    ),
                }
            }
        };
        anyhow::ensure!(
            expected_types
                .iter()
                .any(|(subject, object)| *subject == claim.subject.kind && *object == object_kind),
            "{claim_label}: predicate {} does not allow {} -> {}",
            claim.predicate,
            claim.subject.kind,
            object_kind
        );
        // Enums constrain polarity, status and scope during JSON deserialization;
        // this match also makes the supported vocabulary explicit here.
        let _ = match claim.status {
            ClaimStatus::Current
            | ClaimStatus::Planned
            | ClaimStatus::Deferred
            | ClaimStatus::Completed
            | ClaimStatus::Rejected
            | ClaimStatus::Historical => true,
        };
        let _ = match claim.scope {
            ClaimScope::Project | ClaimScope::Epic | ClaimScope::Task | ClaimScope::Feature => true,
        };
        anyhow::ensure!(
            !claim.evidence.is_empty(),
            "{claim_label}: missing evidence"
        );
        for evidence in &mut claim.evidence {
            let passage = passage_by_id
                .get(evidence.passage_id.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "{claim_label}: unknown or unselected passage {}",
                        evidence.passage_id
                    )
                })?;
            anyhow::ensure!(
                evidence.start_line >= passage.start_line
                    && evidence.end_line <= passage.end_line
                    && evidence.end_line >= evidence.start_line,
                "{claim_label}: evidence range {}-{} is outside passage {} ({}-{})",
                evidence.start_line,
                evidence.end_line,
                passage.id,
                passage.start_line,
                passage.end_line
            );
            let mut expected = snapshot_lines(snapshot, evidence.start_line, evidence.end_line)
                .ok_or_else(|| {
                    anyhow::anyhow!("{claim_label}: evidence range is outside snapshot")
                })?;
            let mut evidence_matches = evidence_text_matches_snapshot(&evidence.text, &expected);
            if !evidence_matches
                && extend_prefix_evidence_to_full_lines(&evidence.text, &expected).is_some()
            {
                evidence_matches = true;
            }
            if !evidence_matches {
                if let Some((start, end, exact)) =
                    unique_evidence_span_in_passage(&evidence.text, passage)
                {
                    evidence.start_line = start;
                    evidence.end_line = end;
                    expected = exact;
                    evidence_matches = true;
                }
            }
            if !evidence_matches {
                log_evidence_mismatch(
                    source_path,
                    evidence.start_line,
                    evidence.end_line,
                    &evidence.text,
                    &expected,
                );
            }
            // Echo the exact expected snapshot text alongside the model's
            // submitted text so a one-shot correction (extraction_correction_contract
            // in model.rs) can see the literal diff and copy the correct
            // bytes instead of re-deriving the citation from memory. This is
            // already-selected, already-cited source text, not new data.
            anyhow::ensure!(
                evidence_matches,
                "{claim_label}: evidence text does not match snapshot lines {}-{}: expected {}; got {}",
                evidence.start_line,
                evidence.end_line,
                bounded_escaped(&expected),
                bounded_escaped(&evidence.text)
            );
            // Restore exact snapshot bytes after checking the model's wording.
            // Models often omit a line ending or change whitespace; they must
            // not introduce or replace any source words.
            evidence.text = expected;
        }
    }
    response.enumerations.retain(|enumeration| {
        let supported = matches!(
            enumeration.predicate.as_str(),
            "indexes_language" | "indexes_dialect" | "indexes_extension"
        );
        if !supported {
            eprintln!(
                "graph build: {source_path}: omitting unsupported enumeration predicate {:?}",
                enumeration.predicate
            );
        }
        supported
    });
    let mut valid_enumerations = Vec::with_capacity(response.enumerations.len());
    for (index, mut enumeration) in response.enumerations.drain(..).enumerate() {
        // A stated list is optional extraction metadata. Discard a malformed
        // list instead of sending otherwise valid claims through correction;
        // only a fully validated list may ever support an exhaustive answer.
        let validation = (|| -> Result<()> {
        let label = format!("source {source_path}: enumeration {}", index + 1);
        canonicalize_claim_entity(&mut enumeration.subject);
        validate_entity(&label, "subject", &enumeration.subject)?;
        anyhow::ensure!(enumeration.subject.kind == "index", "{label}: subject must be an index");
        anyhow::ensure!(!enumeration.member_claim_indexes.is_empty(), "{label}: missing members");
        if enumeration.predicate == "indexes_language"
            && !enumeration.member_claim_indexes.is_empty()
            && enumeration.member_claim_indexes.iter().all(|member| {
                response.claims.get(*member).is_some_and(|claim| {
                    claim.predicate == "indexes_dialect"
                        && matches!(&claim.object, ClaimObject::Entity(entity) if entity.kind == "dialect")
                })
            })
        {
            enumeration.predicate = "indexes_dialect".to_owned();
        }
        let span = passages
            .iter()
            .find(|p| p.id == enumeration.evidence.passage_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{label}: unknown or unselected supporting passage {}",
                    enumeration.evidence.passage_id
                )
            })?;
        anyhow::ensure!(
            enumeration.evidence.start_line >= span.start_line
                && enumeration.evidence.end_line <= span.end_line
                && enumeration.evidence.end_line >= enumeration.evidence.start_line,
            "{label}: evidence range is outside passage {}",
            span.id
        );
        let mut expected = snapshot_lines(
            snapshot,
            enumeration.evidence.start_line,
            enumeration.evidence.end_line,
        )
        .ok_or_else(|| anyhow::anyhow!("{label}: evidence range is outside snapshot"))?;
        let mut evidence_matches = evidence_text_matches_snapshot(&enumeration.evidence.text, &expected);
        if !evidence_matches
            && extend_prefix_evidence_to_full_lines(&enumeration.evidence.text, &expected).is_some()
        {
            evidence_matches = true;
        }
        if !evidence_matches {
            if let Some((start, end, exact)) =
                unique_evidence_span_in_passage(&enumeration.evidence.text, span)
            {
                enumeration.evidence.start_line = start;
                enumeration.evidence.end_line = end;
                expected = exact;
                evidence_matches = true;
            }
        }
        if !evidence_matches {
            log_evidence_mismatch(
                source_path,
                enumeration.evidence.start_line,
                enumeration.evidence.end_line,
                &enumeration.evidence.text,
                &expected,
            );
        }
        anyhow::ensure!(
            evidence_matches,
            "{label}: evidence text does not match snapshot lines {}-{}: expected {}; got {}",
            enumeration.evidence.start_line,
            enumeration.evidence.end_line,
            bounded_escaped(&expected),
            bounded_escaped(&enumeration.evidence.text)
        );
        enumeration.evidence.text = expected;
        // A model's exhaustive flag is only accepted if the actual citation
        // explicitly describes completeness; a list of examples stays open.
        enumeration.exhaustive &= explicitly_complete_wording(&enumeration.evidence.text);
        let expected_object_kind = match enumeration.predicate.as_str() {
            "indexes_language" => "language",
            "indexes_dialect" => "dialect",
            _ => "extension",
        };
        anyhow::ensure!(enumeration.member_claim_indexes.iter().all(|member| {
            response.claims.get(*member).is_some_and(|claim| {
                claim.subject.canonicalized().canonical_key == enumeration.subject.canonicalized().canonical_key
                    && claim.predicate == enumeration.predicate
                    && claim.polarity == model::Polarity::Positive
                    && claim.status == model::ClaimStatus::Current
                    && claim.scope == enumeration.scope
                    && matches!(&claim.object, model::ClaimObject::Entity(entity) if entity.kind == expected_object_kind || entity.canonicalized().kind == expected_object_kind)
            })
        }), "{label}: members must reference matching claims from this extraction");
        if enumeration.exhaustive {
            let stated = stated_list_members(&enumeration.evidence.text, expected_object_kind);
            let members: BTreeSet<_> = enumeration
                .member_claim_indexes
                .iter()
                .filter_map(|index| {
                    let claim = response.claims.get(*index)?;
                    let model::ClaimObject::Entity(entity) = &claim.object else {
                        return None;
                    };
                    // The member claim must cite the list itself, even if it also
                    // cites a narrower passage elsewhere in the document.
                    let cites_list = claim.evidence.iter().any(|evidence| {
                        evidence.passage_id == enumeration.evidence.passage_id
                            && evidence.start_line <= enumeration.evidence.start_line
                            && evidence.end_line >= enumeration.evidence.end_line
                    });
                    cites_list.then(|| entity.canonicalized().canonical_key)
                })
                .collect();
            enumeration.exhaustive =
                stated.is_some_and(|stated| !stated.is_empty() && stated == members);
        }
        Ok(())
        })();
        match validation {
            Ok(()) => valid_enumerations.push(enumeration),
            Err(error) => eprintln!("graph build: {source_path}: omitting invalid enumeration {}: {error}", index + 1),
        }
    }
    response.enumerations = valid_enumerations;
    Ok(response)
}

fn evidence_text_matches_snapshot(proposed: &str, snapshot: &str) -> bool {
    !proposed.trim().is_empty()
        && proposed.split_whitespace().eq(snapshot.split_whitespace())
}

/// Complete a model's partial quote when it is a genuine word-sequence
/// prefix of the already-declared, already-correct evidence range.
///
/// Unlike `unique_evidence_span_in_passage`, this never searches for
/// anything: `expected` already comes from the model's own declared
/// `start_line`/`end_line`, which are not in question here. The only thing
/// checked is whether the model's quote stops partway through that exact,
/// unambiguously identified text (a sentence-boundary truncation within one
/// physical Markdown line, or of the last line of an otherwise-exact
/// multi-line range) rather than diverging from it. If any compared word
/// differs, this returns `None` and the caller falls through to the
/// existing, stricter failure path unchanged — a truncation point is the
/// only thing this function is allowed to forgive.
fn extend_prefix_evidence_to_full_lines(proposed: &str, expected: &str) -> Option<String> {
    if proposed.trim().is_empty() {
        return None;
    }
    let mut proposed_words = proposed.split_whitespace();
    let mut expected_words = expected.split_whitespace();
    loop {
        match (proposed_words.next(), expected_words.next()) {
            (Some(p), Some(e)) => {
                if p != e {
                    return None;
                }
            }
            (Some(_), None) => return None, // proposed has extra words: not a prefix
            (None, Some(_)) => return Some(expected.to_owned()), // genuine proper prefix
            (None, None) => return None, // exact match: handled elsewhere
        }
    }
}

/// Correct a model's line numbers only when its wording identifies exactly
/// one contiguous span in the selected, snapshot-verified passage.
///
/// `evidence_text_matches_snapshot` compares word sequences, so a
/// whitespace-only Markdown paragraph-separator line contributes no words to
/// either side of the comparison. Without care, that lets the search below
/// report two "different" spans for what is really the same cited wording —
/// for example the true span plus the same span with a trailing blank line
/// (or a leading blank line before it) folded in — and the early bail-out
/// on ambiguity would then wrongly refuse to re-anchor a claim whose
/// evidence is otherwise an exact, unique match. To keep the ambiguity check
/// meaningful, a candidate span must both start on a line with real content
/// and stop at the first (minimal) end that matches; growing further can
/// only ever re-match by appending blank lines, which is not new evidence.
fn unique_evidence_span_in_passage(
    proposed: &str,
    passage: &SourcePassage,
) -> Option<(u32, u32, String)> {
    if proposed.trim().is_empty() {
        return None;
    }
    let lines: Vec<&str> = passage.text.split_inclusive('\n').collect();
    let mut unique = None;
    for start in 0..lines.len() {
        if lines[start].trim().is_empty() {
            continue;
        }
        let mut exact = String::new();
        for (end, line) in lines.iter().enumerate().skip(start) {
            exact.push_str(line);
            if evidence_text_matches_snapshot(proposed, &exact) {
                if unique.is_some() {
                    return None;
                }
                unique = Some((
                    passage.start_line + start as u32,
                    passage.start_line + end as u32,
                    exact.clone(),
                ));
                // The minimal match at this start is canonical; continuing
                // to extend could only re-match by absorbing trailing blank
                // lines, which would be the same evidence, not new evidence.
                break;
            }
        }
    }
    unique
}

fn log_evidence_mismatch(
    source_path: &str,
    start_line: u32,
    end_line: u32,
    proposed: &str,
    snapshot: &str,
) {
    if std::env::var_os("DAFTPROMPT_GRAPH_DEBUG_EVIDENCE").as_deref()
        != Some(std::ffi::OsStr::new("1"))
    {
        return;
    }
    eprintln!(
        "{}",
        evidence_mismatch_diagnostic(source_path, start_line, end_line, proposed, snapshot)
    );
}

/// Bound and escape source text for inclusion in stderr diagnostics and
/// validation-error messages (which, for an evidence-text mismatch, become
/// part of the one-shot correction prompt in `extraction_correction_contract`
/// via `model.rs`). This is already-selected, already-cited source text, not
/// new or sensitive data; the bound simply keeps a single bad citation from
/// dominating the message.
fn bounded_escaped(text: &str) -> String {
    let prefix: String = text.chars().take(500).collect();
    let suffix = (text.chars().count() > 500).then_some(" [truncated]").unwrap_or("");
    format!("{prefix:?}{suffix}")
}

fn evidence_mismatch_diagnostic(
    source_path: &str,
    start_line: u32,
    end_line: u32,
    proposed: &str,
    snapshot: &str,
) -> String {
    format!(
        "graph build: evidence mismatch at {source_path:?}:{start_line}-{end_line}: model={}; snapshot={}",
        bounded_escaped(proposed),
        bounded_escaped(snapshot)
    )
}

fn explicitly_complete_wording(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "all supported",
        "only supported",
        "complete list",
        "full list",
        "the entire set",
        "every supported",
    ]
    .iter()
    .any(|cue| text.contains(cue))
}

/// Recognize a simple, closed list in the cited wording. Unrecognized prose
/// leaves the enumeration open, since absence is otherwise used to prove `no`.
fn stated_list_members(text: &str, kind: &str) -> Option<BTreeSet<String>> {
    let mut without_parentheses = String::new();
    let mut depth = 0usize;
    for ch in text.chars() {
        match ch {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => without_parentheses.push(ch),
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    let lower = without_parentheses.to_ascii_lowercase();
    let plural = match kind {
        "language" => "languages",
        "dialect" => "dialects",
        "extension" => "extensions",
        _ => return None,
    };
    let cue_start = [
        "all supported",
        "only supported",
        "complete list",
        "full list",
        "the entire set",
        "every supported",
    ]
    .iter()
    .filter_map(|cue| lower.find(cue))
    .min()?;
    let start = cue_start + lower.get(cue_start..)?.find(plural)? + plural.len();
    let rest = without_parentheses.get(start..)?;
    let rest = rest.trim_start();
    let rest = rest
        .strip_prefix(':')
        .or_else(|| rest.strip_prefix("are "))?;
    let next_list = rest.to_ascii_lowercase().find(", and all supported ");
    let end = [rest.find([';', '.']), next_list]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(rest.len());
    let list = rest.get(..end)?.trim();
    let mut names = BTreeSet::new();
    for item in list
        .split(',')
        .map(str::trim)
        .flat_map(|item| item.split(" and "))
    {
        let name = item
            .trim()
            .trim_start_matches("and ")
            .trim_start_matches("Git-tracked ")
            .trim();
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch.is_alphanumeric() || matches!(ch, ' ' | '+' | '#' | '-' | '.'))
        {
            return None;
        }
        names.insert(
            model::ClaimEntity {
                kind: kind.into(),
                name: name.into(),
                aliases: Vec::new(),
            }
            .canonicalized()
            .canonical_key,
        );
    }
    (!names.is_empty()).then_some(names)
}

/// Store an already validated extraction as one atomic graph update. The FTS
/// table is maintained by schema triggers as claims and aliases are inserted.
pub fn persist_extraction(
    db: &Connection,
    build_id: &str,
    source_version_id: i64,
    response: &model::ExtractionResponse,
) -> Result<Vec<String>> {
    let tx = db.unchecked_transaction()?;
    let ids = persist_extraction_tx(&tx, build_id, source_version_id, response)?;
    tx.commit()?;
    Ok(ids)
}

fn persist_extraction_tx(
    tx: &rusqlite::Transaction<'_>,
    build_id: &str,
    source_version_id: i64,
    response: &model::ExtractionResponse,
) -> Result<Vec<String>> {
    use model::ClaimObject;
    let selected: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM graph_build_sources WHERE build_id=?1 AND source_version_id=?2)",
        rusqlite::params![build_id, source_version_id],
        |row| row.get(0),
    )?;
    anyhow::ensure!(
        selected,
        "source version {source_version_id} is not selected by graph build {build_id}"
    );
    let mut persisted_ids = Vec::with_capacity(response.claims.len());
    for claim in &response.claims {
        anyhow::ensure!(
            !claim.evidence.is_empty(),
            "cannot persist a claim without validated evidence"
        );
        let subject = persist_entity(&tx, &claim.subject)?;
        let (object_node, object_literal) = match &claim.object {
            ClaimObject::Entity(entity) => (Some(persist_entity(&tx, entity)?), None),
            ClaimObject::Literal { value, .. } => (None, Some(value.as_str())),
        };
        let evidence_key = claim
            .evidence
            .iter()
            .map(|e| format!("{}:{}-{}", e.passage_id, e.start_line, e.end_line))
            .collect::<Vec<_>>()
            .join(",");
        let object_identity = object_node
            .map(|id| format!("node:{id}"))
            .or_else(|| object_literal.map(|value| format!("literal:{value}")))
            .unwrap_or_default();
        let stable_payload = format!(
            "{}|{}|{}|{}|{:?}|{:?}|{:?}|{}",
            source_version_id,
            subject,
            claim.predicate,
            object_identity,
            claim.polarity,
            claim.status,
            claim.scope,
            evidence_key
        );
        let claim_id = format!(
            "c{:016x}",
            xxhash_rust::xxh3::xxh3_64(stable_payload.as_bytes())
        );
        let object_key = object_node;
        tx.execute(
            "INSERT INTO graph_claims(id, subject_node_id, predicate, object_node_id, object_literal, polarity, modality, status, scope, source_version_id, build_id, claim_text, active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1)
             ON CONFLICT(id) DO UPDATE SET active=1, claim_text=excluded.claim_text",
            rusqlite::params![claim_id, subject, claim.predicate, object_key, object_literal,
                match claim.polarity { model::Polarity::Positive => "positive", model::Polarity::Negative => "negative" },
                format!("{:?}", claim.status).to_ascii_lowercase(),
                format!("{:?}", claim.status).to_ascii_lowercase(),
                format!("{:?}", claim.scope).to_ascii_lowercase(), source_version_id, build_id,
                claim.evidence.iter().map(|e| e.text.as_str()).collect::<Vec<_>>().join(" ")],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO graph_build_claims(build_id, claim_id) VALUES (?1, ?2)",
            rusqlite::params![build_id, claim_id],
        )?;
        tx.execute(
            "DELETE FROM graph_claim_evidence WHERE claim_id=?1",
            [&claim_id],
        )?;
        for evidence in &claim.evidence {
            let span_id = persist_evidence_span_tx(tx, source_version_id, evidence)?;
            tx.execute(
                "INSERT OR IGNORE INTO graph_claim_evidence(claim_id, span_id) VALUES (?1, ?2)",
                rusqlite::params![claim_id, span_id],
            )?;
        }
        persisted_ids.push(claim_id);
    }
    for (i, enumeration) in response.enumerations.iter().enumerate() {
        if enumeration.member_claim_indexes.is_empty() {
            continue;
        }
        let span_id = persist_evidence_span_tx(tx, source_version_id, &enumeration.evidence)?;
        let subject = persist_entity(&tx, &enumeration.subject)?;
        let enum_id = format!(
            "e{:016x}",
            xxhash_rust::xxh3::xxh3_64(
                format!(
                    "{source_version_id}|{}|{}|{}|{}",
                    subject, enumeration.predicate, span_id, i
                )
                .as_bytes()
            )
        );
        tx.execute("INSERT INTO graph_enumerations(id, subject_node_id, predicate, scope, exhaustive, source_version_id, supporting_span_id, build_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(id) DO NOTHING",
            rusqlite::params![enum_id, subject, enumeration.predicate, format!("{:?}", enumeration.scope).to_ascii_lowercase(), enumeration.exhaustive as i32, source_version_id, span_id, build_id])?;
        tx.execute("INSERT INTO graph_build_enumerations(build_id, enumeration_id, exhaustive) VALUES (?1, ?2, ?3) ON CONFLICT(build_id, enumeration_id) DO UPDATE SET exhaustive=excluded.exhaustive",
            rusqlite::params![build_id, enum_id, enumeration.exhaustive as i32])?;
        for member in &enumeration.member_claim_indexes {
            let claim_id = persisted_ids
                .get(*member)
                .ok_or_else(|| anyhow::anyhow!("enumeration member index {member} is missing"))?;
            tx.execute("INSERT OR IGNORE INTO graph_build_enumeration_members(build_id, enumeration_id, claim_id) VALUES (?1, ?2, ?3)", rusqlite::params![build_id, enum_id, claim_id])?;
        }
    }
    Ok(persisted_ids)
}

fn persist_evidence_span_tx(
    tx: &rusqlite::Transaction<'_>,
    source_version_id: i64,
    evidence: &model::ClaimEvidence,
) -> Result<String> {
    let passage: Option<(String, u32, u32, String)> = tx
        .query_row(
            "SELECT heading_path, start_line, end_line, text FROM graph_spans
             WHERE id=?1 AND source_version_id=?2",
            rusqlite::params![evidence.passage_id, source_version_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let (heading, passage_start, passage_end, passage_text) = passage.ok_or_else(|| {
        anyhow::anyhow!(
            "evidence passage {} does not belong to source version {}",
            evidence.passage_id,
            source_version_id
        )
    })?;
    anyhow::ensure!(
        evidence.start_line >= passage_start
            && evidence.end_line >= evidence.start_line
            && evidence.end_line <= passage_end,
        "evidence range is outside passage {}",
        evidence.passage_id
    );
    let snapshot: String = tx.query_row(
        "SELECT snapshot FROM graph_source_versions WHERE id=?1",
        [source_version_id],
        |row| row.get(0),
    )?;
    anyhow::ensure!(
        snapshot_lines(&snapshot, passage_start, passage_end).as_deref()
            == Some(passage_text.as_str()),
        "evidence passage {} does not match source snapshot",
        evidence.passage_id
    );
    anyhow::ensure!(
        snapshot_lines(&snapshot, evidence.start_line, evidence.end_line).as_deref()
            == Some(evidence.text.as_str()),
        "evidence text does not match source snapshot at {}-{}",
        evidence.start_line,
        evidence.end_line
    );
    let key = format!(
        "{}|{}|{}|{}|{}",
        source_version_id,
        evidence.passage_id,
        evidence.start_line,
        evidence.end_line,
        evidence.text
    );
    let span_id = format!(
        "sv{source_version_id}:e{:016x}",
        xxhash_rust::xxh3::xxh3_64(key.as_bytes())
    );
    tx.execute(
        "INSERT OR IGNORE INTO graph_spans(id, source_version_id, heading_path, start_line, end_line, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![span_id, source_version_id, heading, evidence.start_line, evidence.end_line, evidence.text],
    )?;
    Ok(span_id)
}

fn persist_entity(tx: &rusqlite::Transaction<'_>, entity: &model::ClaimEntity) -> Result<i64> {
    let canonical = entity.canonicalized();
    tx.execute("INSERT INTO graph_nodes(canonical_key, kind, display_name) VALUES (?1, ?2, ?3) ON CONFLICT(canonical_key) DO UPDATE SET display_name=excluded.display_name",
        rusqlite::params![canonical.canonical_key, canonical.kind, canonical.display_name])?;
    let id: i64 = tx.query_row(
        "SELECT id FROM graph_nodes WHERE canonical_key=?1",
        [&canonical.canonical_key],
        |r| r.get(0),
    )?;
    for alias in canonical.aliases {
        tx.execute("INSERT OR IGNORE INTO graph_aliases(node_id, kind, alias, normalized_alias) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![id, alias.kind, alias.surface, alias.normalized])?;
    }
    Ok(id)
}

/// Return the exact bytes represented by a one-based inclusive line range.
fn snapshot_lines(snapshot: &str, start_line: u32, end_line: u32) -> Option<String> {
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

/// The complete set of valid entity `kind` values. Scope names (`project`,
/// `epic`, `task`, `feature` as a *scope*) must never appear here — scopes
/// are a separate field (`ClaimScope`) from entity kind, and `"feature"`
/// below is the entity kind for an indexed product feature, not the scope.
/// [`model::EXTRACTION_CONTRACT`]'s prompt text must list every value in
/// this slice; `model::tests::extraction_contract_lists_every_valid_entity_kind`
/// asserts that so the two never silently drift apart.
pub(crate) const ENTITY_KINDS: &[&str] = &[
    "feature",
    "index",
    "language",
    "dialect",
    "extension",
    "parser",
    "work_item",
];

fn validate_entity(label: &str, position: &str, entity: &model::ClaimEntity) -> Result<()> {
    anyhow::ensure!(
        ENTITY_KINDS.contains(&entity.kind.as_str()),
        "{label}: invalid {position} entity kind {:?}",
        entity.kind
    );
    anyhow::ensure!(
        !entity.name.trim().is_empty(),
        "{label}: {position} entity name is empty"
    );
    anyhow::ensure!(
        entity.aliases.iter().all(|alias| !alias.trim().is_empty()),
        "{label}: {position} entity has an empty alias"
    );
    Ok(())
}

fn canonicalize_claim_entity(entity: &mut model::ClaimEntity) {
    let canonical = entity.canonicalized();
    entity.kind = canonical.kind;
    entity.name = canonical.display_name;
    // Keep the model's surface aliases intact in the DTO. Their typed,
    // normalized representation is available from ClaimEntity::canonicalized
    // for graph_aliases persistence in the next task.
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use model::{
        ClaimEntity, ClaimEvidence, ClaimObject, ClaimScope, ClaimStatus, ExtractedClaim,
        ExtractionResponse, Polarity,
    };
    use std::sync::{Condvar, Mutex};

    #[test]
    fn evidence_mismatch_diagnostic_is_escaped_and_bounded() {
        let proposed = format!("\"wrong\n{}", "x".repeat(600));
        let message = evidence_mismatch_diagnostic("epics/test.md", 38, 38, &proposed, "right\nline");
        assert!(message.contains("epics/test.md\":38-38"));
        assert!(message.contains("model=\"\\\"wrong\\n"));
        assert!(message.contains("snapshot=\"right\\nline\""));
        assert!(message.contains("[truncated]"));
        assert!(!message.contains(&"x".repeat(501)));
    }

    struct InterpretationRetryFixture {
        first: model::InterpretationResponse,
        corrected: model::InterpretationResponse,
        retries: Mutex<Vec<model::InterpretationRequest>>,
    }

    impl model::GraphModel for InterpretationRetryFixture {
        fn extract_claims(&self, _request: &model::ExtractionRequest) -> Result<ExtractionResponse> {
            unreachable!()
        }

        fn interpret_question(
            &self,
            _request: &model::InterpretationRequest,
        ) -> Result<model::InterpretationResponse> {
            Ok(self.first.clone())
        }

        fn retry_interpretation(
            &self,
            request: &model::InterpretationRequest,
            _validation_error: &str,
        ) -> Option<Result<model::InterpretationResponse>> {
            self.retries.lock().unwrap().push(request.clone());
            Some(Ok(self.corrected.clone()))
        }

        fn model_name(&self) -> &str {
            "interpretation retry fixture"
        }
    }

    #[test]
    fn invalid_interpretation_gets_one_correction_with_active_vocabulary() {
        let request = model::InterpretationRequest {
            question: "Does the code index index TypeScript?".into(),
            candidate_entities: vec![ClaimEntity {
                kind: "language".into(),
                name: "TypeScript".into(),
                aliases: vec![],
            }],
            candidate_predicates: vec!["indexes_language".into()],
        };
        let first = model::InterpretationResponse {
            intent: model::QuestionIntent::DirectRelation,
            entities: request.candidate_entities.clone(),
            predicates: vec!["supports_language".into()],
        };
        let mut corrected = first.clone();
        corrected.predicates = request.candidate_predicates.clone();
        let adapter = InterpretationRetryFixture {
            first,
            corrected: corrected.clone(),
            retries: Mutex::new(vec![]),
        };

        assert_eq!(interpret_with_one_correction(&adapter, &request).unwrap(), corrected);
        assert_eq!(*adapter.retries.lock().unwrap(), [request]);
    }

    #[test]
    fn missing_code_search_target_gets_one_correction() {
        let request = model::InterpretationRequest {
            question: "Can we search Rust code?".into(),
            candidate_entities: vec![ClaimEntity {
                kind: "language".into(),
                name: "Rust".into(),
                aliases: vec![],
            }],
            candidate_predicates: vec!["indexes_language".into()],
        };
        let first = model::InterpretationResponse {
            intent: model::QuestionIntent::CodeSearchLanguage,
            entities: vec![],
            predicates: request.candidate_predicates.clone(),
        };
        let corrected = model::InterpretationResponse {
            entities: request.candidate_entities.clone(),
            ..first.clone()
        };
        let adapter = InterpretationRetryFixture {
            first,
            corrected: corrected.clone(),
            retries: Mutex::new(vec![]),
        };

        assert_eq!(interpret_with_one_correction(&adapter, &request).unwrap(), corrected);
        assert_eq!(adapter.retries.lock().unwrap().len(), 1);
    }

    #[test]
    fn code_search_language_target_must_match_question() {
        let request = model::InterpretationRequest {
            question: "Can we search Python code?".into(),
            candidate_entities: vec![
                ClaimEntity { kind: "language".into(), name: "Python".into(), aliases: vec!["Py".into()] },
                ClaimEntity { kind: "language".into(), name: "Rust".into(), aliases: vec![] },
            ],
            candidate_predicates: vec!["indexes_language".into()],
        };
        let interpretation = |name: &str| model::InterpretationResponse {
            intent: model::QuestionIntent::CodeSearchLanguage,
            entities: vec![ClaimEntity { kind: "language".into(), name: name.into(), aliases: vec![] }],
            predicates: vec!["indexes_language".into()],
        };
        assert!(validate_interpretation(&interpretation("Python"), &request).is_ok());
        assert!(validate_interpretation(&interpretation("Rust"), &request)
            .unwrap_err().to_string().contains("does not match"));
        assert!(validate_interpretation(&interpretation("code"), &request).is_err());
        assert!(validate_interpretation(&interpretation("Lua"), &request).is_err());

        let mut unknown = request.clone();
        unknown.question = "Can we search Lua code?".into();
        assert!(validate_interpretation(&interpretation("Lua"), &unknown).is_ok());
        assert!(validate_interpretation(&interpretation("code"), &unknown).is_err());

        let mut ambiguous = request;
        ambiguous.question = "Can we search Python and Rust code?".into();
        assert!(validate_interpretation(&interpretation("Python"), &ambiguous)
            .unwrap_err().to_string().contains("multiple known languages"));
    }

    #[test]
    fn mismatched_code_search_target_cannot_survive_model_correction() {
        let request = model::InterpretationRequest {
            question: "Can we search Python code?".into(),
            candidate_entities: vec![
                ClaimEntity { kind: "language".into(), name: "Python".into(), aliases: vec![] },
                ClaimEntity { kind: "language".into(), name: "Rust".into(), aliases: vec![] },
            ],
            candidate_predicates: vec!["indexes_language".into()],
        };
        let wrong = model::InterpretationResponse {
            intent: model::QuestionIntent::CodeSearchLanguage,
            entities: vec![ClaimEntity { kind: "language".into(), name: "Rust".into(), aliases: vec![] }],
            predicates: vec!["indexes_language".into()],
        };
        let adapter = InterpretationRetryFixture {
            first: wrong.clone(), corrected: wrong, retries: Mutex::new(vec![]),
        };
        assert!(interpret_with_one_correction(&adapter, &request)
            .unwrap_err().to_string().contains("does not match"));
        assert_eq!(adapter.retries.lock().unwrap().len(), 1);
    }

    #[test]
    fn explicit_index_question_corrects_code_search_intent() {
        let request = model::InterpretationRequest {
            question: "Does the code index index TypeScript?".into(),
            candidate_entities: vec![
                ClaimEntity {
                    kind: "index".into(),
                    name: "code index".into(),
                    aliases: vec![],
                },
                ClaimEntity {
                    kind: "language".into(),
                    name: "TypeScript".into(),
                    aliases: vec![],
                },
            ],
            candidate_predicates: vec!["indexes_language".into()],
        };
        let first = model::InterpretationResponse {
            intent: model::QuestionIntent::CodeSearchLanguage,
            entities: request.candidate_entities.clone(),
            predicates: request.candidate_predicates.clone(),
        };
        let corrected = model::InterpretationResponse {
            intent: model::QuestionIntent::DirectRelation,
            ..first.clone()
        };
        let adapter = InterpretationRetryFixture {
            first,
            corrected: corrected.clone(),
            retries: Mutex::new(vec![]),
        };

        assert_eq!(interpret_with_one_correction(&adapter, &request).unwrap(), corrected);
        assert_eq!(adapter.retries.lock().unwrap().len(), 1);
    }

    #[test]
    fn direct_language_relation_corrects_extension_entity() {
        let request = model::InterpretationRequest {
            question: "Does the code index index Rust?".into(),
            candidate_entities: vec![
                ClaimEntity { kind: "index".into(), name: "code index".into(), aliases: vec![] },
                ClaimEntity { kind: "extension".into(), name: ".rs".into(), aliases: vec![] },
                ClaimEntity { kind: "language".into(), name: "Rust".into(), aliases: vec![".rs".into()] },
            ],
            candidate_predicates: vec!["indexes_language".into()],
        };
        let first = model::InterpretationResponse {
            intent: model::QuestionIntent::DirectRelation,
            entities: request.candidate_entities[..2].to_vec(),
            predicates: request.candidate_predicates.clone(),
        };
        let corrected = model::InterpretationResponse {
            entities: vec![request.candidate_entities[0].clone(), request.candidate_entities[2].clone()],
            ..first.clone()
        };
        let adapter = InterpretationRetryFixture {
            first,
            corrected: corrected.clone(),
            retries: Mutex::new(vec![]),
        };
        assert_eq!(interpret_with_one_correction(&adapter, &request).unwrap(), corrected);
        assert_eq!(adapter.retries.lock().unwrap().len(), 1);
    }

    #[test]
    fn interpreted_entity_resolution_prefers_exact_kind_over_extension_alias() {
        let candidates = vec![
            ("extension:rs".into(), ClaimEntity { kind: "extension".into(), name: ".rs".into(), aliases: vec![] }),
            ("language:rust".into(), ClaimEntity { kind: "language".into(), name: "Rust".into(), aliases: vec![".rs".into()] }),
        ];
        let rust = ClaimEntity { kind: "language".into(), name: "Rust".into(), aliases: vec![".rs".into()] };
        assert_eq!(resolve_interpreted_entity_key(&rust, &candidates).unwrap(), "language:rust");
        let ruby = ClaimEntity { kind: "language".into(), name: "Ruby".into(), aliases: vec![".rb".into()] };
        assert_eq!(resolve_interpreted_entity_key(&ruby, &candidates).unwrap(), "language:ruby");
    }

    #[test]
    fn ambiguous_same_kind_alias_is_an_error() {
        let candidates = vec![
            ("language:one".into(), ClaimEntity { kind: "language".into(), name: "One".into(), aliases: vec!["Shared".into()] }),
            ("language:two".into(), ClaimEntity { kind: "language".into(), name: "Two".into(), aliases: vec!["Shared".into()] }),
        ];
        let proposed = ClaimEntity { kind: "language".into(), name: "Shared".into(), aliases: vec![] };
        assert!(resolve_interpreted_entity_key(&proposed, &candidates)
            .unwrap_err().to_string().contains("multiple active language matches"));
    }

    #[test]
    fn invalid_interpretation_after_correction_remains_an_error() {
        let request = model::InterpretationRequest {
            question: "Does the code index index TypeScript?".into(),
            candidate_entities: vec![],
            candidate_predicates: vec!["indexes_language".into()],
        };
        let invalid = model::InterpretationResponse {
            intent: model::QuestionIntent::DirectRelation,
            entities: vec![],
            predicates: vec!["supports_language".into()],
        };
        let adapter = InterpretationRetryFixture {
            first: invalid.clone(),
            corrected: invalid,
            retries: Mutex::new(vec![]),
        };

        let error = interpret_with_one_correction(&adapter, &request)
            .unwrap_err()
            .to_string();
        assert!(error.contains("predicate outside the active graph vocabulary"));
        assert!(error.contains("model correction also failed"));
        assert_eq!(adapter.retries.lock().unwrap().len(), 1);
    }

    fn fixture() -> (String, Vec<SourcePassage>, ExtractionResponse) {
        let snapshot = "# Search\nThe code index indexes Rust.\n".to_owned();
        let passage = SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: "Search".into(),
            start_line: 1,
            end_line: 2,
            text: "# Search\nThe code index indexes Rust.\n".into(),
        };
        let claim = ExtractedClaim {
            subject: ClaimEntity {
                kind: "index".into(),
                name: "CodeIndex".into(),
                aliases: vec![],
            },
            predicate: "indexes_language".into(),
            object: ClaimObject::Entity(ClaimEntity {
                kind: "language".into(),
                name: "Rust".into(),
                aliases: vec![".rs".into()],
            }),
            polarity: Polarity::Positive,
            status: ClaimStatus::Current,
            scope: ClaimScope::Project,
            evidence: vec![ClaimEvidence {
                passage_id: passage.id.clone(),
                start_line: 2,
                end_line: 2,
                text: "The code index indexes Rust.\n".into(),
            }],
        };
        (
            snapshot,
            vec![passage],
            ExtractionResponse {
                claims: vec![claim],
                enumerations: vec![],
            },
        )
    }

    struct InvalidCorrectionFixture {
        retries: Mutex<usize>,
    }

    impl model::GraphModel for InvalidCorrectionFixture {
        fn extract_claims(&self, request: &model::ExtractionRequest) -> Result<ExtractionResponse> {
            let passage = &request.passages[0];
            let mut response = ExtractionResponse {
                claims: vec![ExtractedClaim {
                    subject: ClaimEntity { kind: "index".into(), name: "CodeIndex".into(), aliases: vec![] },
                    predicate: "indexes_language".into(),
                    object: ClaimObject::Entity(ClaimEntity { kind: "language".into(), name: "Rust".into(), aliases: vec![] }),
                    polarity: Polarity::Positive,
                    status: ClaimStatus::Current,
                    scope: ClaimScope::Project,
                    evidence: vec![ClaimEvidence {
                        passage_id: passage.id.clone(),
                        start_line: passage.start_line,
                        end_line: passage.end_line,
                        text: "Invented evidence.\n".into(),
                    }],
                }],
                enumerations: vec![],
            };
            let mut valid = response.claims[0].clone();
            valid.evidence[0].text = passage.text.clone();
            response.claims.insert(0, valid);
            Ok(response)
        }

        fn retry_extraction(
            &self,
            request: &model::ExtractionRequest,
            _previous_response: &ExtractionResponse,
            validation_error: &str,
        ) -> Option<Result<ExtractionResponse>> {
            assert!(validation_error.contains("evidence text does not match"));
            *self.retries.lock().unwrap() += 1;
            Some(self.extract_claims(request))
        }

        fn interpret_question(
            &self,
            _request: &model::InterpretationRequest,
        ) -> Result<model::InterpretationResponse> {
            unreachable!()
        }

        fn model_name(&self) -> &str {
            "invalid-correction-fixture"
        }
    }

    #[test]
    fn corrected_extraction_with_one_invalid_claim_fails_entire_batch() {
        let (snapshot, passages, _) = fixture();
        let source = SelectedSource {
            path: "README.md".into(),
            bytes: snapshot.as_bytes().to_vec(),
            text: snapshot,
        };
        let adapter = InvalidCorrectionFixture { retries: Mutex::new(0) };
        let error = extract_batch(&adapter, &source, &passages, 0, 1)
            .unwrap_err()
            .to_string();
        assert!(error.contains("source README.md batch 1/1"));
        assert!(error.contains("model correction also failed"));
        assert!(error.contains("claim 2: evidence text does not match"));
        assert_eq!(*adapter.retries.lock().unwrap(), 1);
    }

    #[test]
    fn failed_correction_preserves_previous_completed_build() -> Result<()> {
        let repo = tempfile::tempdir()?;
        let cache = tempfile::tempdir()?;
        let config = crate::IndexerConfig {
            cache_dir: Some(cache.path().to_path_buf()),
            ..crate::IndexerConfig::default()
        };
        let db = open_database(repo.path(), &config)?;
        init_schema(&db)?;
        let (snapshot, _, _) = fixture();
        let source = SelectedSource {
            path: "README.md".into(),
            bytes: snapshot.as_bytes().to_vec(),
            text: snapshot,
        };
        let successful = build(
            &db,
            repo.path(),
            std::slice::from_ref(&source),
            &BatchedFixture { requests: Mutex::new(vec![]) },
            "fixture-v1",
        )?;
        let active_before: i64 = db.query_row(
            "SELECT COUNT(*) FROM graph_claims WHERE active = 1",
            [],
            |row| row.get(0),
        )?;
        assert!(active_before > 0);

        let adapter = InvalidCorrectionFixture { retries: Mutex::new(0) };
        let error = build(&db, repo.path(), &[source], &adapter, "fixture-v2")
            .unwrap_err()
            .to_string();
        assert!(error.contains("model correction also failed"));
        assert_eq!(*adapter.retries.lock().unwrap(), 1);
        let latest_completed: String = db.query_row(
            "SELECT id FROM graph_builds WHERE state = 'completed' ORDER BY completed_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(latest_completed, successful);
        let active_after: i64 = db.query_row(
            "SELECT COUNT(*) FROM graph_claims WHERE active = 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(active_after, active_before);
        let failed: i64 = db.query_row(
            "SELECT COUNT(*) FROM graph_builds WHERE state = 'failed'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(failed, 1);
        Ok(())
    }

    struct BatchedFixture {
        requests: Mutex<Vec<Vec<String>>>,
    }

    #[test]
    fn passage_batches_respect_byte_and_count_limits() {
        let passages: Vec<_> = (0..7)
            .map(|index| SourcePassage {
                id: format!("sv1:p{index}"),
                source_version_id: 1,
                heading_path: String::new(),
                start_line: index + 1,
                end_line: index + 1,
                text: "abc".into(),
            })
            .collect();

        let by_count = passage_batches(&passages, Some(6_000), Some(4));
        assert_eq!(by_count.iter().map(|batch| batch.len()).collect::<Vec<_>>(), [4, 3]);
        assert_eq!(by_count[0][0].id, "sv1:p0");
        assert_eq!(by_count[1][0].id, "sv1:p4");

        let by_bytes = passage_batches(&passages, Some(6), Some(4));
        assert_eq!(
            by_bytes.iter().map(|batch| batch.len()).collect::<Vec<_>>(),
            [2, 2, 2, 1]
        );

        let unlimited = passage_batches(&passages, None, None);
        assert_eq!(unlimited.len(), 1);
        assert_eq!(unlimited[0].len(), passages.len());
    }

    impl model::GraphModel for BatchedFixture {
        fn extraction_batch_max_chars(&self) -> Option<usize> {
            Some(45)
        }

        fn extract_claims(&self, request: &model::ExtractionRequest) -> Result<ExtractionResponse> {
            self.requests.lock().unwrap().push(
                request
                    .passages
                    .iter()
                    .map(|passage| passage.id.clone())
                    .collect(),
            );
            let passage = &request.passages[0];
            let language = if passage.text.contains("Rust") {
                "Rust"
            } else {
                "TypeScript"
            };
            let subject = ClaimEntity {
                kind: "index".into(),
                name: "CodeIndex".into(),
                aliases: vec![],
            };
            let evidence = ClaimEvidence {
                passage_id: passage.id.clone(),
                start_line: passage.start_line,
                end_line: passage.end_line,
                text: passage.text.clone(),
            };
            Ok(ExtractionResponse {
                claims: vec![ExtractedClaim {
                    subject: subject.clone(),
                    predicate: "indexes_language".into(),
                    object: ClaimObject::Entity(ClaimEntity {
                        kind: "language".into(),
                        name: language.into(),
                        aliases: vec![],
                    }),
                    polarity: Polarity::Positive,
                    status: ClaimStatus::Current,
                    scope: ClaimScope::Project,
                    evidence: vec![evidence.clone()],
                }],
                enumerations: vec![model::ExtractedEnumeration {
                    subject,
                    predicate: "indexes_language".into(),
                    scope: ClaimScope::Project,
                    exhaustive: false,
                    evidence,
                    member_claim_indexes: vec![0],
                }],
            })
        }

        fn interpret_question(
            &self,
            _request: &model::InterpretationRequest,
        ) -> Result<model::InterpretationResponse> {
            unreachable!()
        }

        fn model_name(&self) -> &str {
            "batched fixture"
        }
    }

    #[test]
    fn batches_passages_and_rebases_enumeration_members() {
        let text = "The code index indexes Rust.\nThe code index indexes TypeScript.\n";
        let passages = vec![
            SourcePassage {
                id: "sv1:p0".into(),
                source_version_id: 1,
                heading_path: String::new(),
                start_line: 1,
                end_line: 1,
                text: "The code index indexes Rust.\n".into(),
            },
            SourcePassage {
                id: "sv1:p1".into(),
                source_version_id: 1,
                heading_path: String::new(),
                start_line: 2,
                end_line: 2,
                text: "The code index indexes TypeScript.\n".into(),
            },
        ];
        let source = SelectedSource {
            path: "README.md".into(),
            bytes: text.as_bytes().to_vec(),
            text: text.into(),
        };
        let adapter = BatchedFixture {
            requests: Mutex::new(Vec::new()),
        };
        let result = extract_source_in_batches(&adapter, &source, &passages).unwrap();
        assert_eq!(
            *adapter.requests.lock().unwrap(),
            vec![vec!["sv1:p0".to_owned()], vec!["sv1:p1".to_owned()]]
        );
        assert_eq!(result.claims.len(), 2);
        assert_eq!(result.enumerations[0].member_claim_indexes, [0]);
        assert_eq!(result.enumerations[1].member_claim_indexes, [1]);
        validate_extraction("README.md", &source.text, &passages, result).unwrap();
    }

    struct OutOfOrderFixture {
        inner: BatchedFixture,
        second_done: (Mutex<bool>, Condvar),
        completed: Mutex<Vec<String>>,
    }

    impl model::GraphModel for OutOfOrderFixture {
        fn extraction_batch_max_chars(&self) -> Option<usize> {
            self.inner.extraction_batch_max_chars()
        }

        fn extract_claims(&self, request: &model::ExtractionRequest) -> Result<ExtractionResponse> {
            let id = &request.passages[0].id;
            if id == "sv1:p0" {
                let (lock, ready) = &self.second_done;
                let mut done = lock.lock().unwrap();
                while !*done {
                    done = ready.wait(done).unwrap();
                }
            }
            let response = self.inner.extract_claims(request)?;
            self.completed.lock().unwrap().push(id.clone());
            if id == "sv1:p1" {
                let (lock, ready) = &self.second_done;
                *lock.lock().unwrap() = true;
                ready.notify_one();
            }
            Ok(response)
        }

        fn interpret_question(
            &self,
            _request: &model::InterpretationRequest,
        ) -> Result<model::InterpretationResponse> {
            unreachable!()
        }

        fn model_name(&self) -> &str {
            "out-of-order fixture"
        }
    }

    #[test]
    fn parallel_batches_merge_in_source_order_after_out_of_order_completion() {
        let text = "The code index indexes Rust.\nThe code index indexes TypeScript.\n";
        let passages = vec![
            SourcePassage {
                id: "sv1:p0".into(),
                source_version_id: 1,
                heading_path: String::new(),
                start_line: 1,
                end_line: 1,
                text: "The code index indexes Rust.\n".into(),
            },
            SourcePassage {
                id: "sv1:p1".into(),
                source_version_id: 1,
                heading_path: String::new(),
                start_line: 2,
                end_line: 2,
                text: "The code index indexes TypeScript.\n".into(),
            },
        ];
        let source = SelectedSource {
            path: "README.md".into(),
            bytes: text.as_bytes().to_vec(),
            text: text.into(),
        };
        let adapter = OutOfOrderFixture {
            inner: BatchedFixture {
                requests: Mutex::new(Vec::new()),
            },
            second_done: (Mutex::new(false), Condvar::new()),
            completed: Mutex::new(Vec::new()),
        };
        let result = extract_source_in_batches_with_concurrency(&adapter, &source, &passages, 2)
            .unwrap();
        assert_eq!(*adapter.completed.lock().unwrap(), ["sv1:p1", "sv1:p0"]);
        assert_eq!(result.claims[0].evidence[0].passage_id, "sv1:p0");
        assert_eq!(result.claims[1].evidence[0].passage_id, "sv1:p1");
        assert_eq!(result.enumerations[0].member_claim_indexes, [0]);
        assert_eq!(result.enumerations[1].member_claim_indexes, [1]);
    }

    #[test]
    fn oversized_heading_section_splits_on_exact_source_lines() {
        let text = format!(
            "# Search\r\n{}{}{}{}{}",
            "Rust searchable.\r\n",
            "TypeScript searchable.\n",
            "JavaScript searchable.\n",
            "🦀 Rust parser.\n",
            "Last line without newline"
        );
        let passages = split_passages(&text, Some(55)).unwrap();
        assert!(passages.len() > 1);
        assert_eq!(
            passages.iter().map(|(_, _, _, part)| part.as_str()).collect::<String>(),
            text
        );
        for (index, (heading, start, end, part)) in passages.iter().enumerate() {
            assert_eq!(heading, "Search");
            assert!(heading.len() + part.len() <= 55);
            assert_eq!(snapshot_lines(&text, *start, *end).as_deref(), Some(part.as_str()));
            if let Some((_, _, previous_end, _)) = index.checked_sub(1).and_then(|i| passages.get(i)) {
                assert_eq!(*start, previous_end + 1);
            }
        }
        assert_eq!(passages[0].1, 1);
        assert_eq!(passages.last().unwrap().2, 6);
    }

    #[test]
    fn oversized_single_line_reports_citation_preserving_error() {
        let text = format!("# Search\n{}\n", "x".repeat(60));
        let error = split_passages(&text, Some(50)).unwrap_err().to_string();
        assert!(error.contains("line 2"));
        assert!(error.contains("split the source line"));
    }

    #[test]
    fn valid_claim_preserves_original_wording_and_evidence() {
        let (snapshot, passages, response) = fixture();
        let checked =
            validate_extraction("README.md", &snapshot, &passages, response.clone()).unwrap();
        assert_eq!(checked, response);
        assert_eq!(
            checked.claims[0].evidence[0].text,
            "The code index indexes Rust.\n"
        );
    }

    #[test]
    fn reanchors_claim_and_enumeration_to_unique_lines_in_selected_passage() {
        let snapshot = "# Search\nThe code index indexes Rust.\nSupported languages: Rust.\n".to_owned();
        let (_, mut passages, mut response) = fixture();
        passages[0].end_line = 3;
        passages[0].text = snapshot.clone();
        response.claims[0].evidence[0].start_line = 1;
        response.claims[0].evidence[0].end_line = 1;
        response.enumerations.push(model::ExtractedEnumeration {
            subject: response.claims[0].subject.clone(),
            predicate: "indexes_language".into(),
            scope: ClaimScope::Project,
            exhaustive: false,
            evidence: ClaimEvidence {
                passage_id: passages[0].id.clone(),
                start_line: 1,
                end_line: 1,
                text: "Supported languages: Rust.".into(),
            },
            member_claim_indexes: vec![0],
        });

        let checked = validate_extraction("README.md", &snapshot, &passages, response).unwrap();
        assert_eq!((checked.claims[0].evidence[0].start_line, checked.claims[0].evidence[0].end_line), (2, 2));
        assert_eq!(checked.claims[0].evidence[0].text, "The code index indexes Rust.\n");
        assert_eq!((checked.enumerations[0].evidence.start_line, checked.enumerations[0].evidence.end_line), (3, 3));
        assert_eq!(checked.enumerations[0].evidence.text, "Supported languages: Rust.\n");
    }

    /// Regression test for a real graph build failure against
    /// `epics/009-typescript-tsx-code-indexer.md`: a multi-line claim quoted
    /// three whole paragraph lines verbatim but declared `end_line` one line
    /// short. The correct three-line span is followed by a blank Markdown
    /// paragraph-separator line before unrelated prose. Word-sequence
    /// matching treats that blank line as contributing no words, so before
    /// the fix `unique_evidence_span_in_passage` found the same wording
    /// "twice" (with and without the trailing/leading blank line folded in)
    /// and wrongly bailed out on ambiguity instead of re-anchoring.
    #[test]
    fn reanchors_multiline_claim_across_a_trailing_blank_paragraph_line() {
        let snapshot = "# Introduction\n\nExtend the existing Rust code indexer so Git-tracked TypeScript (`.ts`) and\nTypeScript JSX (`.tsx`) files become inspectable code evidence in the same\nSQLite/FTS5/vector index and unified code-search experience.\n\nThis is a language backend for the existing code indexer, not a separate\ndatabase.\n".to_owned();
        let passage = SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: "Introduction".into(),
            start_line: 1,
            end_line: 8,
            text: snapshot.clone(),
        };
        let (_, _, mut response) = fixture();
        response.claims[0].evidence[0] = ClaimEvidence {
            passage_id: passage.id.clone(),
            // The model declared end_line one short of the quoted text's
            // real extent (5-6 instead of 5-7).
            start_line: 5,
            end_line: 6,
            text: "Extend the existing Rust code indexer so Git-tracked TypeScript (`.ts`) and\nTypeScript JSX (`.tsx`) files become inspectable code evidence in the same\nSQLite/FTS5/vector index and unified code-search experience.\n".into(),
        };
        let checked =
            validate_extraction("epics/009.md", &snapshot, &[passage], response).unwrap();
        let evidence = &checked.claims[0].evidence[0];
        assert_eq!((evidence.start_line, evidence.end_line), (3, 5));
        assert_eq!(
            evidence.text,
            "Extend the existing Rust code indexer so Git-tracked TypeScript (`.ts`) and\nTypeScript JSX (`.tsx`) files become inspectable code evidence in the same\nSQLite/FTS5/vector index and unified code-search experience.\n"
        );
    }

    #[test]
    fn unique_evidence_span_ignores_blank_line_padding_but_still_rejects_real_ambiguity() {
        let passage = SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: String::new(),
            start_line: 1,
            end_line: 4,
            text: "\nReal content line.\n\nMore trailing text.\n".into(),
        };
        // Matches uniquely once blank-line padding is no longer treated as a
        // distinct candidate span.
        assert_eq!(
            unique_evidence_span_in_passage("Real content line.\n", &passage),
            Some((2, 2, "Real content line.\n".to_owned()))
        );

        // Genuine repetition of the same wording in two different
        // non-blank locations must still be rejected as ambiguous.
        let repeated_passage = SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: String::new(),
            start_line: 1,
            end_line: 2,
            text: "Repeated line.\nRepeated line.\n".into(),
        };
        assert_eq!(
            unique_evidence_span_in_passage("Repeated line.\n", &repeated_passage),
            None
        );
    }

    #[test]
    fn reanchoring_rejects_ambiguous_or_invented_wording() {
        let (_, mut passages, mut response) = fixture();
        let repeated = format!("# Search\n{0}{0}", response.claims[0].evidence[0].text);
        passages[0].end_line = 3;
        passages[0].text = repeated.clone();
        response.claims[0].evidence[0].start_line = 1;
        response.claims[0].evidence[0].end_line = 1;
        let error = validate_extraction("README.md", &repeated, &passages, response.clone())
            .unwrap_err().to_string();
        assert!(error.contains("evidence text does not match"));

        response.claims[0].evidence[0].text = "The code index indexes Python.".into();
        let error = validate_extraction("README.md", &repeated, &passages, response)
            .unwrap_err().to_string();
        assert!(error.contains("evidence text does not match"));
        assert!(unique_evidence_span_in_passage("fabricated", &passages[0]).is_none());
    }

    /// Regression test for a real graph build failure against
    /// `epics/009-typescript-tsx-code-indexer.md` line 15, which reads
    /// "Epic 004 established the Rust pipeline. Epic 008 established the
    /// evidence," — two sentences on one physical Markdown line. The model
    /// correctly identified this exact line (`start_line == end_line`) but
    /// quoted only the leading sentence, dropping the trailing
    /// " Epic 008 established the evidence," text. This is a genuine
    /// word-sequence prefix, not a wording error, so it must be
    /// auto-extended to the full line without a model correction round.
    #[test]
    fn auto_extends_single_line_prefix_quote_to_the_full_declared_line() {
        let snapshot =
            "Epic 004 established the Rust pipeline. Epic 008 established the evidence,\n"
                .to_owned();
        let passage = SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: String::new(),
            start_line: 1,
            end_line: 1,
            text: snapshot.clone(),
        };
        let (_, _, mut response) = fixture();
        response.claims[0].evidence[0] = ClaimEvidence {
            passage_id: passage.id.clone(),
            start_line: 1,
            end_line: 1,
            text: "Epic 004 established the Rust pipeline.".into(),
        };
        let checked =
            validate_extraction("epics/009.md", &snapshot, &[passage], response).unwrap();
        let evidence = &checked.claims[0].evidence[0];
        assert_eq!((evidence.start_line, evidence.end_line), (1, 1));
        assert_eq!(evidence.text, snapshot);
    }

    /// Same shape, but the truncated line is the last line of an
    /// otherwise-exact multi-line declared range: the earlier lines match
    /// verbatim and only the final line is a partial quote. One mechanism
    /// (`extend_prefix_evidence_to_full_lines`) must cover both shapes.
    #[test]
    fn auto_extends_prefix_quote_on_the_last_line_of_a_multiline_range() {
        let snapshot = "First full line.\nEpic 004 established the Rust pipeline. Epic 008 established the evidence,\n".to_owned();
        let passage = SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: String::new(),
            start_line: 1,
            end_line: 2,
            text: snapshot.clone(),
        };
        let (_, _, mut response) = fixture();
        response.claims[0].evidence[0] = ClaimEvidence {
            passage_id: passage.id.clone(),
            start_line: 1,
            end_line: 2,
            text: "First full line.\nEpic 004 established the Rust pipeline.".into(),
        };
        let checked =
            validate_extraction("epics/009.md", &snapshot, &[passage], response).unwrap();
        let evidence = &checked.claims[0].evidence[0];
        assert_eq!((evidence.start_line, evidence.end_line), (1, 2));
        assert_eq!(evidence.text, snapshot);
    }

    /// Negative case: evidence that differs in wording, not just a
    /// truncation point, must still fail validation exactly as before —
    /// `extend_prefix_evidence_to_full_lines` must not become a backdoor for
    /// accepting invented text.
    #[test]
    fn prefix_extension_does_not_forgive_genuinely_wrong_wording() {
        let snapshot =
            "Epic 004 established the Rust pipeline. Epic 008 established the evidence,\n"
                .to_owned();
        let passage = SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: String::new(),
            start_line: 1,
            end_line: 1,
            text: snapshot.clone(),
        };
        let (_, _, mut response) = fixture();
        response.claims[0].evidence[0] = ClaimEvidence {
            passage_id: passage.id.clone(),
            start_line: 1,
            end_line: 1,
            // Differs in wording ("Python" instead of "Rust"), not just a
            // truncation point.
            text: "Epic 004 established the Python pipeline.".into(),
        };
        let error = validate_extraction("epics/009.md", &snapshot, &[passage], response)
            .unwrap_err()
            .to_string();
        assert!(error.contains("evidence text does not match"));
        assert!(extend_prefix_evidence_to_full_lines(
            "Epic 004 established the Python pipeline.",
            &snapshot
        )
        .is_none());
    }

    #[test]
    fn extend_prefix_evidence_to_full_lines_only_accepts_a_genuine_proper_prefix() {
        let expected = "Epic 004 established the Rust pipeline. Epic 008 established the evidence,\n";
        // Genuine proper prefix: extend.
        assert_eq!(
            extend_prefix_evidence_to_full_lines("Epic 004 established the Rust pipeline.", expected),
            Some(expected.to_owned())
        );
        // Exact match is handled by evidence_text_matches_snapshot, not this
        // function: it must return None so the caller doesn't double-handle it.
        assert_eq!(extend_prefix_evidence_to_full_lines(expected, expected), None);
        // Extra words beyond expected: not a prefix.
        assert_eq!(
            extend_prefix_evidence_to_full_lines(
                "Epic 004 established the Rust pipeline. Extra invented words.",
                expected
            ),
            None
        );
        // Differing wording, not just a truncation point: must not be forgiven.
        assert_eq!(
            extend_prefix_evidence_to_full_lines("Epic 004 established the Python pipeline.", expected),
            None
        );
        // Empty proposed text: never a prefix.
        assert_eq!(extend_prefix_evidence_to_full_lines("   ", expected), None);
    }

    /// A genuine, unfixable evidence mismatch (a dropped Markdown list marker
    /// and indent, not just wrong line numbers) must report the exact
    /// expected snapshot text alongside what the model submitted, so that
    /// the one-shot correction in `extraction_correction_contract`
    /// (`model.rs`) can echo the literal diff back to the model instead of
    /// asking it to re-derive the citation from memory.
    #[test]
    fn evidence_mismatch_error_echoes_expected_snapshot_text_for_correction() {
        let snapshot = "# Search\n- Index generated output under\n  `node_modules` in the first version.\n".to_owned();
        let passage = SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: "Search".into(),
            start_line: 1,
            end_line: 3,
            text: snapshot.clone(),
        };
        let (_, _, mut response) = fixture();
        response.claims[0].evidence[0] = ClaimEvidence {
            passage_id: passage.id.clone(),
            start_line: 2,
            end_line: 3,
            // Model dropped the leading "- " bullet marker and the
            // continuation indent, so this cannot re-anchor: the words
            // genuinely differ from any span in the passage.
            text: "Index generated output under\nnode_modules in the first version.".into(),
        };
        let error = validate_extraction("epics/x.md", &snapshot, &[passage], response)
            .unwrap_err()
            .to_string();
        assert!(error.contains("evidence text does not match snapshot lines 2-3"));
        assert!(error.contains("expected"));
        assert!(error.contains("- Index generated output under"));
        assert!(error.contains("got"));
        assert!(error.contains("Index generated output under\\nnode_modules"));
    }

    #[test]
    fn normalizes_status_literal_and_restores_markdown_line_ending() {
        let snapshot = "**Status:** ✅ Completed  \n".to_owned();
        let passage = SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: "Task 1".into(),
            start_line: 1,
            end_line: 1,
            text: snapshot.clone(),
        };
        let (_, _, mut response) = fixture();
        response.claims[0].subject = ClaimEntity {
            kind: "work_item".into(),
            name: "Task 1".into(),
            aliases: vec![],
        };
        response.claims[0].predicate = "work_item_status".into();
        response.claims[0].object = ClaimObject::Literal {
            value: " Completed ".into(),
            value_kind: "string".into(),
        };
        response.claims[0].status = ClaimStatus::Completed;
        response.claims[0].scope = ClaimScope::Task;
        response.claims[0].evidence[0].text = "**Status:** ✅ Completed".into();
        response.claims[0].evidence[0].start_line = 1;
        response.claims[0].evidence[0].end_line = 1;

        let checked = validate_extraction("epics/001.md", &snapshot, &[passage], response).unwrap();
        assert_eq!(
            checked.claims[0].object,
            ClaimObject::Literal {
                value: "completed".into(),
                value_kind: "status".into(),
            }
        );
        assert_eq!(checked.claims[0].evidence[0].text, snapshot);
    }

    #[test]
    fn typed_index_literals_become_canonical_entities() {
        for (predicate, value_kind, value, canonical_name) in [
            ("indexes_extension", "extension", "RS", ".rs"),
            ("indexes_language", "language", "rust", "Rust"),
            ("indexes_dialect", "dialect", "jsx", "JSX"),
        ] {
            let (snapshot, passages, mut response) = fixture();
            response.claims[0].predicate = predicate.into();
            response.claims[0].object = ClaimObject::Literal {
                value: value.into(),
                value_kind: value_kind.into(),
            };
            let checked = validate_extraction("README.md", &snapshot, &passages, response).unwrap();
            let ClaimObject::Entity(entity) = &checked.claims[0].object else {
                panic!("{predicate} should have an entity object");
            };
            assert_eq!(entity.kind, value_kind);
            assert_eq!(entity.name, canonical_name);
            assert_eq!(entity.aliases, [value]);
        }
    }

    #[test]
    fn canonical_dialect_changes_language_predicate_and_enumeration() {
        let (snapshot, passages, mut response) = fixture();
        response.claims[0].object = ClaimObject::Entity(ClaimEntity {
            kind: "language".into(),
            name: "JSX".into(),
            aliases: vec![".jsx".into()],
        });
        response.enumerations.push(model::ExtractedEnumeration {
            subject: response.claims[0].subject.clone(),
            predicate: "indexes_language".into(),
            scope: ClaimScope::Project,
            exhaustive: true,
            evidence: response.claims[0].evidence[0].clone(),
            member_claim_indexes: vec![0],
        });
        let checked = validate_extraction("README.md", &snapshot, &passages, response).unwrap();
        assert_eq!(checked.claims[0].predicate, "indexes_dialect");
        assert!(matches!(&checked.claims[0].object, ClaimObject::Entity(entity) if entity.kind == "dialect" && entity.name == "JSX"));
        assert_eq!(checked.enumerations[0].predicate, "indexes_dialect");
        assert!(!checked.enumerations[0].exhaustive);
    }

    #[test]
    fn unsupported_enumeration_is_omitted_without_losing_valid_claim() {
        let (snapshot, passages, mut response) = fixture();
        response.enumerations.push(model::ExtractedEnumeration {
            subject: response.claims[0].subject.clone(),
            predicate: "excludes_path".into(),
            scope: ClaimScope::Project,
            exhaustive: true,
            evidence: response.claims[0].evidence[0].clone(),
            member_claim_indexes: vec![0],
        });
        let checked = validate_extraction("README.md", &snapshot, &passages, response).unwrap();
        assert_eq!(checked.claims.len(), 1);
        assert_eq!(checked.claims[0].predicate, "indexes_language");
        assert!(checked.enumerations.is_empty());
    }

    #[test]
    fn invalid_enumerations_are_omitted_without_losing_valid_claims() {
        let (snapshot, passages, mut response) = fixture();
        let enumeration = model::ExtractedEnumeration {
            subject: response.claims[0].subject.clone(),
            predicate: "indexes_language".into(),
            scope: ClaimScope::Project,
            exhaustive: true,
            evidence: response.claims[0].evidence[0].clone(),
            member_claim_indexes: vec![0],
        };
        response.enumerations.push(enumeration.clone());
        response.enumerations[0].member_claim_indexes = vec![99];
        let mut invalid_citation = enumeration.clone();
        invalid_citation.evidence.passage_id = "sv1:p99".into();
        response.enumerations.push(invalid_citation);
        let mut mismatched_text = enumeration;
        mismatched_text.evidence.text = "Invented list".into();
        response.enumerations.push(mismatched_text);

        let checked = validate_extraction("README.md", &snapshot, &passages, response).unwrap();
        assert_eq!(checked.claims.len(), 1);
        assert!(checked.enumerations.is_empty());
    }

    #[test]
    fn invalid_enumeration_does_not_relax_claim_citation_validation() {
        let (snapshot, passages, mut response) = fixture();
        response.enumerations.push(model::ExtractedEnumeration {
            subject: response.claims[0].subject.clone(),
            predicate: "indexes_language".into(),
            scope: ClaimScope::Project,
            exhaustive: true,
            evidence: response.claims[0].evidence[0].clone(),
            member_claim_indexes: vec![99],
        });
        response.claims[0].evidence[0].text = "Invented claim evidence".into();

        let error = validate_extraction("README.md", &snapshot, &passages, response)
            .unwrap_err()
            .to_string();
        assert!(error.contains("claim 1: evidence text does not match snapshot lines 2-2"));
    }

    #[test]
    fn rejects_untyped_and_malformed_index_literals() {
        for (value, value_kind, error_text) in [
            (".rs", "string", "invalid literal type"),
            ("src/main.rs", "extension", "invalid extension literal"),
            (".rs.md", "extension", "invalid extension literal"),
        ] {
            let (snapshot, passages, mut response) = fixture();
            response.claims[0].predicate = "indexes_extension".into();
            response.claims[0].object = ClaimObject::Literal {
                value: value.into(),
                value_kind: value_kind.into(),
            };
            let error = validate_extraction("README.md", &snapshot, &passages, response)
                .unwrap_err()
                .to_string();
            assert!(error.contains(error_text), "{error}");
        }
    }

    #[test]
    fn rejects_unsupported_string_status_and_out_of_range_evidence() {
        let (snapshot, passages, mut response) = fixture();
        response.claims[0].subject.kind = "work_item".into();
        response.claims[0].predicate = "work_item_status".into();
        response.claims[0].object = ClaimObject::Literal {
            value: "almost completed".into(),
            value_kind: "string".into(),
        };
        assert!(
            validate_extraction("README.md", &snapshot, &passages, response)
                .unwrap_err()
                .to_string()
                .contains("invalid literal type")
        );

        let (snapshot, passages, mut response) = fixture();
        response.claims[0].evidence[0].end_line = 3;
        assert!(
            validate_extraction("README.md", &snapshot, &passages, response)
                .unwrap_err()
                .to_string()
                .contains("outside passage")
        );
    }

    #[test]
    fn rejects_unknown_predicate_and_bad_types_with_source_context() {
        let (snapshot, passages, mut response) = fixture();
        response.claims[0].predicate = "invented_relation".into();
        let error = validate_extraction("docs/guide.md", &snapshot, &passages, response)
            .unwrap_err()
            .to_string();
        assert!(error.contains("docs/guide.md") && error.contains("unknown predicate"));

        let (snapshot, passages, mut response) = fixture();
        response.claims[0].subject.kind = "language".into();
        let error = validate_extraction("docs/guide.md", &snapshot, &passages, response)
            .unwrap_err()
            .to_string();
        assert!(error.contains("docs/guide.md") && error.contains("does not allow"));
    }

    #[test]
    fn rejects_missing_unknown_or_fabricated_evidence() {
        let (snapshot, passages, mut response) = fixture();
        response.claims[0].evidence.clear();
        assert!(
            validate_extraction("README.md", &snapshot, &passages, response)
                .unwrap_err()
                .to_string()
                .contains("README.md")
        );

        let (snapshot, passages, mut response) = fixture();
        response.claims[0].evidence[0].passage_id = "sv1:p99".into();
        assert!(
            validate_extraction("README.md", &snapshot, &passages, response)
                .unwrap_err()
                .to_string()
                .contains("unselected passage")
        );

        let (snapshot, passages, mut response) = fixture();
        response.claims[0].evidence[0].text = "Invented citation".into();
        let error = validate_extraction("README.md", &snapshot, &passages, response)
            .unwrap_err()
            .to_string();
        assert!(error.contains("claim 1: evidence text does not match snapshot lines 2-2"));

        let (snapshot, passages, mut response) = fixture();
        response.claims[0].evidence[0].text = "The code index indexes Python.\n".into();
        let error = validate_extraction("README.md", &snapshot, &passages, response)
            .unwrap_err()
            .to_string();
        assert!(error.contains("claim 1: evidence text does not match snapshot lines 2-2"));
    }

    #[test]
    fn restores_exact_snapshot_text_after_whitespace_only_change() {
        let (snapshot, passages, mut response) = fixture();
        response.claims[0].evidence[0].text = " The code index\tindexes Rust. ".into();
        let checked = validate_extraction("README.md", &snapshot, &passages, response).unwrap();
        assert_eq!(checked.claims[0].evidence[0].text, "The code index indexes Rust.\n");
    }

    #[test]
    fn rejects_passage_that_does_not_match_immutable_snapshot() {
        let (snapshot, mut passages, response) = fixture();
        passages[0].text = "different text".into();
        let error = validate_extraction("README.md", &snapshot, &passages, response)
            .unwrap_err()
            .to_string();
        assert!(error.contains("README.md") && error.contains("does not match its snapshot"));
    }

    #[test]
    fn incomplete_exhaustive_list_is_left_open() {
        use super::model::{ClaimScope, ExtractedEnumeration};
        let (_, _, mut response) = fixture();
        let snapshot =
            "All supported code-search languages: Rust, TypeScript, JavaScript.\n".to_owned();
        let passages = vec![SourcePassage {
            id: "sv1:p0".into(),
            source_version_id: 1,
            heading_path: String::new(),
            start_line: 1,
            end_line: 1,
            text: snapshot.clone(),
        }];
        response.claims[0].evidence = vec![model::ClaimEvidence {
            passage_id: "sv1:p0".into(),
            start_line: 1,
            end_line: 1,
            text: snapshot.clone(),
        }];
        let mut typescript = response.claims[0].clone();
        typescript.object = ClaimObject::Entity(model::ClaimEntity {
            kind: "language".into(),
            name: "TypeScript".into(),
            aliases: Vec::new(),
        });
        response.claims.push(typescript);
        response.enumerations.push(ExtractedEnumeration {
            subject: response.claims[0].subject.clone(),
            predicate: "indexes_language".into(),
            scope: ClaimScope::Project,
            exhaustive: true,
            evidence: response.claims[0].evidence[0].clone(),
            member_claim_indexes: vec![0, 1],
        });
        response.enumerations[0].evidence.text = snapshot.trim_end().into();
        let checked =
            validate_extraction("README.md", &snapshot, &passages, response.clone()).unwrap();
        assert!(!checked.enumerations[0].exhaustive);
        assert_eq!(checked.enumerations[0].evidence.text, snapshot);
        response.enumerations[0].evidence.text = "Paraphrased list".into();
        let checked = validate_extraction("README.md", &snapshot, &passages, response.clone())
            .unwrap();
        assert!(checked.enumerations.is_empty());
        response.enumerations[0].evidence.text = snapshot.clone();
        response.enumerations[0].evidence.passage_id = "sv1:p99".into();
        let checked = validate_extraction("README.md", &snapshot, &passages, response.clone())
            .unwrap();
        assert!(checked.enumerations.is_empty());
        response.enumerations[0].evidence.passage_id = "sv1:p0".into();
        response.claims[1].status = ClaimStatus::Planned;
        let checked = validate_extraction("README.md", &snapshot, &passages, response).unwrap();
        assert!(checked.enumerations.is_empty());
    }

    #[test]
    fn readme_closed_language_and_dialect_lists_are_parsed() {
        let text = "- All supported code-search languages are Git-tracked Rust (`.rs`), TypeScript (`.ts`), and JavaScript (`.js`), and all supported dialects are TSX (`.tsx`) and JSX (`.jsx`); they share one code index\n";
        assert_eq!(stated_list_members(text, "language").unwrap().len(), 3);
        assert_eq!(stated_list_members(text, "dialect").unwrap().len(), 2);
        assert!(stated_list_members(
            "Supported languages include Rust and TypeScript, such as those in this guide.",
            "language"
        )
        .is_none());
    }
}

/// Persist the selected source set for a build and return the exact passages
/// available to extraction. Existing versions are reused by (source, hash).
/// Hashing is over the original working-tree bytes, before UTF-8 decoding.
pub fn snapshot_sources(
    db: &Connection,
    build_id: &str,
    repo_path: &Path,
    sources: &[SelectedSource],
    extraction_version: &str,
) -> Result<Vec<SnapshottedSource>> {
    snapshot_sources_with_limit(db, build_id, repo_path, sources, extraction_version, None)
}

fn snapshot_sources_with_limit(
    db: &Connection,
    build_id: &str,
    repo_path: &Path,
    sources: &[SelectedSource],
    extraction_version: &str,
    max_chars: Option<usize>,
) -> Result<Vec<SnapshottedSource>> {
    let head = git_head(repo_path);
    let tx = db.unchecked_transaction()?;
    if let Some(head) = &head {
        tx.execute(
            "UPDATE graph_builds SET git_head = ?1 WHERE id = ?2",
            rusqlite::params![head, build_id],
        )?;
    }
    let mut output = Vec::with_capacity(sources.len());
    for source in sources {
        let content_hash = format!("{:016x}", xxhash_rust::xxh3::xxh3_64(&source.bytes));
        tx.execute(
            "INSERT INTO graph_sources(path, human_maintained, active, latest_content_hash)
             VALUES (?1, 1, 0, ?2)
             ON CONFLICT(path) DO UPDATE SET human_maintained = 1",
            rusqlite::params![source.path, content_hash],
        )?;
        let source_id: i64 = tx.query_row(
            "SELECT id FROM graph_sources WHERE path = ?1",
            [&source.path],
            |row| row.get(0),
        )?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM graph_source_versions WHERE source_id = ?1 AND content_hash = ?2",
                rusqlite::params![source_id, content_hash],
                |row| row.get(0),
            )
            .optional()?;
        let source_version_id = match existing {
            Some(id) => id,
            None => {
                tx.execute(
                    "INSERT INTO graph_source_versions(source_id, content_hash, git_head, snapshot, extraction_version, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![source_id, content_hash, head, source.text, extraction_version, chrono::Utc::now().to_rfc3339()],
                )?;
                tx.last_insert_rowid()
            }
        };
        let version_head: Option<String> = tx.query_row(
            "SELECT git_head FROM graph_source_versions WHERE id = ?1",
            [source_version_id],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO graph_build_sources(build_id, source_id, source_version_id) VALUES (?1, ?2, ?3)
             ON CONFLICT(build_id, source_id) DO UPDATE SET source_version_id = excluded.source_version_id",
            rusqlite::params![build_id, source_id, source_version_id],
        )?;
        let passage_specs = split_passages(&source.text, max_chars)
            .map_err(|error| anyhow::anyhow!("source {}: {error:#}", source.path))?;
        let mut passages = Vec::with_capacity(passage_specs.len());
        for (heading_path, start_line, end_line, text) in passage_specs {
            // Line-range IDs let a reused source version retain older passage
            // spans and claim evidence when a later extractor uses a new limit.
            let passage_id = format!("sv{source_version_id}:l{start_line}-{end_line}");
            tx.execute(
                "INSERT OR IGNORE INTO graph_spans(id, source_version_id, heading_path, start_line, end_line, text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![passage_id, source_version_id, heading_path, start_line, end_line, text],
            )?;
            passages.push(SourcePassage {
                id: passage_id,
                source_version_id,
                heading_path,
                start_line,
                end_line,
                text,
            });
        }
        output.push(SnapshottedSource {
            path: source.path.clone(),
            content_hash,
            source_version_id,
            git_head: version_head,
            passages,
        });
    }
    tx.commit()?;
    Ok(output)
}

fn git_head(repo_path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!head.is_empty()).then_some(head)
}

/// Split text at Markdown headings. Each passage is a contiguous, exact slice
/// of the stored snapshot, with line numbers starting at one. Heading levels
/// form a compact breadcrumb for each section.
fn split_passages(
    text: &str,
    max_chars: Option<usize>,
) -> Result<Vec<(String, u32, u32, String)>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let mut headings: Vec<(usize, usize, String)> = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes)
            && (trimmed[hashes..].starts_with(' ') || trimmed[hashes..].starts_with('\t'))
        {
            let title = trimmed[hashes..].trim().to_owned();
            while stack.last().is_some_and(|(level, _)| *level >= hashes) {
                stack.pop();
            }
            stack.push((hashes, title));
            headings.push((
                index,
                hashes,
                stack
                    .iter()
                    .map(|(_, title)| title.as_str())
                    .collect::<Vec<_>>()
                    .join(" > "),
            ));
        }
    }
    let mut starts = Vec::new();
    if headings.first().is_none_or(|(index, _, _)| *index > 0) {
        starts.push((0usize, String::new()));
    }
    starts.extend(headings.into_iter().map(|(index, _, path)| (index, path)));
    if starts.is_empty() {
        starts.push((0, String::new()));
    }
    let sections = starts
        .iter()
        .enumerate()
        .filter_map(|(i, (start, heading))| {
            let end = starts
                .get(i + 1)
                .map(|(next, _)| *next)
                .unwrap_or(lines.len());
            let byte_start: usize = lines.iter().take(*start).map(|l| l.len()).sum();
            let byte_end: usize = lines.iter().take(end).map(|l| l.len()).sum();
            let passage = text.get(byte_start..byte_end)?.to_owned();
            Some((heading.clone(), (start + 1) as u32, end as u32, passage))
        })
        .collect::<Vec<_>>();
    let mut passages = Vec::new();
    for (heading, start, end, section) in sections {
        let limit = max_chars.unwrap_or(usize::MAX);
        let line_budget = limit.saturating_sub(heading.len());
        anyhow::ensure!(
            line_budget > 0,
            "heading at line {start} exceeds the extraction passage limit of {limit} bytes"
        );
        let mut chunk = String::new();
        let mut chunk_start = start;
        let mut chunk_end = start;
        for (offset, line) in section.split_inclusive('\n').enumerate() {
            let line_number = start + offset as u32;
            anyhow::ensure!(
                line.len() <= line_budget,
                "line {line_number} is {} bytes, exceeding the extraction passage limit of {limit} bytes after its heading; split the source line to retain exact line citations",
                line.len()
            );
            if !chunk.is_empty() && chunk.len() + line.len() > line_budget {
                passages.push((heading.clone(), chunk_start, chunk_end, std::mem::take(&mut chunk)));
                chunk_start = line_number;
            }
            chunk.push_str(line);
            chunk_end = line_number;
        }
        if !chunk.is_empty() {
            debug_assert_eq!(chunk_end, end);
            passages.push((heading, chunk_start, chunk_end, chunk));
        }
    }
    Ok(passages)
}

/// Expand repository-relative file paths and globs, then validate and read all
/// unique matches. Explicitly selected untracked files are accepted because
/// selection is based on the working tree, not Git's tracked-file list.
pub fn resolve_sources(repo_path: &Path, specifications: &[String]) -> Result<Vec<SelectedSource>> {
    anyhow::ensure!(
        !specifications.is_empty(),
        "at least one --source path or glob is required"
    );
    let repo_root = fs::canonicalize(repo_path).map_err(|error| {
        anyhow::anyhow!(
            "cannot resolve repository root {}: {error}",
            repo_path.display()
        )
    })?;
    anyhow::ensure!(
        repo_root.is_dir(),
        "repository root is not a directory: {}",
        repo_root.display()
    );

    let mut matches = BTreeMap::<String, PathBuf>::new();
    for specification in specifications {
        let relative = Path::new(specification);
        anyhow::ensure!(
            !relative.is_absolute()
                && relative.components().all(|component| matches!(component, Component::Normal(_) | Component::CurDir)),
            "source must be a repository-relative path or glob without '..' or a root: {specification}"
        );
        let pattern = repo_root.join(relative);
        let pattern = pattern
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("source path is not valid UTF-8: {specification}"))?;
        let paths = glob::glob(pattern)
            .map_err(|error| anyhow::anyhow!("invalid source glob {specification:?}: {error}"))?;
        let mut count = 0usize;
        for item in paths {
            let matched = item.map_err(|error| {
                anyhow::anyhow!("could not expand source glob {specification:?}: {error}")
            })?;
            count += 1;
            let canonical = fs::canonicalize(&matched).map_err(|error| {
                anyhow::anyhow!(
                    "selected source {} does not exist or cannot be resolved: {error}",
                    matched.display()
                )
            })?;
            let relative = canonical.strip_prefix(&repo_root).map_err(|_| {
                anyhow::anyhow!(
                    "selected source {} escapes repository root {} (possibly through a symlink)",
                    matched.display(),
                    repo_root.display()
                )
            })?;
            let relative_string = relative
                .to_str()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "selected source path is not valid UTF-8: {}",
                        relative.display()
                    )
                })?
                .replace('\\', "/");
            matches.insert(relative_string, canonical);
        }
        anyhow::ensure!(
            count > 0,
            "source pattern {specification:?} matched no files"
        );
    }

    let mut sources = Vec::with_capacity(matches.len());
    for (relative_path, canonical) in matches {
        anyhow::ensure!(
            canonical.is_file(),
            "selected source is not a regular file: {relative_path}"
        );
        let extension = canonical
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        anyhow::ensure!(
            matches!(extension.as_str(), "md" | "markdown" | "txt" | "text"),
            "selected source {relative_path} is not a Markdown or plain-text file (.md, .markdown, .txt, or .text)"
        );
        let bytes = fs::read(&canonical).map_err(|error| {
            anyhow::anyhow!("cannot read selected source {relative_path}: {error}")
        })?;
        let text = String::from_utf8(bytes.clone()).map_err(|error| {
            anyhow::anyhow!("selected source {relative_path} is not valid UTF-8: {error}")
        })?;
        sources.push(SelectedSource {
            path: relative_path,
            bytes,
            text,
        });
    }
    Ok(sources)
}

/// Open the repository's shared SQLite file for graph operations.
///
/// This deliberately does not construct an [`crate::Indexer`], initialize the
/// search schema, register sqlite-vec, or load an embedding model. Graph schema
/// initialization is handled separately by the graph storage layer.
pub fn open_database(repo_path: &Path, config: &IndexerConfig) -> Result<Connection> {
    let path = repository_db_path(repo_path, config)?;
    Ok(Connection::open(path)?)
}

/// Create or upgrade the namespaced provenance graph schema.
///
/// This migration only creates `graph_` objects. It does not call the regular
/// indexer's schema initializer, which also creates/updates search tables.
pub fn init_schema(db: &Connection) -> Result<()> {
    db.execute_batch("PRAGMA foreign_keys = ON;")?;
    db.execute_batch("BEGIN IMMEDIATE;")?;
    let migration = (|| -> Result<()> {
        let table_exists = |name: &str| -> Result<bool> {
            Ok(db.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [name],
                |row| row.get(0),
            )?)
        };
        let needs_claim_backfill = !table_exists("graph_build_claims")?;
        let needs_enumeration_backfill = !table_exists("graph_build_enumerations")?;
        let needs_member_backfill = !table_exists("graph_build_enumeration_members")?;
        db.execute_batch(include_str!("graph_schema.sql"))?;
        if needs_claim_backfill {
            db.execute_batch(
                "INSERT OR IGNORE INTO graph_build_claims(build_id, claim_id)
                 SELECT build_id, id FROM graph_claims;",
            )?;
        }
        if needs_enumeration_backfill {
            db.execute_batch(
                "INSERT OR IGNORE INTO graph_build_enumerations(build_id, enumeration_id, exhaustive)
                 SELECT build_id, id, exhaustive FROM graph_enumerations;",
            )?;
        }
        if needs_member_backfill {
            db.execute_batch(
                "INSERT OR IGNORE INTO graph_build_enumeration_members(build_id, enumeration_id, claim_id)
                 SELECT en.build_id, em.enumeration_id, em.claim_id
                 FROM graph_enumeration_members em
                 JOIN graph_enumerations en ON en.id=em.enumeration_id
                 JOIN graph_build_claims bc ON bc.build_id=en.build_id AND bc.claim_id=em.claim_id;",
            )?;
        }
        let version: i64 = db.query_row(
            "SELECT version FROM graph_schema_version WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        anyhow::ensure!(version == 1, "unsupported graph schema version {version}");
        Ok(())
    })();
    match migration {
        Ok(()) => {
            db.execute_batch("COMMIT;")?;
            Ok(())
        }
        Err(error) => {
            let _ = db.execute_batch("ROLLBACK;");
            Err(error)
        }
    }
}

#[cfg(test)]
#[path = "graph_acceptance_tests.rs"]
mod acceptance_tests;
