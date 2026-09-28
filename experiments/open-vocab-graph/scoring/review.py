"""Human-review input handling for Epic 014 scoring.

Exact citation validity is deterministic; whether a cited line actually
*supports* a subject-predicate-object claim is not — that judgment is
reserved for human review (Tasks 5-8). This module:

- generates a per-claim review template from a scorecard (claim IDs, claim
  strings, citations, `support: "unreviewed"`), plus per-question entries;
- loads a filled review file and applies it to scored claims/questions,
  collecting warnings instead of failing on malformed entries.

Deterministic components never depend on the review file: with no review
input every claim stays "unreviewed" and no fact is reported recovered.
"""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Optional

from .taxonomy import SUPPORT_VALUES

REVIEW_SCHEMA_VERSION = 1
QUESTION_COVERAGE_VALUES = (
    "correct_absence",
    "false_claims",
    "covered",
    "partial",
    "not_covered",
)


def build_review_template(scorecard: dict) -> dict:
    """Template with every scored claim and question, support unreviewed."""
    return {
        "schema_version": REVIEW_SCHEMA_VERSION,
        "purpose": (
            "Human claim-to-evidence support and question-coverage review for "
            "Epic 014. Fill `support` per claim (supported | weak | misleading); "
            "leave as unreviewed when not judged. Deterministic scoring never "
            "reads this file's existence — absent review keeps every claim "
            "unreviewed and no fact recovered."
        ),
        "support_values": list(SUPPORT_VALUES),
        "question_coverage_values": list(QUESTION_COVERAGE_VALUES),
        "run_directory": scorecard.get("run_directory"),
        "claims": [
            {
                "claim_id": claim["claim_id"],
                "claim_key": claim["claim_key"],
                "claim": claim["claim"],
                "citations": (
                    [f"AGENTS.md:{claim['citation']['start_line']}-{claim['citation']['end_line']}"]
                    if claim.get("citation")
                    else []
                ),
                "accepted": claim["accepted"],
                "support": "unreviewed",
                "reviewer": None,
                "note": "",
            }
            for claim in scorecard["candidates"]["claims"]
        ],
        "questions": [
            {
                "question_id": question["id"],
                "question": question["question"],
                "expected": question["expected"],
                "coverage": None,
                "reviewer": None,
                "note": "",
            }
            for question in scorecard["questions"]
        ],
    }


def load_review(path: Path | None) -> tuple[dict, list[str]]:
    """Load a review file; returns (data, warnings). Missing/None -> ({}, [])."""
    if path is None:
        return {}, []
    path = Path(path)
    if not path.is_file():
        return {}, []
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        return {}, [f"review file {path} is not valid JSON: {error}"]
    if not isinstance(data, dict):
        return {}, [f"review file {path} must contain a JSON object"]
    return data, []


def apply_review(scorecard: dict, review: dict) -> list[str]:
    """Apply review judgments onto the scorecard (mutates claims/questions).

    Returns warnings for unrecognized values, unknown claim IDs, and
    claim_key mismatches. Malformed entries are skipped, never fatal.
    """
    warnings: list[str] = []
    claims = scorecard["candidates"]["claims"]
    by_id = {claim["claim_id"]: claim for claim in claims}

    seen_ids: set[str] = set()
    for entry in review.get("claims") or []:
        if not isinstance(entry, dict):
            warnings.append("ignored non-object claim review entry")
            continue
        claim_id = entry.get("claim_id")
        if claim_id not in by_id:
            warnings.append(f"review references unknown claim_id {claim_id!r}")
            continue
        claim = by_id[claim_id]
        seen_ids.add(claim_id)
        expected_key = claim["claim_key"]
        if entry.get("claim_key") not in (None, expected_key):
            warnings.append(
                f"review claim_key mismatch for {claim_id}: "
                f"{entry.get('claim_key')!r} != {expected_key!r}; applying by claim_id"
            )
        support = entry.get("support")
        if support in SUPPORT_VALUES:
            claim["support"] = support
            claim["reviewer"] = entry.get("reviewer")
            claim["review_note"] = str(entry.get("note") or "")
        elif support not in (None, "unreviewed"):
            warnings.append(
                f"claim {claim_id}: unrecognized support {support!r}; "
                f"expected one of {SUPPORT_VALUES} — kept unreviewed"
            )

    questions = scorecard["questions"]
    questions_by_id = {question["id"]: question for question in questions}
    for entry in review.get("questions") or []:
        if not isinstance(entry, dict):
            warnings.append("ignored non-object question review entry")
            continue
        question_id = entry.get("question_id")
        if question_id not in questions_by_id:
            warnings.append(f"review references unknown question_id {question_id!r}")
            continue
        question = questions_by_id[question_id]
        coverage = entry.get("coverage")
        if coverage in QUESTION_COVERAGE_VALUES:
            question["coverage"] = coverage
            question["coverage_source"] = "review"
            question["reviewer"] = entry.get("reviewer")
            question["review_note"] = str(entry.get("note") or "")
        elif coverage is not None:
            warnings.append(
                f"question {question_id}: unrecognized coverage {coverage!r}; "
                f"kept deterministic value"
            )

    scorecard["review"]["claims_reviewed"] = len(seen_ids)
    scorecard["review"]["questions_reviewed"] = sum(
        1 for question in questions if question.get("coverage_source") == "review"
    )
    return warnings
