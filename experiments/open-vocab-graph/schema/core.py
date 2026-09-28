"""Epic 014 GLiNER2.5 experiment schema: loading, validation, model mapping.

The versioned artifact (`gliner25-schema.json`) is generated deterministically
by `build.py`. This module is the runtime API used by the tests and, later, by
the Task 3 extraction adapter and Task 4 evidence validator.
"""
from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Optional

HERE = Path(__file__).resolve().parent
ARTIFACT_PATH = HERE / "gliner25-schema.json"


@dataclass(frozen=True)
class Support:
    """One auditable source line backing a schema addition."""

    source: str
    line: int
    quote: str
    reason: str


@dataclass(frozen=True)
class EntityKind:
    name: str
    description: str
    origin: str  # "seed" | "added_task2"
    support: tuple[Support, ...] = ()


@dataclass(frozen=True)
class AllowedPair:
    subject_kind: str
    object_kind: str
    origin: str
    support: tuple[Support, ...] = ()


@dataclass(frozen=True)
class Predicate:
    name: str
    description: str
    origin: str
    allowed_pairs: tuple[AllowedPair, ...]

    def pair_is_allowed(self, subject_kind: str, object_kind: str) -> bool:
        return any(
            p.subject_kind == subject_kind and p.object_kind == object_kind
            for p in self.allowed_pairs
        )


@dataclass(frozen=True)
class JointIELabel:
    """One relation label as declared to gliner2.joint_ie.JointSchema."""

    label: str
    canonical_predicate: str
    head: tuple[str, ...]
    tail: tuple[str, ...]
    description: str


@dataclass(frozen=True)
class Claim:
    """A typed subject-predicate-object claim (mention text is provenance)."""

    subject_text: str
    subject_kind: str
    predicate: str
    object_text: str
    object_kind: str
    model_label: Optional[str] = None


@dataclass(frozen=True)
class Verdict:
    accepted: bool
    codes: tuple[str, ...]
    details: tuple[str, ...]


class ExperimentSchema:
    def __init__(self, data: Mapping[str, Any]):
        self.data = dict(data)
        self.schema_id: str = data["schema_id"]
        self.schema_version: str = data["schema_version"]
        self.entity_kinds: dict[str, EntityKind] = {}
        for raw in data["entity_kinds"]:
            self.entity_kinds[raw["kind"]] = EntityKind(
                name=raw["kind"],
                description=raw["description"],
                origin=raw["origin"],
                support=_parse_support(raw.get("support", ())),
            )
        self.predicates: dict[str, Predicate] = {}
        for raw in data["predicates"]:
            pairs = tuple(
                AllowedPair(
                    subject_kind=p["subject_kind"],
                    object_kind=p["object_kind"],
                    origin=p["origin"],
                    support=_parse_support(p.get("support", ())),
                )
                for p in raw["allowed_pairs"]
            )
            self.predicates[raw["predicate"]] = Predicate(
                name=raw["predicate"],
                description=raw["description"],
                origin=raw["origin"],
                allowed_pairs=pairs,
            )

    @property
    def model_mapping(self) -> Mapping[str, Any]:
        return self.data["model_mapping"]

    def pair_is_allowed(self, predicate: str, subject_kind: str, object_kind: str) -> bool:
        pred = self.predicates.get(predicate)
        return pred is not None and pred.pair_is_allowed(subject_kind, object_kind)

    def joint_ie_labels(self) -> dict[str, JointIELabel]:
        """Derive the JointIE relation-label mapping from the declared pairs."""
        return derive_joint_ie_labels(self.predicates)

    def independent_labels(self) -> dict[str, str]:
        """Canonical predicate -> label for untyped `extract_relations` runs."""
        return {name: name for name in self.predicates}

    def example_triples(self) -> tuple[Mapping[str, Any], ...]:
        return tuple(self.data["example_triples"])


def _parse_support(raw) -> tuple[Support, ...]:
    return tuple(
        Support(
            source=s["source"],
            line=int(s["line"]),
            quote=s["quote"],
            reason=s["reason"],
        )
        for s in raw
    )


