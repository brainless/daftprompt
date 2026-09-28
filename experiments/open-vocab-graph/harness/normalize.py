"""Normalization of raw GLiNER2.5 chunk output into typed, span-stable candidates.

Pure stdlib (no gliner2/torch imports) so tests run without loading a
checkpoint. Responsibilities:

- remap chunk-local spans to document offsets (via `chunking.remap_span`) and
  verify each mention against the snapshot text;
- map model labels to canonical predicates (`uses__2` -> `uses`) using the
  schema's JointIE label mapping, or the 1:1 mapping for the independent
  variant;
- validate every relation claim against the schema's declared endpoint pairs
  (`schema.core.validate_claim`) and record rejected candidates with reason
  codes instead of dropping them;
- deduplicate repeated mentions and chunk-overlap copies by stable spans —
  entity key `(kind, doc_start, doc_end)`, relation key
  `(canonical_predicate, subject_span_key, object_span_key)` — keeping the
  highest-confidence copy while distinct spans stay distinct, even when the
  surface text is identical.

Raw input shapes (produced by `adapter`):

- jointie: `{"entities": [{"id","type","text","start","end","confidence"?}],
  "relations": [{"type","head","tail","confidence"?}], "feasible": bool}`
  (JointResult.to_dict plus `feasible`; spans are chunk-local);
- independent: the verbatim dict from `ExtractorRuntimeMixin.extract` with
  `include_spans=True`/`include_confidence=True`:
  `{"entities": {kind: [{"text","start","end","confidence"?}]},
  "relation_extraction": {label: [{"head": {...}, "tail": {...}}]}}`.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Optional

from schema import core

from .chunking import Chunk, remap_span


@dataclass(frozen=True)
class NormalizedEntity:
    """One entity mention at document-level offsets from a single chunk."""

    kind: str
    text: str
    start: int
    end: int
    confidence: Optional[float]
    chunk_index: int
    span_verified: bool
    notes: tuple[str, ...] = ()

    @property
    def span_key(self) -> tuple[str, int, int]:
        return (self.kind, self.start, self.end)

    def to_dict(self) -> dict:
        return {
            "kind": self.kind,
            "text": self.text,
            "start": self.start,
            "end": self.end,
            "confidence": self.confidence,
            "chunk_index": self.chunk_index,
            "span_verified": self.span_verified,
            "notes": list(self.notes),
        }


@dataclass(frozen=True)
class NormalizedRelation:
    """One typed subject-predicate-object candidate from a single chunk."""

    predicate: str
    model_label: str
    subject: NormalizedEntity
    object: NormalizedEntity
    confidence: Optional[float]
    chunk_index: int
    span_verified: bool
    verdict: dict
    notes: tuple[str, ...] = ()

    @property
    def key(self) -> tuple:
        return (self.predicate, self.subject.span_key, self.object.span_key)

    def to_dict(self) -> dict:
        return {
            "predicate": self.predicate,
            "model_label": self.model_label,
            "subject": self.subject.to_dict(),
            "object": self.object.to_dict(),
            "confidence": self.confidence,
            "chunk_index": self.chunk_index,
            "span_verified": self.span_verified,
            "verdict": dict(self.verdict),
            "notes": list(self.notes),
        }


@dataclass(frozen=True)
class MergedEntity:
    """A deduplicated entity mention; one row per stable span."""

    kind: str
    text: str
    start: int
    end: int
    confidence: Optional[float]
    occurrences: int
    source_chunks: tuple[int, ...]
    span_verified: bool

    @property
    def span_key(self) -> tuple[str, int, int]:
        return (self.kind, self.start, self.end)

    def to_dict(self) -> dict:
        return {
            "kind": self.kind,
            "text": self.text,
            "start": self.start,
            "end": self.end,
            "confidence": self.confidence,
            "occurrences": self.occurrences,
            "source_chunks": list(self.source_chunks),
            "span_verified": self.span_verified,
        }


@dataclass(frozen=True)
class MergedRelation:
    """A deduplicated relation candidate; one row per stable endpoint spans."""

    predicate: str
    model_label: str
    subject: MergedEntity
    object: MergedEntity
    confidence: Optional[float]
    occurrences: int
    source_chunks: tuple[int, ...]
    span_verified: bool
    verdict: dict

    def to_dict(self) -> dict:
        return {
            "predicate": self.predicate,
            "model_label": self.model_label,
            "subject": self.subject.to_dict(),
            "object": self.object.to_dict(),
            "confidence": self.confidence,
            "occurrences": self.occurrences,
            "source_chunks": list(self.source_chunks),
            "span_verified": self.span_verified,
            "verdict": dict(self.verdict),
        }


def chunk_status(raw: dict, variant: str) -> str:
    """Classify a raw chunk result. Empty and infeasible chunks stay visible."""
    if variant == "jointie":
        if raw.get("feasible") is False:
            return "infeasible"
        entities = raw.get("entities") or []
        relations = raw.get("relations") or []
        return "ok" if entities or relations else "empty"
    entities = raw.get("entities") or {}
    relations = raw.get("relation_extraction") or {}
    has_content = any(entities.values()) or any(relations.values())
    return "ok" if has_content else "empty"


def _verdict_dict(verdict: core.Verdict) -> dict:
    return {
        "accepted": bool(verdict.accepted),
        "codes": list(verdict.codes),
        "details": list(verdict.details),
    }


def _confidence(value: Any) -> Optional[float]:
    if value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def _entity_from_raw(
    raw_entity: dict,
    chunk: Chunk,
    document_text: str,
    default_kind: str = "",
) -> tuple[NormalizedEntity, list[str]]:
    notes: list[str] = []
    kind = str(raw_entity.get("type") or raw_entity.get("kind") or default_kind or "")
    text = str(raw_entity.get("text", ""))
    local_start = int(raw_entity.get("start", 0))
    local_end = int(raw_entity.get("end", 0))
    doc_start, doc_end, clamped, verified = remap_span(
        chunk, local_start, local_end, document_text, mention_text=text or None
    )
    if clamped:
        notes.append(
            f"span ({local_start}, {local_end}) escaped chunk input; clamped to "
            f"({doc_start}, {doc_end})"
        )
    if text and not verified:
        notes.append(
            f"remapped span text mismatch: snapshot slice "
            f"{document_text[doc_start:doc_end]!r} != mention {text!r}"
        )
    return (
        NormalizedEntity(
            kind=kind,
            text=text,
            start=doc_start,
            end=doc_end,
            confidence=_confidence(raw_entity.get("confidence")),
            chunk_index=chunk.index,
            span_verified=verified,
            notes=tuple(notes),
        ),
        notes,
    )


def _claim_verdict(
    schema: core.ExperimentSchema,
    predicate: str,
    model_label: str,
    subject: NormalizedEntity,
    object_: NormalizedEntity,
) -> dict:
    claim = core.Claim(
        subject_text=subject.text,
        subject_kind=subject.kind,
        predicate=predicate,
        object_text=object_.text,
        object_kind=object_.kind,
        model_label=model_label,
    )
    return _verdict_dict(core.validate_claim(schema, claim))


def _jointie_labels(schema: core.ExperimentSchema) -> dict[str, str]:
    return {
        spec.label: spec.canonical_predicate for spec in schema.joint_ie_labels().values()
    }


def normalize_jointie_chunk(
    raw: dict,
    chunk: Chunk,
    schema: core.ExperimentSchema,
    document_text: str,
) -> tuple[list[NormalizedEntity], list[NormalizedRelation], list[str], list[dict]]:
    """Normalize one JointIE chunk result.

    Returns `(entities, relations, notes, dropped)`. A relation whose endpoint
    id is missing from the chunk's entity list can never be grounded, so it is
    moved to `dropped` with its raw record — recorded, never silently hidden.
    """
    label_map = _jointie_labels(schema)
    notes: list[str] = []
    dropped: list[dict] = []
    entities: list[NormalizedEntity] = []
    by_id: dict[str, NormalizedEntity] = {}
    for raw_entity in raw.get("entities") or []:
        entity, entity_notes = _entity_from_raw(raw_entity, chunk, document_text)
        notes.extend(entity_notes)
        entities.append(entity)
        entity_id = raw_entity.get("id")
        if entity_id is not None:
            by_id[str(entity_id)] = entity

    relations: list[NormalizedRelation] = []
    for raw_relation in raw.get("relations") or []:
        label = str(raw_relation.get("type") or raw_relation.get("label") or "")
        predicate = label_map.get(label, label)
        head_id = str(raw_relation.get("head", ""))
        tail_id = str(raw_relation.get("tail", ""))
        if head_id not in by_id or tail_id not in by_id:
            missing = [
                f"{name}={found}"
                for name, found in (("head", head_id), ("tail", tail_id))
                if found not in by_id
            ]
            dropped.append(
                {
                    "reason": f"relation endpoint(s) {', '.join(missing)} not present in chunk entities",
                    "raw": dict(raw_relation),
                }
            )
            continue
        subject = by_id[head_id]
        object_ = by_id[tail_id]
        relation_notes = list(subject.notes) + list(object_.notes)
        relations.append(
            NormalizedRelation(
                predicate=predicate,
                model_label=label,
                subject=subject,
                object=object_,
                confidence=_confidence(raw_relation.get("confidence")),
                chunk_index=chunk.index,
                span_verified=subject.span_verified and object_.span_verified,
                verdict=_claim_verdict(schema, predicate, label, subject, object_),
                notes=tuple(relation_notes),
            )
        )
    return entities, relations, notes, dropped


def normalize_independent_chunk(
    raw: dict,
    chunk: Chunk,
    schema: core.ExperimentSchema,
    document_text: str,
) -> tuple[list[NormalizedEntity], list[NormalizedRelation], list[str], list[dict]]:
    """Normalize one independent `extract_relations` chunk result.

    The independent relation head carries no endpoint typing, so endpoint kinds
    are attached by matching each endpoint's remapped span against the chunk's
    own entity mentions. Unmatched endpoints keep kind "" and the claim is
    recorded as a rejected candidate (unknown kind codes), never dropped.
    """
    label_map = schema.independent_labels()
    notes: list[str] = []
    dropped: list[dict] = []
    entities: list[NormalizedEntity] = []
    for kind, items in (raw.get("entities") or {}).items():
        for raw_entity in items or []:
            if not isinstance(raw_entity, dict) or "start" not in raw_entity or "end" not in raw_entity:
                dropped.append(
                    {
                        "reason": "entity mention without character offsets cannot be grounded",
                        "raw": raw_entity,
                    }
                )
                continue
            entity, entity_notes = _entity_from_raw(
                dict(raw_entity), chunk, document_text, default_kind=str(kind)
            )
            notes.extend(entity_notes)
            entities.append(entity)

    kind_by_span: dict[tuple[int, int], str] = {}
    for entity in entities:
        kind_by_span.setdefault((entity.start, entity.end), entity.kind)

    def endpoint(raw_endpoint: Any) -> tuple[NormalizedEntity, list[str]]:
        if not isinstance(raw_endpoint, dict) or "start" not in raw_endpoint or "end" not in raw_endpoint:
            raise ValueError("relation endpoint without character offsets")
        entity, entity_notes = _entity_from_raw(dict(raw_endpoint), chunk, document_text)
        if entity.kind == "":
            matched = kind_by_span.get((entity.start, entity.end))
            if matched is not None:
                entity = NormalizedEntity(
                    kind=matched,
                    text=entity.text,
                    start=entity.start,
                    end=entity.end,
                    confidence=entity.confidence,
                    chunk_index=entity.chunk_index,
                    span_verified=entity.span_verified,
                    notes=entity.notes + ("kind attached by span match to chunk entity",),
                )
            else:
                entity = NormalizedEntity(
                    kind="",
                    text=entity.text,
                    start=entity.start,
                    end=entity.end,
                    confidence=entity.confidence,
                    chunk_index=entity.chunk_index,
                    span_verified=entity.span_verified,
                    notes=entity.notes + ("no chunk entity mention at this span; kind unknown",),
                )
        return entity, entity_notes

    relations: list[NormalizedRelation] = []
    for label, items in (raw.get("relation_extraction") or {}).items():
        predicate = label_map.get(str(label), str(label))
        for raw_relation in items or []:
            try:
                subject, subject_notes = endpoint(raw_relation.get("head"))
                object_, object_notes = endpoint(raw_relation.get("tail"))
            except ValueError as exc:
                dropped.append({"reason": str(exc), "raw": raw_relation})
                continue
            notes.extend(subject_notes + object_notes)
            confidence = _confidence(raw_relation.get("confidence"))
            if confidence is None:
                head_conf = subject.confidence
                tail_conf = object_.confidence
                if head_conf is not None and tail_conf is not None:
                    confidence = head_conf * tail_conf
                    notes.append(
                        "relation confidence derived from endpoint confidences "
                        "(independent head emits none)"
                    )
            relations.append(
                NormalizedRelation(
                    predicate=predicate,
                    model_label=str(label),
                    subject=subject,
                    object=object_,
                    confidence=confidence,
                    chunk_index=chunk.index,
                    span_verified=subject.span_verified and object_.span_verified,
                    verdict=_claim_verdict(schema, predicate, str(label), subject, object_),
                    notes=tuple(subject_notes + object_notes),
                )
            )
    return entities, relations, notes, dropped


def normalize_chunk(raw: dict, chunk: Chunk, schema, document_text: str, variant: str):
    if variant == "jointie":
        return normalize_jointie_chunk(raw, chunk, schema, document_text)
    if variant == "independent":
        return normalize_independent_chunk(raw, chunk, schema, document_text)
    raise ValueError(f"unknown variant {variant!r}; expected 'jointie' or 'independent'")


def merge_entities(entities: list[NormalizedEntity]) -> list[MergedEntity]:
    """Deduplicate by stable span `(kind, start, end)`; distinct spans stay distinct."""
    best: dict[tuple[str, int, int], MergedEntity] = {}
    for entity in entities:
        key = entity.span_key
        previous = best.get(key)
        if previous is None:
            best[key] = MergedEntity(
                kind=entity.kind,
                text=entity.text,
                start=entity.start,
                end=entity.end,
                confidence=entity.confidence,
                occurrences=1,
                source_chunks=(entity.chunk_index,),
                span_verified=entity.span_verified,
            )
            continue
        confidence = entity.confidence
        if confidence is not None and (previous.confidence is None or confidence > previous.confidence):
            kept_confidence = confidence
        else:
            kept_confidence = previous.confidence
        best[key] = MergedEntity(
            kind=previous.kind,
            text=previous.text,
            start=previous.start,
            end=previous.end,
            confidence=kept_confidence,
            occurrences=previous.occurrences + 1,
            source_chunks=tuple(sorted(set(previous.source_chunks) | {entity.chunk_index})),
            span_verified=previous.span_verified and entity.span_verified,
        )
    return sorted(
        best.values(),
        key=lambda item: (item.start, item.end, item.kind),
    )


def merge_relations(relations: list[NormalizedRelation]) -> list[MergedRelation]:
    """Deduplicate by `(predicate, subject span, object span)`; keep max confidence."""
    entity_index: dict[tuple[str, int, int], MergedEntity] = {
        entity.span_key: entity for entity in merge_entities(
            [relation.subject for relation in relations]
            + [relation.object for relation in relations]
        )
    }
    best: dict[tuple, MergedRelation] = {}
    for relation in relations:
        key = relation.key
        previous = best.get(key)
        if previous is None:
            best[key] = MergedRelation(
                predicate=relation.predicate,
                model_label=relation.model_label,
                subject=entity_index[relation.subject.span_key],
                object=entity_index[relation.object.span_key],
                confidence=relation.confidence,
                occurrences=1,
                source_chunks=(relation.chunk_index,),
                span_verified=relation.span_verified,
                verdict=relation.verdict,
            )
            continue
        confidence = relation.confidence
        if confidence is not None and (previous.confidence is None or confidence > previous.confidence):
            kept_confidence = confidence
            kept_verdict = relation.verdict
            kept_label = relation.model_label
        else:
            kept_confidence = previous.confidence
            kept_verdict = previous.verdict
            kept_label = previous.model_label
        best[key] = MergedRelation(
            predicate=previous.predicate,
            model_label=kept_label,
            subject=previous.subject,
            object=previous.object,
            confidence=kept_confidence,
            occurrences=previous.occurrences + 1,
            source_chunks=tuple(sorted(set(previous.source_chunks) | {relation.chunk_index})),
            span_verified=previous.span_verified and relation.span_verified,
            verdict=kept_verdict,
        )
    return sorted(
        best.values(),
        key=lambda item: (
            item.predicate,
            item.subject.start,
            item.subject.end,
            item.object.start,
            item.object.end,
        ),
    )
