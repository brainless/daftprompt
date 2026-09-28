"""Frozen reference-set loading and structural matching for Epic 014 scoring.

The frozen reference set (`frozen-inputs/reference-set.json`, recorded before
any extraction run) defines 9 supported relations (R1-R9) with verbatim
expected-evidence lines, 8 repeated entities with occurrence lines, and six
questions (Q5/Q6 expect "unknown"). This module:

- loads those inputs without modifying them;
- resolves each fact's predicate hint and kind hints against the versioned
  schema (schema gaps become the `absent_schema_label` category);
- matches scored claims to facts with deterministic tiers:

  - **name** (strong): whitespace-normalized case-sensitive equality or
    substring containment either way between the mention and the fact's
    entity name;
  - **name_weak**: token Jaccard >= 0.6 after punctuation-aware lowercase
    tokenization and stopword removal (handles "and"/punctuation variants
    like `.js, and .jsx`);
  - **kind_evidence** (accepted claims only): both endpoint kinds equal the
    schema mapping of the fact's kind hints, the citation overlaps the
    fact's expected evidence lines, and each endpoint mention shares a
    lowercase token with the fact's corresponding name (both endpoints, not
    just one: parallel facts on the same evidence line share their subject —
    e.g. R1 akar vs R2 gitoxide on line 7 — so one shared token on the
    subject alone would cross-match them).

  Tier `kind_evidence` is deliberately weaker; its matches are flagged and
  never auto-recover a fact — recovery additionally requires human-reviewed
  support == "supported".

Pure stdlib; no gliner2/torch imports.
"""
from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

from .taxonomy import (
    CATEGORY_ABSENT_SCHEMA_LABEL,
    CATEGORY_EVIDENCE_MISMATCH,
    CATEGORY_MISSING_ENTITY,
    CATEGORY_MISSING_RELATION,
    CATEGORY_UNSUPPORTED_BUT_ACCEPTED,
    CATEGORY_WRONG_DIRECTION,
    CATEGORY_WRONG_PREDICATE,
    CATEGORY_WRONG_TYPE,
    STATUS_ATTEMPTED_REJECTED,
    STATUS_MATCHED_EVIDENCE_MISMATCH,
    STATUS_MATCHED_PENDING_REVIEW,
    STATUS_MISSING_ENTITY,
    STATUS_MISSING_RELATION,
    STATUS_RECOVERED,
    STATUS_SCHEMA_GAP,
    SUPPORT_MISLEADING,
    SUPPORT_SUPPORTED,
    SUPPORT_UNREVIEWED,
    SUPPORT_WEAK,
)

TOKEN_RE = re.compile(r"[a-z0-9_.*+]+")
WEAK_JACCARD_THRESHOLD = 0.6
STOPWORDS = frozenset(
    {"a", "an", "and", "for", "in", "is", "of", "on", "or", "the", "to", "via", "+"}
)


@dataclass(frozen=True)
class Evidence:
    start_line: int
    end_line: int
    text: str


@dataclass(frozen=True)
class ReferenceFact:
    id: str
    subject_name: str
    subject_kind_hint: str
    predicate_hint: str
    object_name: str
    object_kind_hint: str
    evidence: tuple[Evidence, ...]
    verification_note: str

    @property
    def evidence_line_ranges(self) -> tuple[tuple[int, int], ...]:
        return tuple((ev.start_line, ev.end_line) for ev in self.evidence)


@dataclass(frozen=True)
class RepeatedEntity:
    name: str
    occurrence_lines: tuple[int, ...]
    note: str


@dataclass(frozen=True)
class ReferenceQuestion:
    id: str
    question: str
    expected: str  # "supported" | "unknown"
    passages: tuple[tuple[int, int], ...]
    rationale: str