def derive_joint_ie_labels(predicates: Mapping[str, Predicate]) -> dict[str, JointIELabel]:
    """Map multi-shape predicates onto JointIE relation labels.

    JointIE's typed endpoints are a Cartesian product: `TypedEndpoints.allows`
    accepts an edge iff head type is in head_types AND tail type is in
    tail_types (gliner2 joint_ie/constraints.py), and the candidate builder
    pairs roles with `itertools.product` (joint_ie/candidates.py). A predicate
    with several allowed endpoint pairs therefore maps as follows:

    - If the declared pairs equal the product of distinct head x distinct tail
      types, emit one relation label with multi-type head/tail.
    - Otherwise group the pairs by subject kind; each group is exactly the
      product of that one head type with the group's tail types, so it is
      losslessly declarable. The largest group (ties: alphabetical head) keeps
      the bare predicate name; the rest become `predicate__2`, `predicate__3`...

    Every emitted label's head x tail product stays inside the declared pairs,
    and the validator (validate_claim) enforces the canonical declared pairs,
    never the label-level product.

    Rejected alternative: one variant label per allowed pair. It is lossless
    too, but it scatters one predicate over more labels than necessary and
    fragments per-label model calibration; grouping by subject kind is the
    smallest deterministic split that never admits an undeclared pair.
    """
    labels: dict[str, JointIELabel] = {}
    for name, pred in predicates.items():
        declared = {(p.subject_kind, p.object_kind) for p in pred.allowed_pairs}
        heads = sorted({p.subject_kind for p in pred.allowed_pairs})
        tails = sorted({p.object_kind for p in pred.allowed_pairs})
        if len(declared) == len(heads) * len(tails):
            labels[name] = JointIELabel(
                label=name,
                canonical_predicate=name,
                head=tuple(heads),
                tail=tuple(tails),
                description=pred.description,
            )
            continue
        groups: dict[str, list[tuple[str, str]]] = {}
        for pair in pred.allowed_pairs:
            groups.setdefault(pair.subject_kind, []).append(
                (pair.subject_kind, pair.object_kind)
            )
        ordered = sorted(groups.items(), key=lambda item: (-len(item[1]), item[0]))
        for index, (head, group_pairs) in enumerate(ordered):
            label = name if index == 0 else f"{name}__{index + 1}"
            group_tails = sorted({tail for _, tail in group_pairs})
            description = pred.description
            if index > 0:
                description = (
                    f"{pred.description} (shape variant {index + 1}: "
                    f"{head} -> {'|'.join(group_tails)})"
                )
            labels[label] = JointIELabel(
                label=label,
                canonical_predicate=name,
                head=(head,),
                tail=tuple(group_tails),
                description=description,
            )
    return labels


def validate_claim(schema: ExperimentSchema, claim: Claim) -> Verdict:
    """Validate a typed claim against the schema's declared endpoint pairs.

    The exact (subject_kind, object_kind) pair must be declared for the
    predicate. A claim is never accepted because its endpoints individually
    appear in *some* allowed pairs (e.g. uses(Command, Subsystem) is rejected
    even though Command and Subsystem each occur in other declared pairs).
    Span containment, snapshot citation, and entailment are later layers
    (Task 4); this validator only enforces types and pairs.
    """
    codes: list[str] = []
    details: list[str] = []
    pred = schema.predicates.get(claim.predicate)
    if pred is None:
        codes.append("unknown_predicate")
        details.append(f"predicate {claim.predicate!r} is not declared in the schema")
    else:
        if claim.subject_kind not in schema.entity_kinds:
            codes.append("unknown_subject_kind")
            details.append(f"subject kind {claim.subject_kind!r} is not declared")
        if claim.object_kind not in schema.entity_kinds:
            codes.append("unknown_object_kind")
            details.append(f"object kind {claim.object_kind!r} is not declared")
        if not claim.subject_text.strip():
            codes.append("empty_subject_text")
            details.append("subject mention text is empty")
        if not claim.object_text.strip():
            codes.append("empty_object_text")
            details.append("object mention text is empty")
        if not codes and not pred.pair_is_allowed(claim.subject_kind, claim.object_kind):
            codes.append("disallowed_pair")
            declared = ", ".join(
                f"({p.subject_kind}, {p.object_kind})" for p in pred.allowed_pairs
            )
            details.append(
                f"pair ({claim.subject_kind}, {claim.object_kind}) is not declared "
                f"for {claim.predicate!r}; declared pairs: {declared}"
            )
    if codes:
        return Verdict(accepted=False, codes=tuple(codes), details=tuple(details))
    provenance = f" via model label {claim.model_label!r}" if claim.model_label else ""
    return Verdict(
        accepted=True,
        codes=("ok",),
        details=(f"{claim.subject_kind}({claim.subject_text}) -{claim.predicate}-> "
                 f"{claim.object_kind}({claim.object_text}){provenance}",),
    )


def interpret_example_triples(schema: ExperimentSchema):
    """Interpret the seed example triples as schema sanity checks.

    The seed examples are evaluation/sanity material only; they are never
    passed to the model as inference-time examples (the artifact records that
    policy).
    """
    results = []
    for example in schema.example_triples():
        claim = Claim(
            subject_text=example["subject"]["name"],
            subject_kind=example["subject"]["kind"],
            predicate=example["predicate"],
            object_text=example["object"]["name"],
            object_kind=example["object"]["kind"],
        )
        results.append((example, claim, validate_claim(schema, claim)))
    return results


def to_joint_schema(schema: ExperimentSchema):
    """Build a real gliner2.joint_ie.JointSchema from the artifact.

    Imports gliner2 lazily; importing builds no model and needs no download.
    """
    from gliner2.joint_ie import JointSchema

    joint = JointSchema()
    joint.entities({k.name: k.description for k in schema.entity_kinds.values()})
    for spec in schema.joint_ie_labels().values():
        joint.relation(
            spec.label,
            list(spec.head),
            list(spec.tail),
            description=spec.description,
        )
    return joint


def load_schema(path: Optional[Path] = None) -> ExperimentSchema:
    artifact = Path(path) if path else ARTIFACT_PATH
    with open(artifact, "r", encoding="utf-8") as handle:
        return ExperimentSchema(json.load(handle))
