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
        let Some(target) = prepared
            .resolved_entity_keys
            .iter()
            .find(|key| key.starts_with("language:"))
        else {
            return Ok(EvidenceResolution::Unknown {
                reason: "the code-search language target is missing".into(),
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
               AND c.scope IN ('project','feature')
               AND s.active=1 AND s.human_maintained=1
               AND (json_array_length(?3)=0 OR subj.canonical_key IN (SELECT value FROM json_each(?3)))
               AND (json_array_length(?4)=0 OR obj.canonical_key IN (SELECT value FROM json_each(?4)))
             ORDER BY c.id, e.span_id",
        )?;
        let subjects = serde_json::to_string(&subject_keys)?;
        let objects = serde_json::to_string(&object_keys)?;
        for row in stmt.query_map(
            rusqlite::params![prepared.build_id, predicate, subjects, objects],
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
    let interpretation = adapter
        .interpret_question(&request)
        .map_err(|error| anyhow::anyhow!("interpreting graph search question: {error:#}"))?;
    anyhow::ensure!(
        interpretation
            .predicates
            .iter()
            .all(|p| candidate_predicates.contains(p)),
        "graph model returned a predicate outside the active graph vocabulary"
    );

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
        let requested_aliases = canonical
            .aliases
            .iter()
            .map(|a| (a.kind.as_str(), a.normalized.as_str()))
            .collect::<Vec<_>>();
        let found = candidate_entities
            .iter()
            .find(|(_, entity)| {
                let known = entity.canonicalized();
                if known.canonical_key == canonical.canonical_key {
                    return true;
                }
                known.aliases.iter().any(|known_alias| {
                    requested_aliases.iter().any(|(kind, value)| {
                        *kind == known_alias.kind && *value == known_alias.normalized
                    })
                })
            })
            .map(|(key, _)| key.clone());
        // A query may name an entity that has no node because the source set
        // never mentioned it (for example, Lua). Keep its typed canonical key
        // as the search target so an exhaustive supported-language list can
        // prove absence. It is never inserted into the graph.
        let found = found.unwrap_or(canonical.canonical_key);
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
    Ok(SearchPreparation {
        build_id,
        interpretation,
        resolved_entity_keys,
        candidates,
    })
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
        let snapshots = snapshot_sources(db, &build_id, repo_path, sources, extractor_version)?;
        let mut extractions = Vec::with_capacity(snapshots.len());
        for (source, snapshot) in sources.iter().zip(&snapshots) {
            let request = model::ExtractionRequest {
                source_path: source.path.clone(),
                passages: snapshot.passages.clone(),
            };
            let response = adapter.extract_claims(&request).map_err(|error| {
                anyhow::anyhow!("extracting claims for source {}: {error:#}", source.path)
            })?;
            let checked = match validate_extraction(
                &source.path,
                &source.text,
                &snapshot.passages,
                response,
            ) {
                Ok(checked) => checked,
                Err(initial_error) => {
                    let Some(retry) = adapter.retry_extraction(&request, &initial_error.to_string()) else {
                        return Err(initial_error);
                    };
                    let response = retry?;
                    validate_extraction(&source.path, &source.text, &snapshot.passages, response)
                        .map_err(|error| anyhow::anyhow!(
                            "{error}; model correction also failed after: {initial_error}"
                        ))?
                }
            };
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
        let object_kind = match &claim.object {
            ClaimObject::Entity(entity) => {
                validate_entity(&claim_label, "object", entity)?;
                entity.kind.as_str()
            }
            ClaimObject::Literal { value, value_kind } => {
                anyhow::ensure!(
                    !value.trim().is_empty(),
                    "{claim_label}: literal object is empty"
                );
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
        for evidence in &claim.evidence {
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
            let expected = snapshot_lines(snapshot, evidence.start_line, evidence.end_line)
                .ok_or_else(|| {
                    anyhow::anyhow!("{claim_label}: evidence range is outside snapshot")
                })?;
            anyhow::ensure!(
                expected == evidence.text,
                "{claim_label}: evidence text does not exactly match snapshot lines {}-{}",
                evidence.start_line,
                evidence.end_line
            );
        }
    }
    for (index, enumeration) in response.enumerations.iter_mut().enumerate() {
        let label = format!("source {source_path}: enumeration {}", index + 1);
        canonicalize_claim_entity(&mut enumeration.subject);
        validate_entity(&label, "subject", &enumeration.subject)?;
        anyhow::ensure!(
            matches!(
                enumeration.predicate.as_str(),
                "indexes_language" | "indexes_dialect" | "indexes_extension"
            ),
            "{label}: unsupported enumeration predicate {:?}",
            enumeration.predicate
        );
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
        let expected = snapshot_lines(
            snapshot,
            enumeration.evidence.start_line,
            enumeration.evidence.end_line,
        )
        .ok_or_else(|| anyhow::anyhow!("{label}: evidence range is outside snapshot"))?;
        anyhow::ensure!(
            expected == enumeration.evidence.text,
            "{label}: evidence text does not exactly match snapshot"
        );
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
    }
    Ok(response)
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
        evidence
            .passage_id
            .starts_with(&format!("sv{source_version_id}:p"))
            && evidence.start_line >= passage_start
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

fn validate_entity(label: &str, position: &str, entity: &model::ClaimEntity) -> Result<()> {
    const ENTITY_KINDS: &[&str] = &[
        "feature",
        "index",
        "language",
        "dialect",
        "extension",
        "parser",
        "work_item",
    ];
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
    fn rejects_missing_invented_and_mismatched_evidence() {
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
        assert!(
            validate_extraction("README.md", &snapshot, &passages, response)
                .unwrap_err()
                .to_string()
                .contains("exactly match")
        );
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
        let checked =
            validate_extraction("README.md", &snapshot, &passages, response.clone()).unwrap();
        assert!(!checked.enumerations[0].exhaustive);
        response.claims[1].status = ClaimStatus::Planned;
        assert!(
            validate_extraction("README.md", &snapshot, &passages, response)
                .unwrap_err()
                .to_string()
                .contains("members must reference matching claims")
        );
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
        let passage_specs = split_passages(&source.text);
        for (index, (heading_path, start_line, end_line, text)) in
            passage_specs.into_iter().enumerate()
        {
            let passage_id = format!("sv{source_version_id}:p{index}");
            tx.execute(
                "INSERT OR IGNORE INTO graph_spans(id, source_version_id, heading_path, start_line, end_line, text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![passage_id, source_version_id, heading_path, start_line, end_line, text],
            )?;
        }
        let mut statement = tx.prepare(
            "SELECT id, heading_path, start_line, end_line, text FROM graph_spans
             WHERE source_version_id = ?1 AND id GLOB ?2 ORDER BY start_line, id",
        )?;
        let passages = statement
            .query_map(
                rusqlite::params![source_version_id, format!("sv{source_version_id}:p[0-9]*")],
                |row| {
                    Ok(SourcePassage {
                        id: row.get(0)?,
                        source_version_id,
                        heading_path: row.get(1)?,
                        start_line: row.get(2)?,
                        end_line: row.get(3)?,
                        text: row.get(4)?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
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
fn split_passages(text: &str) -> Vec<(String, u32, u32, String)> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if lines.is_empty() {
        return Vec::new();
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
    starts
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
        .collect()
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