def load_reference_set(path: Path) -> dict:
    """Load the frozen reference set (read-only)."""
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    facts = tuple(
        ReferenceFact(
            id=raw["id"],
            subject_name=raw["subject"]["name"],
            subject_kind_hint=raw["subject"].get("kind_hint", ""),
            predicate_hint=raw.get("predicate_hint", ""),
            object_name=raw["object"]["name"],
            object_kind_hint=raw["object"].get("kind_hint", ""),
            evidence=tuple(
                Evidence(
                    start_line=int(ev["start_line"]),
                    end_line=int(ev["end_line"]),
                    text=ev["text"],
                )
                for ev in raw.get("expected_evidence", ())
            ),
            verification_note=raw.get("verification_note", ""),
        )
        for raw in data["supported_relations"]
    )
    repeated = tuple(
        RepeatedEntity(
            name=raw["name"],
            occurrence_lines=tuple(int(n) for n in raw["occurrence_lines"]),
            note=raw.get("note", ""),
        )
        for raw in data.get("repeated_entities", ())
    )
    questions = tuple(
        ReferenceQuestion(
            id=raw["id"],
            question=raw["question"],
            expected=raw.get("expected", "supported"),
            passages=tuple(
                (int(pair[0]), int(pair[1]))
                for pair in raw.get("expected_supporting_passages", ())
            ),
            rationale=raw.get("rationale", ""),
        )
        for raw in data.get("questions", ())
    )
    return {
        "path": Path(path),
        "facts": facts,
        "repeated_entities": repeated,
        "questions": questions,
        "recorded_at": data.get("recorded_at"),
    }


def tokens(text: str) -> frozenset[str]:
    """Lowercase punctuation-aware tokens minus stopwords."""
    return frozenset(
        token for token in TOKEN_RE.findall(text.lower()) if token not in STOPWORDS
    )


def jaccard(a: frozenset[str], b: frozenset[str]) -> float:
    if not a or not b:
        return 0.0
    return len(a & b) / len(a | b)


def name_match(mention: str, name: str) -> Optional[str]:
    """Match a mention against a reference entity name.

    Returns "strong" (exact/containment after whitespace normalization,
    case-sensitive), "weak" (token Jaccard >= threshold), or None.
    """
    mention_norm = " ".join(mention.split())
    name_norm = " ".join(name.split())
    if not mention_norm or not name_norm:
        return None
    if mention_norm == name_norm or mention_norm in name_norm or name_norm in mention_norm:
        return "strong"
    if jaccard(tokens(mention), tokens(name)) >= WEAK_JACCARD_THRESHOLD:
        return "weak"
    return None


def canonical_predicate_for_hint(schema, hint: str) -> Optional[str]:
    """Resolve a reference `predicate_hint` to a declared canonical predicate.

    Longest-prefix match of the hint against declared predicate names with
    underscores read as spaces ("serves as" -> serves_as, "uses (...)" ->
    uses). No match -> the fact has no schema label (schema gap).
    """
    hint_l = " ".join(hint.lower().split())
    if not hint_l:
        return None
    names = sorted(schema.predicates, key=lambda name: -len(name))
    for name in names:
        spaced = name.replace("_", " ")
        if hint_l == spaced or hint_l.startswith(spaced + " ") or hint_l.startswith(spaced + "("):
            return name
    return None


def schema_kind_for_hint(schema, hint: str) -> Optional[str]:
    """Resolve a lowercase kind hint via the artifact's reference_kind_hints."""
    mapping = schema.data.get("reference_kind_hints") or {}
    return mapping.get(hint.strip().lower())


def fact_shape_present(schema, fact: ReferenceFact) -> tuple[bool, tuple[str, ...]]:
    """Whether the schema can express this fact at all.

    True iff the predicate is declared, both kind hints resolve to declared
    kinds, and the exact (subject, object) pair is allowed. Failure reasons
    are returned for the diagnosis note.
    """
    reasons: list[str] = []
    predicate = canonical_predicate_for_hint(schema, fact.predicate_hint)
    if predicate is None:
        reasons.append(
            f"predicate hint {fact.predicate_hint!r} matches no declared predicate"
        )
        return False, tuple(reasons)
    subject_kind = schema_kind_for_hint(schema, fact.subject_kind_hint)
    object_kind = schema_kind_for_hint(schema, fact.object_kind_hint)
    if subject_kind is None:
        reasons.append(f"kind hint {fact.subject_kind_hint!r} maps to no declared kind")
    if object_kind is None:
        reasons.append(f"kind hint {fact.object_kind_hint!r} maps to no declared kind")
    if reasons:
        return False, tuple(reasons)
    if not schema.pair_is_allowed(predicate, subject_kind, object_kind):
        reasons.append(
            f"pair ({subject_kind}, {object_kind}) is not allowed for {predicate!r}"
        )
        return False, tuple(reasons)
    return True, ()


