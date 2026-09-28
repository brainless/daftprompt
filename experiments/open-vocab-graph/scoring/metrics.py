"""Metric aggregation for Epic 014 scorecards.

All functions are pure: they take already-scored claims / fact diagnoses /
question diagnoses and produce the metric blocks embedded in the scorecard.
Nothing here reads the human-review file; reviewed support values simply
arrive inside the claims.

Metric list (Epic 014 Task 4 acceptance criteria):
- R1-R9 recovery (recovery requires an accepted, citation-valid,
  evidence-overlapping claim whose support review says "supported"),
  plus matched-pending-review and omission breakdowns;
- valid / supported / misleading claims (citation validity separated from
  semantic support), duplicates, omissions;
- entity recall/precision and relation recall/precision **where the frozen
  reference set supports them** (repeated entities + R1-R9; the reference
  set is not a complete entity/relation inventory, so full precision is not
  computable and is reported as a clearly-labeled reference-supported lower
  bound instead);
- six-question evidence coverage;
- runtime and peak memory when the run recorded them.
"""
from __future__ import annotations

from .reference import RepeatedEntity, name_match
from .taxonomy import (
    ALL_CATEGORIES,
    CATEGORY_UNSUPPORTED_BUT_ACCEPTED,
    OMISSION_STATUSES,
    STATUS_MATCHED_PENDING_REVIEW,
    STATUS_RECOVERED,
    STATUS_SCHEMA_GAP,
    SUPPORT_MISLEADING,
    SUPPORT_SUPPORTED,
    SUPPORT_UNREVIEWED,
    SUPPORT_WEAK,
)


def claim_metrics(claims: list[dict], duplicates: list[dict]) -> dict:
    """Claim-level counts: citation validity separated from support.

    "Valid" = accepted by the grounding pipeline (schema pair + exact
    snapshot grounding + chunk locality) with a resolvable citation — the
    analog of Epic 013's harness-accepted claims. Support counts are over
    valid claims only and default to unreviewed.
    """
    valid = [c for c in claims if c["accepted"] and c["citation_valid"]]
    duplicate_count = sum(group["duplicate_count"] for group in duplicates)
    return {
        "emitted_candidates": len(claims),
        "accepted": sum(1 for c in claims if c["accepted"]),
        "rejected": sum(1 for c in claims if not c["accepted"]),
        "valid": len(valid),
        "valid_but_citation_invalid": sum(
            1 for c in claims if c["accepted"] and not c["citation_valid"]
        ),
        "supported": sum(1 for c in valid if c["support"] == SUPPORT_SUPPORTED),
        "weak": sum(1 for c in valid if c["support"] == SUPPORT_WEAK),
        "misleading": sum(1 for c in valid if c["support"] == SUPPORT_MISLEADING),
        "unreviewed": sum(1 for c in valid if c["support"] == SUPPORT_UNREVIEWED),
        "duplicates": duplicate_count,
        "duplicate_groups": duplicates,
    }


def reference_relation_metrics(fact_diagnoses: list[dict]) -> dict:
    """R1-R9 recovery, pending matches, omissions, and the reference-supported
    relation precision/recall lower bounds."""
    total = len(fact_diagnoses)
    recovered = [f for f in fact_diagnoses if f["status"] == STATUS_RECOVERED]
    pending = [f for f in fact_diagnoses if f["status"] == STATUS_MATCHED_PENDING_REVIEW]
    schema_gaps = [f for f in fact_diagnoses if f["status"] == STATUS_SCHEMA_GAP]
    omissions = [f for f in fact_diagnoses if f["status"] in OMISSION_STATUSES]
    matched_any = [
        f
        for f in fact_diagnoses
        if any(m["accepted"] and m["direction"] == "forward" for m in f["matches"])
    ]
    accepted_match_claims = {
        str(m["claim_id"])
        for f in fact_diagnoses
        for m in f["matches"]
        if m["accepted"] and m["direction"] == "forward"
    }
    evidence_backed_claims = {
        str(m["claim_id"])
        for f in fact_diagnoses
        for m in f["matches"]
        if m["accepted"] and m["direction"] == "forward" and m["evidence_overlap"]
    }
    return {
        "total": total,
        "recovered": len(recovered),
        "recovered_ids": [f["id"] for f in recovered],
        "matched_pending_review": len(pending),
        "pending_ids": [f["id"] for f in pending],
        "schema_gaps": len(schema_gaps),
        "omissions": len(omissions),
        "omission_ids": [f["id"] for f in omissions],
        "reference_supported_precision": {
            "note": (
                "lower bound: accepted claims that are evidence-overlapping "
                "matches of a frozen reference fact; the reference set is not "
                "a complete relation inventory, so full precision requires "
                "human support review"
            ),
            "matched_accepted_claims": len(accepted_match_claims),
            "evidence_backed_accepted_claims": len(evidence_backed_claims),
        },
        "reference_supported_recall": {
            "note": (
                "structural match (any accepted forward match, evidence "
                "overlap not required) over R1-R9; recovery additionally "
                "requires support review"
            ),
            "facts_with_accepted_match": len(matched_any),
        },
    }


