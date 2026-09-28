"""End-to-end scoring of one Task 3 run directory (Epic 014 Task 4).

`score_run()` is the single entry point used by the CLI and by Tasks 5-8:

1. load the versioned schema and the run directory (manifest, relations,
   entities, per-chunk records);
2. ground the frozen snapshot: its sha256 must equal both the schema
   artifact's frozen `source_snapshot.sha256` and the run manifest's
   recorded snapshot sha256 — scoring fails closed otherwise;
3. re-validate and ground every emitted relation candidate (schema pair
   verdict, exact snapshot text, chunk locality); citations are derived
   from the snapshot alone;
4. score entity mentions, surface duplicates, and chunk-level drops;
5. match claims to the frozen reference set (R1-R9), diagnose each fact,
   and derive six-question evidence coverage;
6. apply the human-review file when one exists (support/coverage),
   otherwise everything stays "unreviewed";
7. aggregate metrics and (optionally) write `scorecard.json`,
   `scorecard.md`, and a `review-template.json`.

Pure stdlib; no gliner2/torch imports; never writes inside frozen-inputs/
or Epic 013's output/.
"""
from __future__ import annotations

import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

from schema import core

from . import (
    DEFAULT_REFERENCE_SET_PATH,
    DEFAULT_SCHEMA_PATH,
    DEFAULT_SNAPSHOT_PATH,
    REVIEW_FILE_NAME,
    SCORING_DIR_NAME,
    metrics,
)
from .accept import (
    chunk_span_map,
    dropped_records,
    load_run_directory,
    run_peak_memory_bytes,
    score_candidates,
    score_entity_mentions,
    surface_duplicate_groups,
)
from .grounding import SnapshotGrounding, SnapshotMismatchError, sha256_bytes
from .reference import (
    canonical_predicate_for_hint,
    diagnose_fact,
    fact_shape_present,
    load_reference_set,
    match_fact,
)
from .report import write_scorecard
from .review import apply_review, load_review
from .taxonomy import SUPPORT_UNREVIEWED