def ranges_overlap(a: tuple[int, int], b: tuple[int, int]) -> bool:
    return a[0] <= b[1] and a[1] >= b[0]


def evidence_overlap(fact: ReferenceFact, citation: Optional[tuple[int, int]]) -> bool:
    """Whether a claim's citation line range touches any expected evidence."""
    if citation is None:
        return False
    return any(ranges_overlap(citation, ev) for ev in fact.evidence_line_ranges)


def match_claim_to_fact(
    fact: ReferenceFact,
    fact_predicate: Optional[str],
    schema,
    claim: dict,
) -> Optional[dict]:
    """Match one scored claim against one reference fact.

    Returns a match record (tier, direction, evidence overlap) or None.
    Rejected claims can only match by name (their kinds are unreliable by
    definition); reversed-direction matches are recorded but flagged and
    never count as recovery.
    """
    if fact_predicate is None or claim.get("predicate") != fact_predicate:
        return None
    subject = claim.get("subject") or {}
    object_ = claim.get("object") or {}
    subject_match = name_match(subject.get("text", ""), fact.subject_name)
    object_match = name_match(object_.get("text", ""), fact.object_name)
    forward = subject_match is not None and object_match is not None
    reversed_match = (
        name_match(subject.get("text", ""), fact.object_name) is not None
        and name_match(object_.get("text", ""), fact.subject_name) is not None
    )
    citation = claim.get("citation_range")
    accepted = bool(claim.get("accepted"))
    tier: Optional[str] = None
    direction: Optional[str] = None
    if forward:
        direction = "forward"
        tier = (
            "name"
            if subject_match == "strong" and object_match == "strong"
            else "name_weak"
        )
    elif reversed_match and accepted:
        direction = "reversed"
        tier = "name"
    elif accepted and _kind_evidence_match(fact, schema, claim):
        direction = "forward"
        tier = "kind_evidence"
    if tier is None or direction is None:
        return None
    return {
        "claim_id": claim.get("claim_id"),
        "tier": tier,
        "direction": direction,
        "evidence_overlap": evidence_overlap(fact, citation),
        "accepted": accepted,
        "support": claim.get("support"),
        "confidence": claim.get("confidence"),
        "citation_range": list(citation) if citation else None,
    }


def _kind_evidence_match(fact: ReferenceFact, schema, claim: dict) -> bool:
    subject_kind = schema_kind_for_hint(schema, fact.subject_kind_hint)
    object_kind = schema_kind_for_hint(schema, fact.object_kind_hint)
    if subject_kind is None or object_kind is None:
        return False
    subject = claim.get("subject") or {}
    object_ = claim.get("object") or {}
    if subject.get("kind") != subject_kind or object_.get("kind") != object_kind:
        return False
    if not evidence_overlap(fact, claim.get("citation_range")):
        return False
    subject_share = bool(tokens(subject.get("text", "")) & tokens(fact.subject_name))
    object_share = bool(tokens(object_.get("text", "")) & tokens(fact.object_name))
    # Rejected alternative: `subject_share or object_share`. Line 7 carries
    # three parallel `uses` facts sharing the subject "daftprompt"; a single
    # shared token on the subject makes the akar claim cross-match R2
    # (gitoxide) and R3 (hybrid search indexer). Both endpoints must align.
    return subject_share and object_share


def match_fact(
    fact: ReferenceFact,
    fact_predicate: Optional[str],
    schema,
    claims: list[dict],
) -> list[dict]:
    """All matches of one fact across the scored claims (deterministic order)."""
    matches = []
    for claim in claims:
        match = match_claim_to_fact(fact, fact_predicate, schema, claim)
        if match is not None:
            matches.append(match)
    return matches