def entity_metrics(
    entity_mentions: list[dict], repeated_entities: tuple[RepeatedEntity, ...]
) -> dict:
    """Repeated-entity recall/precision over the frozen occurrence lines.

    The reference set records occurrence lines as exhaustive case-sensitive
    substring positions for these 8 names only — hence a reference-supported
    metric, not a full entity-inventory precision.
    """
    total = len(repeated_entities)
    matched: list[dict] = []
    missing: list[dict] = []
    for repeated in repeated_entities:
        hits = [
            mention
            for mention in entity_mentions
            if name_match(str(mention.get("text", "")), repeated.name) is not None
        ]
        if hits:
            on_recorded_lines = any(
                mention.get("lines")
                and any(line in repeated.occurrence_lines for line in mention["lines"])
                for mention in hits
            )
            matched.append(
                {
                    "name": repeated.name,
                    "mention_count": len(hits),
                    "on_recorded_lines": on_recorded_lines,
                }
            )
        else:
            missing.append({"name": repeated.name})

    matching_mentions = []
    for mention in entity_mentions:
        for repeated in repeated_entities:
            if name_match(str(mention.get("text", "")), repeated.name) is not None:
                lines = mention.get("lines") or []
                matching_mentions.append(
                    {
                        "name": repeated.name,
                        "text": mention.get("text"),
                        "lines": lines,
                        "on_recorded_lines": any(
                            line in repeated.occurrence_lines for line in lines
                        ),
                    }
                )
                break
    on_line_count = sum(1 for m in matching_mentions if m["on_recorded_lines"])
    return {
        "repeated_entity_recall": {
            "matched": len(matched),
            "total": total,
            "missing": [m["name"] for m in missing],
            "detail": matched,
        },
        "repeated_entity_precision": {
            "note": (
                "mentions matching a repeated-entity name that sit on the "
                "name's recorded occurrence lines; the reference set records "
                "occurrence lines for these 8 names only"
            ),
            "on_recorded_lines": on_line_count,
            "matching_mentions": len(matching_mentions),
        },
    }


def question_summary(question_diagnoses: list[dict]) -> dict:
    """Counts per evidence-coverage outcome over the six questions."""
    counts: dict[str, int] = {}
    for question in question_diagnoses:
        coverage = question["coverage"]
        counts[coverage] = counts.get(coverage, 0) + 1
    return {
        "total": len(question_diagnoses),
        "by_coverage": counts,
        "pending_support_review": sum(
            1 for q in question_diagnoses if q.get("pending_support_review")
        ),
    }


def failure_category_counts(
    claims: list[dict], dropped: list[dict], fact_diagnoses: list[dict]
) -> dict:
    """Occurrences of every taxonomy category, by where it was observed."""
    counts = {
        category: {
            "claim_rejections": 0,
            "dropped_candidates": 0,
            "fact_diagnoses": 0,
            "total": 0,
        }
        for category in ALL_CATEGORIES
    }

    def bump(category: str, bucket: str) -> None:
        if category in counts:
            counts[category][bucket] += 1
            counts[category]["total"] += 1

    for claim in claims:
        if not claim["accepted"]:
            for category in claim["categories"]:
                bump(category, "claim_rejections")
    for record in dropped:
        bump(record["category"], "dropped_candidates")
    for fact in fact_diagnoses:
        for category in fact["categories"]:
            bump(category, "fact_diagnoses")

    # unsupported-but-accepted is review-filled: count reviewed-misleading
    # valid claims directly (the slot stays zero before review)
    counts[CATEGORY_UNSUPPORTED_BUT_ACCEPTED]["claim_rejections"] = sum(
        1
        for c in claims
        if c["accepted"] and c["citation_valid"] and c["support"] == SUPPORT_MISLEADING
    )
    counts[CATEGORY_UNSUPPORTED_BUT_ACCEPTED]["total"] += counts[
        CATEGORY_UNSUPPORTED_BUT_ACCEPTED
    ]["claim_rejections"]
    return {category: data for category, data in counts.items() if data["total"]}