def score_run(
    run_dir: Path,
    schema_path: Optional[Path] = None,
    snapshot_path: Optional[Path] = None,
    reference_set_path: Optional[Path] = None,
    review_path: Optional[Path] = None,
    write: bool = False,
    out_dir: Optional[Path] = None,
    force: bool = False,
) -> dict:
    """Score one harness run directory; returns the scorecard dict."""
    scoring_started = time.perf_counter()
    schema_artifact_path = Path(schema_path) if schema_path else DEFAULT_SCHEMA_PATH
    schema = core.load_schema(schema_artifact_path)
    run = load_run_directory(Path(run_dir))
    manifest = run["manifest"]

    grounding = SnapshotGrounding(
        Path(snapshot_path) if snapshot_path else DEFAULT_SNAPSHOT_PATH
    )
    frozen = schema.data.get("source_snapshot") or {}
    grounding.verify(
        frozen.get("sha256") or "", frozen.get("line_count")
    )
    manifest_snapshot = manifest.get("snapshot") or {}
    if manifest_snapshot.get("sha256") and manifest_snapshot["sha256"] != grounding.sha256:
        raise SnapshotMismatchError(
            f"run {run['run_dir']} was executed against snapshot sha256 "
            f"{manifest_snapshot['sha256']} but scoring snapshot "
            f"{grounding.path} has {grounding.sha256}; refusing to score "
            "across different snapshots"
        )

    chunk_spans = chunk_span_map(run["chunk_records"])
    relations = run["relations_doc"].get("relations") or []
    candidates = score_candidates(relations, schema, grounding, chunk_spans)
    claims = [candidate.to_dict() for candidate in candidates]

    entities = score_entity_mentions(
        run["entities_doc"].get("entities") or [], grounding
    )
    duplicates = surface_duplicate_groups(candidates)
    dropped = dropped_records(run["chunk_records"])

    integrity_warnings = sorted(
        {note for candidate in candidates for note in candidate.notes}
    )
    verdict_disagreements = [
        {"claim_id": candidate.claim_id, "note": note}
        for candidate in candidates
        for note in candidate.notes
        if note.startswith("verdict_disagreement")
    ]

    reference = load_reference_set(
        Path(reference_set_path) if reference_set_path else DEFAULT_REFERENCE_SET_PATH
    )

    question_diagnoses = diagnose_questions(reference["questions"], claims)

    resolved_review_path = _resolve_review_path(
        review_path, Path(run["run_dir"]), out_dir
    )
    review_data, review_warnings = load_review(resolved_review_path)

    peak_memory, peak_memory_note = run_peak_memory_bytes(manifest)
    scoring_elapsed = time.perf_counter() - scoring_started

    scorecard: dict = {
        "scoring_schema_version": 1,
        "epic": "014-gliner25-graph-extraction-experiment",
        "task": "Task 4: ground evidence and implement scoring",
        "generated_utc": datetime.now(timezone.utc).isoformat(),
        "run_directory": str(run["run_dir"]),
        "run_manifest": {
            "model_id": (manifest.get("model") or {}).get("id"),
            "variant": manifest.get("variant"),
            "status": manifest.get("status"),
            "started_utc": manifest.get("started_utc"),
            "finished_utc": manifest.get("finished_utc"),
            "elapsed_seconds": manifest.get("elapsed_seconds"),
            "snapshot_sha256": manifest_snapshot.get("sha256"),
            "schema_artifact_sha256": (manifest.get("schema") or {}).get("artifact_sha256"),
            "counts": manifest.get("counts"),
        },
        "integrity": {
            "snapshot_sha256_verified_against_schema": True,
            "snapshot_sha256_verified_against_run_manifest": True,
            "schema_artifact_sha256": sha256_bytes(schema_artifact_path.read_bytes()),
            "verdict_disagreements": verdict_disagreements,
            "warnings": integrity_warnings,
        },
        "snapshot": {
            "path": str(grounding.path),
            "sha256": grounding.sha256,
            "line_count": grounding.line_count,
            "citation_rule": (
                "citations are physical line ranges derived from this snapshot; "
                "model text is verified against the exact slice and never used "
                "as citation text"
            ),
        },
        "candidates": {
            "claims": claims,
            "dropped": dropped,
            "duplicates": duplicates,
        },
        "entities": {
            "mentions_scored": len(entities),
            "mentions": entities,
        },
        "reference_relations": {"facts": []},
        "questions": question_diagnoses,
        "review": {
            "source": str(resolved_review_path) if review_data else None,
            "claims_reviewed": 0,
            "questions_reviewed": 0,
            "warnings": review_warnings,
            "note": (
                "support defaults to unreviewed; exact citation validity is "
                "deterministic and independent of this file"
            ),
        },
        "runtime": {
            "run_elapsed_seconds": manifest.get("elapsed_seconds"),
            "scoring_elapsed_seconds": scoring_elapsed,
            "run_peak_memory_bytes": peak_memory,
            "run_peak_memory_note": peak_memory_note,
        },
    }

    if review_data:
        warnings = apply_review(scorecard, review_data)
        scorecard["review"]["warnings"].extend(warnings)

    # Fact diagnosis runs after review so match records carry the reviewed
    # support; recovery always requires reviewed support == "supported".
    fact_diagnoses = []
    for fact in reference["facts"]:
        fact_predicate = canonical_predicate_for_hint(schema, fact.predicate_hint)
        shape_present, shape_reasons = fact_shape_present(schema, fact)
        matches = match_fact(fact, fact_predicate, schema, claims)
        fact_diagnoses.append(
            diagnose_fact(
                fact,
                fact_predicate,
                shape_present,
                shape_reasons,
                matches,
                claims,
                entities,
            )
        )
    scorecard["reference_relations"]["facts"] = fact_diagnoses
    _refresh_question_pending(scorecard["questions"], claims)

    scorecard["metrics"] = {
        "claims": metrics.claim_metrics(claims, duplicates),
        "reference_relations": metrics.reference_relation_metrics(fact_diagnoses),
        "entities": metrics.entity_metrics(entities, reference["repeated_entities"]),
        "questions": metrics.question_summary(question_diagnoses),
    }
    scorecard["failure_categories"] = metrics.failure_category_counts(
        claims, dropped, fact_diagnoses
    )

    if write:
        target = Path(out_dir) if out_dir else Path(run["run_dir"]) / SCORING_DIR_NAME
        write_scorecard(scorecard, target, force=force, review_path=resolved_review_path)

    return scorecard