def diagnose_fact(
    fact: ReferenceFact,
    fact_predicate: Optional[str],
    shape_present: bool,
    shape_reasons: tuple[str, ...],
    matches: list[dict],
    claims: list[dict],
    entity_mentions: list[dict],
) -> dict:
    """Per-fact diagnosis: status, categories, and human-readable note.

    Status priority (deterministic):
    1. schema_gap               — the schema cannot express the fact
    2. recovered                — accepted evidence-overlapping match
                                  reviewed as supported
    3. matched_pending_review   — accepted evidence-overlapping match whose
                                  support is unreviewed/weak/misleading
    4. matched_evidence_mismatch — accepted name-tier match, wrong lines
    5. attempted_rejected       — only rejected candidates match the fact
    6. missing_relation         — both entities emitted, no relation candidate
    7. missing_entity           — an expected entity mention was never emitted
    """
    diagnosis: dict[str, Any] = {
        "id": fact.id,
        "subject": {"name": fact.subject_name, "kind_hint": fact.subject_kind_hint},
        "predicate_hint": fact.predicate_hint,
        "predicate": fact_predicate,
        "object": {"name": fact.object_name, "kind_hint": fact.object_kind_hint},
        "expected_evidence_lines": [list(pair) for pair in fact.evidence_line_ranges],
        "schema_shape_present": shape_present,
        "matches": matches,
        "recovered": False,
        "categories": [],
    }
    categories: list[str] = []

    subject_emitted = _find_emitted_mentions(entity_mentions, fact.subject_name)
    object_emitted = _find_emitted_mentions(entity_mentions, fact.object_name)
    diagnosis["subject_emitted"] = subject_emitted
    diagnosis["object_emitted"] = object_emitted

    if not shape_present:
        diagnosis["status"] = STATUS_SCHEMA_GAP
        diagnosis["note"] = "schema cannot express this fact: " + "; ".join(shape_reasons)
        categories.append(CATEGORY_ABSENT_SCHEMA_LABEL)
        diagnosis["categories"] = categories
        return diagnosis

    accepted_forward = [
        m for m in matches if m["accepted"] and m["direction"] == "forward"
    ]
    accepted_overlap = [m for m in accepted_forward if m["evidence_overlap"]]
    accepted_no_overlap = [m for m in accepted_forward if not m["evidence_overlap"]]
    rejected_matches = [m for m in matches if not m["accepted"]]
    reversed_matches = [m for m in matches if m["direction"] == "reversed"]

    if accepted_overlap:
        best = max(
            accepted_overlap,
            key=lambda m: (
                m["support"] == SUPPORT_SUPPORTED,
                m["confidence"] if m["confidence"] is not None else -1.0,
            ),
        )
        support = best["support"]
        diagnosis["best_match"] = best
        if support == SUPPORT_SUPPORTED:
            diagnosis["status"] = STATUS_RECOVERED
            diagnosis["recovered"] = True
            diagnosis["note"] = (
                f"recovered by {best['claim_id']} (tier {best['tier']}) citing "
                f"lines {best['citation_range']}"
            )
        elif support == SUPPORT_MISLEADING:
            diagnosis["status"] = STATUS_MATCHED_PENDING_REVIEW
            diagnosis["note"] = (
                f"match {best['claim_id']} reviewed misleading; not recovered"
            )
            categories.append(CATEGORY_UNSUPPORTED_BUT_ACCEPTED)
        elif support == SUPPORT_WEAK:
            diagnosis["status"] = STATUS_MATCHED_PENDING_REVIEW
            diagnosis["note"] = f"match {best['claim_id']} reviewed weak; not recovered"
        else:
            diagnosis["status"] = STATUS_MATCHED_PENDING_REVIEW
            diagnosis["note"] = (
                f"match {best['claim_id']} awaits claim-to-evidence review"
            )
    elif accepted_no_overlap:
        best = max(
            accepted_no_overlap,
            key=lambda m: m["confidence"] if m["confidence"] is not None else -1.0,
        )
        diagnosis["best_match"] = best
        diagnosis["status"] = STATUS_MATCHED_EVIDENCE_MISMATCH
        diagnosis["note"] = (
            f"accepted match {best['claim_id']} cites lines "
            f"{best['citation_range']} but expected evidence is "
            f"{[list(pair) for pair in fact.evidence_line_ranges]}"
        )
        categories.append(CATEGORY_EVIDENCE_MISMATCH)
    elif rejected_matches:
        diagnosis["status"] = STATUS_ATTEMPTED_REJECTED
        attempted = ", ".join(str(m["claim_id"]) for m in rejected_matches)
        diagnosis["note"] = f"attempted but rejected: {attempted}"
        categories.extend(_rejected_categories(rejected_matches, claims=claims))
    else:
        predicate_misses = _predicate_misses(fact, fact_predicate, claims)
        if predicate_misses:
            diagnosis["status"] = STATUS_ATTEMPTED_REJECTED
            diagnosis["predicate_misses"] = predicate_misses
            diagnosis["note"] = (
                "endpoints related under a different predicate: "
                + ", ".join(
                    f"{m['claim_id']}({m['predicate']})" for m in predicate_misses
                )
            )
            categories.append(CATEGORY_WRONG_PREDICATE)
        elif reversed_matches:
            diagnosis["status"] = STATUS_ATTEMPTED_REJECTED
            diagnosis["note"] = (
                "only reversed-direction matches found (endpoints swapped): "
                + ", ".join(str(m["claim_id"]) for m in reversed_matches)
            )
            categories.append(CATEGORY_WRONG_DIRECTION)
        elif subject_emitted and object_emitted:
            diagnosis["status"] = STATUS_MISSING_RELATION
            diagnosis["note"] = (
                "both entities emitted but no relation candidate covers the fact"
            )
            categories.append(CATEGORY_MISSING_RELATION)
        else:
            missing = []
            if not subject_emitted:
                missing.append(f"subject {fact.subject_name!r}")
            if not object_emitted:
                missing.append(f"object {fact.object_name!r}")
            diagnosis["status"] = STATUS_MISSING_ENTITY
            diagnosis["note"] = "no mention emitted for: " + "; ".join(missing)
            categories.append(CATEGORY_MISSING_ENTITY)

    if reversed_matches and diagnosis["status"] != STATUS_ATTEMPTED_REJECTED:
        # direction errors are recorded even when a forward match exists
        if CATEGORY_WRONG_DIRECTION not in categories:
            categories.append(CATEGORY_WRONG_DIRECTION)

    diagnosis["categories"] = categories
    return diagnosis


