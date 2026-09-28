"""Epic 014 GLiNER2.5-compatible schema (Task 2).

Versioned artifact: gliner25-schema.json, regenerated deterministically by
build.py. See README.md for the mapping decisions and model-schema limitations.
"""
from .core import (
    AllowedPair,
    Claim,
    EntityKind,
    ExperimentSchema,
    JointIELabel,
    Predicate,
    Support,
    Verdict,
    derive_joint_ie_labels,
    interpret_example_triples,
    load_schema,
    to_joint_schema,
    validate_claim,
)
from .build import ARTIFACT_PATH, build_schema, main, serialize

__all__ = [
    "AllowedPair",
    "ARTIFACT_PATH",
    "build_schema",
    "Claim",
    "derive_joint_ie_labels",
    "EntityKind",
    "ExperimentSchema",
    "interpret_example_triples",
    "JointIELabel",
    "load_schema",
    "main",
    "Predicate",
    "serialize",
    "Support",
    "to_joint_schema",
    "validate_claim",
    "Verdict",
]
