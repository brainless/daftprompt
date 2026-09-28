"""Claim acceptance for Epic 014: schema verdict + grounding + chunk locality.

Loads a Task 3 run directory, re-validates every emitted relation candidate
against the versioned schema (never trusting the run's stored verdicts), and
grounds both endpoints on the frozen snapshot via `scoring.grounding`:

- endpoint character offsets must resolve inside the snapshot and the stored
  mention text must equal the snapshot slice **exactly** — a candidate can
  never supply citation text independent of the snapshot;
- both endpoints must share at least one recorded chunk (JointIE locality);
  a violation is bookkeeping/candidate defect, recorded and rejected;
- a claim is accepted only when the schema pair verdict, grounding, exact
  text match, and locality all pass; everything else stays visible as a
  rejected candidate with taxonomy categories.

Each accepted claim's citation is the union of the full physical lines
covering both endpoints, with text derived from the snapshot alone.

Also computes surface duplicates (distinct spans, identical surface triple)
and scores entity mentions (grounding + snapshot lines for the repeated-
entity metrics). Pure stdlib; no gliner2/torch imports.
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional

from schema import core

from .grounding import GroundedSpan, SnapshotGrounding, norm_words, text_match
from .taxonomy import (
    CATEGORY_BOUNDARY_LOSS,
    DROPPED_CATEGORY,
    GROUNDING_CODE_CATEGORIES,
    SUPPORT_UNREVIEWED,
    categories_for_codes,
)

CODE_SPAN_TEXT_MISMATCH = "span_text_mismatch"
CODE_CROSS_CHUNK_LOCALITY = "cross_chunk_locality"
CODE_VERDICT_DISAGREEMENT = "verdict_disagreement"


class ScoringError(RuntimeError):
    """The run directory cannot be scored (missing/corrupt/inconsistent)."""


@dataclass
class ScoredCandidate:
    """One emitted relation candidate after grounding and re-validation.

    `accepted` combines the recomputed schema verdict with grounding (exact
    snapshot text) and chunk locality. `support` is the human-review hook
    and defaults to "unreviewed"; deterministic scoring never sets it.
    """

    claim_id: str
    claim_key: str
    predicate: str
    model_label: str
    subject: dict
    object: dict
    confidence: Optional[float]
    occurrences: int
    source_chunks: list
    schema_verdict: dict
    grounding: dict
    citation: Optional[dict]
    citation_range: Optional[tuple[int, int]]
    accepted: bool
    citation_valid: bool
    codes: list[str]
    categories: list[str]
    support: str = SUPPORT_UNREVIEWED
    reviewer: Optional[str] = None
    review_note: str = ""
    notes: list[str] = field(default_factory=list)

    @property
    def claim(self) -> str:
        return claim_string(self.predicate, self.subject, self.object)

    def to_dict(self) -> dict:
        return {
            "claim_id": self.claim_id,
            "claim_key": self.claim_key,
            "claim": self.claim,
            "predicate": self.predicate,
            "model_label": self.model_label,
            "subject": self.subject,
            "object": self.object,
            "confidence": self.confidence,
            "occurrences": self.occurrences,
            "source_chunks": self.source_chunks,
            "schema_verdict": self.schema_verdict,
            "grounding": self.grounding,
            "citation": self.citation,
            "citation_range": list(self.citation_range) if self.citation_range else None,
            "accepted": self.accepted,
            "citation_valid": self.citation_valid,
            "codes": list(self.codes),
            "categories": list(self.categories),
            "support": self.support,
            "reviewer": self.reviewer,
            "review_note": self.review_note,
            "notes": list(self.notes),
        }


def claim_string(predicate: str, subject: dict, object_: dict) -> str:
    return (
        f"{subject.get('kind', '')}({subject.get('text', '')}) "
        f"-{predicate}-> {object_.get('kind', '')}({object_.get('text', '')})"
    )


def load_run_directory(run_dir: Path) -> dict:
    """Load and shape-check a Task 3 run directory (read-only)."""
    run_dir = Path(run_dir)
    required = ("manifest.json", "relations.json", "entities.json")
    for name in required:
        if not (run_dir / name).is_file():
            raise ScoringError(f"run directory {run_dir} is missing {name}")
    manifest = _load_json(run_dir / "manifest.json")
    relations_doc = _load_json(run_dir / "relations.json")
    entities_doc = _load_json(run_dir / "entities.json")
    chunk_records = []
    chunks_dir = run_dir / "chunks"
    if chunks_dir.is_dir():
        for path in sorted(chunks_dir.glob("chunk-*.json")):
            chunk_records.append(_load_json(path))
    else:
        raise ScoringError(f"run directory {run_dir} is missing chunks/")
    return {
        "run_dir": run_dir,
        "manifest": manifest,
        "relations_doc": relations_doc,
        "entities_doc": entities_doc,
        "chunk_records": chunk_records,
    }


def _load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise ScoringError(f"{path} is not valid JSON: {error}") from error


def chunk_span_map(chunk_records: list[dict]) -> dict[int, tuple[int, int]]:
    spans = {}
    for record in chunk_records:
        chunk = record.get("chunk") or {}
        index = chunk.get("index")
        if index is None:
            raise ScoringError("chunk record without index")
        spans[int(index)] = (int(chunk["start_char"]), int(chunk["end_char"]))
    return spans


def containing_chunks(span: tuple[int, int], chunk_spans: dict[int, tuple[int, int]]) -> list[int]:
    """Chunk indexes whose recorded character range fully contains the span."""
    start, end = span
    return sorted(
        index
        for index, (c_start, c_end) in chunk_spans.items()
        if c_start <= start and end <= c_end
    )


def _ground_endpoint(
    endpoint: dict, grounding: SnapshotGrounding, chunk_spans: dict[int, tuple[int, int]]
) -> tuple[GroundedSpan, dict, list[str]]:
    """Ground one endpoint mention; returns (span, record, codes)."""
    span = grounding.ground(int(endpoint.get("start", -1)), int(endpoint.get("end", -1)))
    exact, normalized_equal = text_match(span, str(endpoint.get("text", "")))
    codes = list(span.codes)
    if span.resolved and not exact:
        codes.append(CODE_SPAN_TEXT_MISMATCH)
    record = {
        **{key: endpoint.get(key) for key in ("kind", "text", "confidence", "occurrences")},
        "start": span.start,
        "end": span.end,
        "start_line": span.start_line,
        "end_line": span.end_line,
        "snapshot_slice": span.slice_text,
        "citation_lines": [span.start_line, span.end_line],
        "citation_text": span.line_text,
        "text_exact_match": exact,
        "text_normalized_match": normalized_equal,
        "containing_chunks": containing_chunks((span.start, span.end), chunk_spans),
        "source_chunks": list(endpoint.get("source_chunks") or []),
        "span_verified_in_run": bool(endpoint.get("span_verified")),
    }
    return span, record, codes


def score_candidate(
    raw_relation: dict,
    schema: core.ExperimentSchema,
    grounding: SnapshotGrounding,
    chunk_spans: dict[int, tuple[int, int]],
    claim_id: str,
) -> ScoredCandidate:
    """Ground + re-validate one emitted relation candidate."""
    subject_raw = raw_relation.get("subject") or {}
    object_raw = raw_relation.get("object") or {}
    predicate = str(raw_relation.get("predicate", ""))
    model_label = str(raw_relation.get("model_label", ""))

    # Re-derive the schema verdict from typed fields; never trust the run's
    # stored verdict (integrity check records any disagreement).
    recomputed = core.validate_claim(
        schema,
        core.Claim(
            subject_text=str(subject_raw.get("text", "")),
            subject_kind=str(subject_raw.get("kind", "")),
            predicate=predicate,
            object_text=str(object_raw.get("text", "")),
            object_kind=str(object_raw.get("kind", "")),
            model_label=model_label or None,
        ),
    )
    recomputed_dict = {
        "accepted": bool(recomputed.accepted),
        "codes": list(recomputed.codes),
        "details": list(recomputed.details),
    }
    stored_verdict = raw_relation.get("verdict") or {}
    notes: list[str] = []
    if bool(stored_verdict.get("accepted")) != bool(recomputed.accepted):
        notes.append(
            "verdict_disagreement: run stored accepted="
            f"{stored_verdict.get('accepted')} but revalidation says "
            f"accepted={recomputed.accepted}"
        )

    subject_span, subject_record, subject_codes = _ground_endpoint(
        subject_raw, grounding, chunk_spans
    )
    object_span, object_record, object_codes = _ground_endpoint(
        object_raw, grounding, chunk_spans
    )

    common_chunks = sorted(
        set(subject_record["containing_chunks"]) & set(object_record["containing_chunks"])
    )
    locality_ok = bool(common_chunks)
    locality_codes: list[str] = []
    if not locality_ok:
        locality_codes.append(CODE_CROSS_CHUNK_LOCALITY)
    # a recorded source chunk that does not contain its endpoint is a run
    # bookkeeping defect — visible, non-fatal
    for name, record in (("subject", subject_record), ("object", object_record)):
        for chunk_index in record["source_chunks"]:
            if chunk_index not in record["containing_chunks"]:
                notes.append(
                    f"{name} source chunk {chunk_index} does not contain its span"
                )

    codes = (
        list(recomputed.codes)
        + subject_codes
        + object_codes
        + locality_codes
    )
    if any(note.startswith("verdict_disagreement") for note in notes):
        codes.append(CODE_VERDICT_DISAGREEMENT)

    citation = None
    citation_range = None
    if subject_span.resolved and object_span.resolved:
        start_line = min(subject_span.start_line, object_span.start_line)
        end_line = max(subject_span.end_line, object_span.end_line)
        citation_range = (start_line, end_line)
        citation = {
            "start_line": start_line,
            "end_line": end_line,
            "text": grounding.lines_text(start_line, end_line),
            "derived_from": "snapshot",
        }

    grounding_record = {
        "subject": subject_record,
        "object": object_record,
        "common_chunks": common_chunks,
        "chunk_locality_ok": locality_ok,
    }

    accepted = bool(recomputed.accepted) and (
        subject_span.resolved
        and object_span.resolved
        and subject_record["text_exact_match"]
        and object_record["text_exact_match"]
        and locality_ok
    )
    # citation validity is exact-line resolvability, independent of support
    citation_valid = bool(
        accepted
        and citation is not None
        and citation_range is not None
        and citation["text"]
        == grounding.lines_text(citation_range[0], citation_range[1])
    )

    unique_codes = list(dict.fromkeys(codes))
    return ScoredCandidate(
        claim_id=claim_id,
        claim_key=claim_key(predicate, subject_raw, object_raw),
        predicate=predicate,
        model_label=model_label,
        subject=subject_record,
        object=object_record,
        confidence=raw_relation.get("confidence"),
        occurrences=int(raw_relation.get("occurrences", 1)),
        source_chunks=list(raw_relation.get("source_chunks") or []),
        schema_verdict=recomputed_dict,
        grounding=grounding_record,
        citation=citation,
        citation_range=citation_range,
        accepted=accepted,
        citation_valid=citation_valid,
        codes=unique_codes,
        categories=list(categories_for_codes(unique_codes)),
        notes=notes,
    )


def claim_key(predicate: str, subject: dict, object_: dict) -> str:
    return (
        f"{predicate}|{subject.get('kind', '')}:{subject.get('start')}-{subject.get('end')}"
        f"|{object_.get('kind', '')}:{object_.get('start')}-{object_.get('end')}"
    )


def score_candidates(
    relations: list[dict],
    schema: core.ExperimentSchema,
    grounding: SnapshotGrounding,
    chunk_spans: dict[int, tuple[int, int]],
) -> list[ScoredCandidate]:
    """Score every emitted candidate; deterministic ID order (span-sorted)."""
    provisional = [
        score_candidate(relation, schema, grounding, chunk_spans, claim_id="")
        for relation in relations
    ]
    provisional.sort(
        key=lambda c: (
            c.subject["start"],
            c.subject["end"],
            c.object["start"],
            c.object["end"],
            c.predicate,
        )
    )
    for index, candidate in enumerate(provisional, start=1):
        candidate.claim_id = f"C-{index:03d}"
    return provisional


def surface_duplicate_groups(candidates: list[ScoredCandidate]) -> list[dict]:
    """Accepted, citation-valid claims sharing a surface triple across
    distinct spans (the span-stable dedup in the harness keeps them apart)."""
    groups: dict[tuple, list[str]] = {}
    for candidate in candidates:
        if not (candidate.accepted and candidate.citation_valid):
            continue
        key = (
            norm_words(candidate.subject.get("text", "")).lower(),
            candidate.predicate,
            norm_words(candidate.object.get("text", "")).lower(),
        )
        groups.setdefault(key, []).append(candidate.claim_id)
    return [
        {
            "surface_key": [item for item in key],
            "claim_ids": claim_ids,
            "duplicate_count": len(claim_ids) - 1,
        }
        for key, claim_ids in sorted(groups.items())
        if len(claim_ids) > 1
    ]


def score_entity_mentions(
    entities: list[dict], grounding: SnapshotGrounding
) -> list[dict]:
    """Ground each deduplicated entity mention; record snapshot lines."""
    scored = []
    for entity in entities:
        span = grounding.ground(int(entity.get("start", -1)), int(entity.get("end", -1)))
        exact, normalized_equal = text_match(span, str(entity.get("text", "")))
        scored.append(
            {
                "kind": entity.get("kind"),
                "text": entity.get("text"),
                "start": span.start,
                "end": span.end,
                "lines": [span.start_line, span.end_line],
                "confidence": entity.get("confidence"),
                "occurrences": entity.get("occurrences"),
                "source_chunks": list(entity.get("source_chunks") or []),
                "resolved": span.resolved,
                "text_exact_match": exact,
                "text_normalized_match": normalized_equal,
                "codes": list(span.codes)
                + ([CODE_SPAN_TEXT_MISMATCH] if span.resolved and not exact else []),
            }
        )
    scored.sort(key=lambda e: (e["start"], e["end"], str(e["kind"])))
    return scored


def dropped_records(chunk_records: list[dict]) -> list[dict]:
    """Every chunk-level dropped relation, tagged with its failure category."""
    records = []
    for record in chunk_records:
        chunk = record.get("chunk") or {}
        for dropped in record.get("dropped") or []:
            records.append(
                {
                    "chunk_index": chunk.get("index"),
                    "reason": dropped.get("reason"),
                    "raw": dropped.get("raw"),
                    "category": DROPPED_CATEGORY,
                }
            )
    return records


def run_peak_memory_bytes(manifest: dict) -> tuple[Optional[int], str]:
    """Peak-memory field from the run manifest, if the runner recorded one.

    Tasks 5-7 live runs may record `manifest.peak_memory_bytes` or
    `manifest.resources.peak_memory_bytes` (torch + RSS); nothing here
    fabricates a value when the field is absent.
    """
    value = manifest.get("peak_memory_bytes")
    if value is None:
        resources = manifest.get("resources") or {}
        value = resources.get("peak_memory_bytes")
    if value is None:
        return None, (
            "run manifest records no peak memory; Tasks 5-7 live runners should "
            "record manifest.peak_memory_bytes (or resources.peak_memory_bytes)"
        )
    try:
        return int(value), "from run manifest"
    except (TypeError, ValueError):
        return None, f"run manifest peak memory value {value!r} is not an integer"