def _find_emitted_mentions(entity_mentions: list[dict], name: str) -> list[dict]:
    """Entity mentions matching a reference name, with their snapshot lines."""
    hits = []
    for entity in entity_mentions:
        tier = name_match(entity.get("text", ""), name)
        if tier is None:
            continue
        hits.append(
            {
                "text": entity.get("text"),
                "kind": entity.get("kind"),
                "start": entity.get("start"),
                "end": entity.get("end"),
                "lines": entity.get("lines"),
                "match": tier,
            }
        )
    return hits


def _rejected_categories(rejected_matches: list[dict], claims: list[dict]) -> list[str]:
    by_id = {str(claim.get("claim_id")): claim for claim in claims}
    categories: list[str] = []
    for match in rejected_matches:
        claim = by_id.get(str(match["claim_id"])) or {}
        for category in claim.get("categories", ()):
            if category not in categories:
                categories.append(category)
    if not categories:
        categories.append(CATEGORY_WRONG_TYPE)
    return categories


def _predicate_misses(
    fact: ReferenceFact, fact_predicate: Optional[str], claims: list[dict]
) -> list[dict]:
    """Claims whose endpoints name-match the fact but use another predicate."""
    if fact_predicate is None:
        return []
    misses = []
    for claim in claims:
        if claim.get("predicate") == fact_predicate:
            continue
        subject = claim.get("subject") or {}
        object_ = claim.get("object") or {}
        if (
            name_match(subject.get("text", ""), fact.subject_name) is not None
            and name_match(object_.get("text", ""), fact.object_name) is not None
        ):
            misses.append(
                {"claim_id": claim.get("claim_id"), "predicate": claim.get("predicate")}
            )
    return misses