def diagnose_questions(reference_questions, claims: list[dict]) -> list[dict]:
    """Evidence-based coverage per question, plus review hooks.

    For questions expecting support: a passage is covered when at least one
    accepted, citation-valid claim cites a line range overlapping it. The
    coverage value is covered/partial/not_covered; `pending_support_review`
    marks that no covering claim has been reviewed as supported yet, so the
    headline "covered" must not be read as verified entailment.

    For unknown-expected questions (no expected passages) evidence cannot
    decide absence of an answer, so the deterministic value is
    `pending_review` and the reviewer records correct_absence/false_claims.
    """
    valid_claims = [
        claim
        for claim in claims
        if claim["accepted"] and claim["citation_valid"] and claim["citation_range"]
    ]
    diagnoses = []
    for question in reference_questions:
        covered_passages = []
        covering_claims: dict[tuple, list[str]] = {}
        for passage in question.passages:
            hits = [
                claim["claim_id"]
                for claim in valid_claims
                if claim["citation_range"][0] <= passage[1]
                and claim["citation_range"][1] >= passage[0]
            ]
            if hits:
                covered_passages.append(list(passage))
                covering_claims[passage] = hits
        if question.expected == "unknown":
            coverage = "pending_review"
            pending = True
            note = (
                "no expected evidence passages exist for an unknown question; "
                "record correct_absence or false_claims during review"
            )
        elif question.passages and len(covered_passages) == len(question.passages):
            coverage = "covered"
            pending = any(
                claim_support_unreviewed(claims, ids) for ids in covering_claims.values()
            )
            note = "all expected passages cited by accepted claims"
        elif covered_passages:
            coverage = "partial"
            pending = any(
                claim_support_unreviewed(claims, ids) for ids in covering_claims.values()
            )
            note = (
                f"{len(covered_passages)}/{len(question.passages)} expected "
                "passages cited"
            )
        else:
            coverage = "not_covered"
            pending = False
            note = "no accepted claim cites any expected passage"
        diagnoses.append(
            {
                "id": question.id,
                "question": question.question,
                "expected": question.expected,
                "passages": [list(pair) for pair in question.passages],
                "covered_passages": covered_passages,
                "covering_claims": {
                    f"{pair[0]}-{pair[1]}": ids for pair, ids in covering_claims.items()
                },
                "coverage": coverage,
                "coverage_source": "evidence_derived",
                "pending_support_review": pending,
                "note": note,
                "rationale": question.rationale,
            }
        )
    return diagnoses


def claim_support_unreviewed(claims: list[dict], claim_ids: list[str]) -> bool:
    by_id = {claim["claim_id"]: claim for claim in claims}
    return any(
        (by_id.get(claim_id) or {}).get("support") == SUPPORT_UNREVIEWED
        for claim_id in claim_ids
    )


def _refresh_question_pending(question_diagnoses: list[dict], claims: list[dict]) -> None:
    """Recompute pending-support flags after review updated claim support."""
    for question in question_diagnoses:
        if question["coverage_source"] != "evidence_derived":
            continue
        covering = [
            claim_id
            for ids in question["covering_claims"].values()
            for claim_id in ids
        ]
        question["pending_support_review"] = (
            claim_support_unreviewed(claims, covering) if covering else False
        )


def _resolve_review_path(
    review_path: Optional[Path], run_dir: Path, out_dir: Optional[Path]
) -> Optional[Path]:
    if review_path is not None:
        return Path(review_path)
    candidate = run_dir / SCORING_DIR_NAME / REVIEW_FILE_NAME
    if candidate.is_file():
        return candidate
    if out_dir is not None:
        candidate = Path(out_dir) / REVIEW_FILE_NAME
        if candidate.is_file():
            return candidate
    return None
