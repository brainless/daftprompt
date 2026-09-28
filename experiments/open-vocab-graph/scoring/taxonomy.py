"""Failure taxonomy for Epic 014 scoring.

The epic requires these failure categories to be separable:

- `absent_schema_label`      the fact's predicate/kind had no schema label
                             (schema gap; measured per reference fact, and
                             per candidate when the model emitted a label
                             the schema never declared)
- `missing_entity`           an expected entity mention was never emitted
- `missing_relation`         entity mentions exist but no relation candidate
                             covers the fact
- `wrong_type`               endpoint kinds unknown or the pair disallowed
- `wrong_predicate`          the right endpoints were related under a
                             different predicate
- `wrong_direction`          endpoints reversed relative to the fact
- `boundary_loss`            span clamped/out of bounds/text mismatch/
                             cross-chunk locality loss
- `evidence_mismatch`        an accepted structurally-matching claim cites
                             lines that do not overlap the fact's expected
                             evidence
- `unsupported_but_accepted` citation-valid, structurally-matching claim that
                             human review marks misleading (the slot exists
                             deterministically; only review can fill it)

Pure constants + mapping helpers; no I/O.
"""
from __future__ import annotations

CATEGORY_ABSENT_SCHEMA_LABEL = "absent_schema_label"
CATEGORY_MISSING_ENTITY = "missing_entity"
CATEGORY_MISSING_RELATION = "missing_relation"
CATEGORY_WRONG_TYPE = "wrong_type"
CATEGORY_WRONG_PREDICATE = "wrong_predicate"
CATEGORY_WRONG_DIRECTION = "wrong_direction"
CATEGORY_BOUNDARY_LOSS = "boundary_loss"
CATEGORY_EVIDENCE_MISMATCH = "evidence_mismatch"
CATEGORY_UNSUPPORTED_BUT_ACCEPTED = "unsupported_but_accepted"

ALL_CATEGORIES = (
    CATEGORY_ABSENT_SCHEMA_LABEL,
    CATEGORY_MISSING_ENTITY,
    CATEGORY_MISSING_RELATION,
    CATEGORY_WRONG_TYPE,
    CATEGORY_WRONG_PREDICATE,
    CATEGORY_WRONG_DIRECTION,
    CATEGORY_BOUNDARY_LOSS,
    CATEGORY_EVIDENCE_MISMATCH,
    CATEGORY_UNSUPPORTED_BUT_ACCEPTED,
)

# Schema-verdict codes (schema/core.validate_claim) -> failure categories.
# Rejected alternative: mapping unknown_*_kind to absent_schema_label as well.
# A model kind outside the declared vocabulary is a typing error by the model
# (the schema declares the full kind list), while an undeclared *predicate*
# label means the fact's predicate had no schema label at all.
SCHEMA_CODE_CATEGORIES = {
    "unknown_predicate": CATEGORY_ABSENT_SCHEMA_LABEL,
    "unknown_subject_kind": CATEGORY_WRONG_TYPE,
    "unknown_object_kind": CATEGORY_WRONG_TYPE,
    "disallowed_pair": CATEGORY_WRONG_TYPE,
    "empty_subject_text": CATEGORY_BOUNDARY_LOSS,
    "empty_object_text": CATEGORY_BOUNDARY_LOSS,
}

# Grounding codes (scoring.grounding / scoring.accept) -> failure categories.
GROUNDING_CODE_CATEGORIES = {
    "span_out_of_bounds": CATEGORY_BOUNDARY_LOSS,
    "empty_span": CATEGORY_BOUNDARY_LOSS,
    "span_text_mismatch": CATEGORY_BOUNDARY_LOSS,
    "cross_chunk_locality": CATEGORY_BOUNDARY_LOSS,
}

# Chunk-record `dropped` entries (relation endpoint id absent from the
# chunk's entities) can never be grounded: a missing entity.
DROPPED_CATEGORY = CATEGORY_MISSING_ENTITY

# Review-settable support values; anything else keeps "unreviewed".
SUPPORT_VALUES = ("supported", "weak", "misleading")
SUPPORT_UNREVIEWED = "unreviewed"
SUPPORT_SUPPORTED = "supported"
SUPPORT_WEAK = "weak"
SUPPORT_MISLEADING = "misleading"

# Fact diagnosis statuses.
STATUS_SCHEMA_GAP = "schema_gap"
STATUS_MISSING_ENTITY = "missing_entity"
STATUS_MISSING_RELATION = "missing_relation"
STATUS_ATTEMPTED_REJECTED = "attempted_rejected"
STATUS_MATCHED_EVIDENCE_MISMATCH = "matched_evidence_mismatch"
STATUS_MATCHED_PENDING_REVIEW = "matched_pending_review"
STATUS_RECOVERED = "recovered"

# Statuses that count the reference fact as an omission (no accepted,
# evidence-overlapping claim exists for it).
OMISSION_STATUSES = (
    STATUS_SCHEMA_GAP,
    STATUS_MISSING_ENTITY,
    STATUS_MISSING_RELATION,
    STATUS_ATTEMPTED_REJECTED,
    STATUS_MATCHED_EVIDENCE_MISMATCH,
)


def categories_for_codes(codes) -> tuple[str, ...]:
    """Map candidate-level codes to failure categories (deterministic order)."""
    seen: list[str] = []
    for code in codes:
        category = SCHEMA_CODE_CATEGORIES.get(code) or GROUNDING_CODE_CATEGORIES.get(code)
        if category is not None and category not in seen:
            seen.append(category)
    return tuple(seen)
