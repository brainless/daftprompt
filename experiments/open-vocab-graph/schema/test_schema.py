"""Tests for the Epic 014 GLiNER2.5 schema (Task 2).

Runs with plain asserts and no model download:

    .venv/bin/python experiments/open-vocab-graph/schema/test_schema.py

The function names are pytest-compatible, so `pytest` also works if installed.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import build  # noqa: E402
import core  # noqa: E402

FROZEN = HERE.parent / "frozen-inputs"
SEED_PATH = FROZEN / "seed-vocabulary.json"
SNAPSHOT_PATH = FROZEN / "AGENTS.md"

ARTIFACT = HERE / "gliner25-schema.json"


def _claim(subject, subject_kind, predicate, obj, object_kind, label=None):
    return core.Claim(
        subject_text=subject,
        subject_kind=subject_kind,
        predicate=predicate,
        object_text=obj,
        object_kind=object_kind,
        model_label=label,
    )


def test_artifact_is_deterministic():
    first = build.serialize(build.build_schema())
    second = build.serialize(build.build_schema())
    assert first == second, "generator output must be byte-identical across runs"
    assert ARTIFACT.read_text(encoding="utf-8") == first, (
        "checked-in gliner25-schema.json does not match the deterministic generator "
        "(run: python3 build.py)"
    )


def test_frozen_input_hash_tripwires():
    seed = SEED_PATH.read_bytes()
    snapshot = SNAPSHOT_PATH.read_bytes()
    build.build_schema(seed_bytes=seed, snapshot_bytes=snapshot)  # sanity: passes
    for tampered, kwargs in (
        ("seed", dict(seed_bytes=seed + b" ", snapshot_bytes=snapshot)),
        ("snapshot", dict(seed_bytes=seed, snapshot_bytes=snapshot + b" ")),
    ):
        try:
            build.build_schema(**kwargs)
        except ValueError as error:
            assert "drift" in str(error), f"{tampered} drift must fail closed"
        else:
            raise AssertionError(f"tampered {tampered} must be rejected")


def test_seed_derived_versus_added_labels():
    schema = core.load_schema()
    seed = json.loads(SEED_PATH.read_text(encoding="utf-8"))
    seed_kinds = {k["kind"] for k in seed["entity_kinds"]}
    seed_predicates = {p["predicate"] for p in seed["predicates"]}

    assert set(schema.entity_kinds) == seed_kinds | {
        "Application", "Subsystem", "ProcessRole", "FileExtensionSet", "PathSet",
        "Feature", "TechnologyPair", "FailureMode", "FallbackBehavior",
    }
    for kind in schema.entity_kinds.values():
        if kind.name in seed_kinds:
            assert kind.origin == "seed"
            seed_desc = next(k["description"] for k in seed["entity_kinds"] if k["kind"] == kind.name)
            assert kind.description == seed_desc, "seed descriptions must pass through unchanged"
        else:
            assert kind.origin == "added_task2"
            assert kind.support, f"added kind {kind.name} must record supporting source lines"
    for pred in schema.predicates.values():
        if pred.name in seed_predicates:
            assert pred.origin == "seed"
        else:
            assert pred.origin == "added_task2"
        for pair in pred.allowed_pairs:
            if pred.name in seed_predicates and (pair.subject_kind, pair.object_kind) in {
                (p["subject_kind"], p["object_kind"]) for p in seed["predicates"] if p["predicate"] == pred.name
            }:
                assert pair.origin == "seed"
            else:
                assert pair.origin == "added_task2"
                assert pair.support, f"added pair {pred.name}({pair.subject_kind}, {pair.object_kind}) must record support"
    assert schema.data["example_triples_role"] == "schema_sanity_checks_only"
    assert schema.data["seed"]["xxh3"] == "c776d7e579d8aed5"
    assert schema.data["schema_version"] == "1.0.0"


def test_support_quotes_resolve_to_snapshot():
    schema = core.load_schema()
    snapshot_lines = SNAPSHOT_PATH.read_text(encoding="utf-8").splitlines()
    supports = [s for k in schema.entity_kinds.values() for s in k.support]
    for pred in schema.predicates.values():
        for pair in pred.allowed_pairs:
            supports.extend(pair.support)
    assert supports, "additions must carry support entries"
    for support in supports:
        assert support.source == "frozen-inputs/AGENTS.md"
        assert 1 <= support.line <= len(snapshot_lines)
        assert support.quote == snapshot_lines[support.line - 1], (
            f"support quote on line {support.line} must be the exact snapshot line"
        )
        assert support.reason.strip()


def test_validator_accepts_declared_pairs():
    schema = core.load_schema()
    accepted = [
        # the motivating Epic 014 fact (line 7)
        _claim("daftprompt", "Application", "uses", "akar", "Tool"),
        _claim("daftprompt", "Application", "uses", "gitoxide", "Tool"),
        _claim("daftprompt", "Application", "uses",
               "hybrid search indexer (SQLite FTS5 + sqlite-vec embeddings)", "Subsystem"),
        _claim("graph build", "Command", "uses", "MIMO_V2_5_PRO model", "Tool"),
        # seed shapes
        _claim("graph build", "Command", "requires", "XIAOMI_API_KEY", "Configuration"),
        _claim("graph search", "Command", "searches", "git log", "DataSource"),
        _claim(".env file", "Configuration", "provided_by", "dotenvy", "Tool"),
        # added shapes (reference facts R4-R9)
        _claim("cargo check --workspace", "Command", "serves_as", "the canonical correctness gate", "ProcessRole"),
        _claim("code indexer", "Subsystem", "indexes", ".rs, .ts, .tsx, .js, .jsx", "FileExtensionSet"),
        _claim("code indexer file discovery", "Subsystem", "excludes", "node_modules/, vendor/, *.min.js", "PathSet"),
        _claim("hybrid search", "Feature", "fuses", "FTS5 (keyword) + sqlite-vec (vector KNN)", "TechnologyPair"),
        _claim("embedding model load failure", "FailureMode", "degrades_to", "FTS5-only search", "FallbackBehavior"),
    ]
    for claim in accepted:
        verdict = core.validate_claim(schema, claim)
        assert verdict.accepted, f"{claim} must be accepted, got {verdict}"
        assert verdict.codes == ("ok",)


def test_validator_rejects_disallowed_pairs():
    schema = core.load_schema()
    rejected = [
        # the endpoint-membership trap: Command and Subsystem each appear in
        # some allowed `uses` pair, but (Command, Subsystem) is not declared
        (_claim("graph build", "Command", "uses", "hybrid search indexer", "Subsystem"),
         "disallowed_pair"),
        # reversed direction
        (_claim("akar", "Tool", "uses", "daftprompt", "Application"), "disallowed_pair"),
        # case-sensitive kinds: lowercase "application" is not declared
        (_claim("akar", "Tool", "uses", "daftprompt", "application"), "unknown_object_kind"),
        (_claim("daftprompt", "application", "uses", "akar", "Tool"), "unknown_subject_kind"),
        # wrong pair for the predicate
        (_claim("daftprompt", "Application", "requires", "akar", "Tool"), "disallowed_pair"),
        (_claim("akar", "Tool", "uses", "akar", "Tool"), "disallowed_pair"),
        # unknown predicate and kinds
        (_claim("daftprompt", "Application", "owns", "akar", "Tool"), "unknown_predicate"),
        (_claim("daftprompt", "Program", "uses", "akar", "Tool"), "unknown_subject_kind"),
        (_claim("daftprompt", "Application", "uses", "akar", "Library"), "unknown_object_kind"),
        # empty mention text
        (_claim("", "Application", "uses", "akar", "Tool"), "empty_subject_text"),
        (_claim("daftprompt", "Application", "uses", "  ", "Tool"), "empty_object_text"),
    ]
    for claim, expected_code in rejected:
        verdict = core.validate_claim(schema, claim)
        assert not verdict.accepted, f"{claim} must be rejected"
        assert expected_code in verdict.codes, f"{claim}: expected {expected_code} in {verdict}"


def test_joint_ie_label_mapping():
    schema = core.load_schema()
    labels = schema.joint_ie_labels()
    artifact_labels = schema.model_mapping["joint_ie"]["labels"]

    # multi-shape predicate: grouped by subject kind, lossless, no extras
    uses_labels = [s for s in labels.values() if s.canonical_predicate == "uses"]
    assert [s.label for s in uses_labels] == ["uses", "uses__2"]
    assert uses_labels[0].head == ("Application",) and uses_labels[0].tail == ("Subsystem", "Tool")
    assert uses_labels[1].head == ("Command",) and uses_labels[1].tail == ("Tool",)

    declared = {(p.subject_kind, p.object_kind) for p in schema.predicates["uses"].allowed_pairs}
    covered: set[tuple[str, str]] = set()
    for spec in uses_labels:
        product = {(h, t) for h in spec.head for t in spec.tail}
        assert product <= declared, f"label {spec.label} must not admit undeclared pairs"
        covered |= product
    assert covered == declared, "every declared pair must be reachable through some label"

    # every predicate maps; product-expressible predicates keep the bare name
    canonical = {s.canonical_predicate for s in labels.values()}
    assert canonical == set(schema.predicates)
    for name in ("requires", "searches", "provided_by", "serves_as", "indexes",
                 "excludes", "fuses", "degrades_to"):
        spec = labels[name]
        assert spec.label == name
        assert len(spec.head) >= 1 and len(spec.tail) >= 1

    # the artifact embeds the same mapping the loader derives
    derived = {s.label: {"canonical_predicate": s.canonical_predicate, "head": list(s.head),
                         "tail": list(s.tail), "description": s.description}
               for s in labels.values()}
    assert derived == artifact_labels


def test_joint_schema_builds_offline():
    schema = core.load_schema()
    try:
        joint = core.to_joint_schema(schema)
    except ImportError:
        raise AssertionError("gliner2 2.0.0 is installed; JointSchema must build offline")
    specs = {s.name: s for s in joint.relation_specs}
    for spec in schema.joint_ie_labels().values():
        assert spec.label in specs
        compiled = specs[spec.label]
        assert compiled.head == spec.head and compiled.tail == spec.tail
        assert compiled.description == spec.description
    entity_names = {s.name for s in joint.entity_specs}
    assert entity_names == set(schema.entity_kinds)
    uses2 = specs["uses__2"]
    assert uses2.head == ("Command",) and uses2.tail == ("Tool",)


def test_per_relation_thresholds_pass_to_joint_schema():
    """Task 4: the five zero-emission predicates carry a lowered decision
    threshold that `to_joint_schema` forwards to `JointSchema.relation(...)`;
    everything else keeps gliner2's default bar. No model download."""
    schema = core.load_schema()
    tuned = ["serves_as", "indexes", "excludes", "fuses", "degrades_to"]
    for name in tuned:
        pred = schema.predicates[name]
        assert pred.threshold == 0.25, f"{name} must carry the Task 4 threshold"
    assert schema.predicates["uses"].threshold is None, (
        "untuned predicates must keep the gliner2 default (None = 0.5 bar)"
    )
    assert schema.predicates["requires"].threshold is None

    labels = schema.joint_ie_labels()
    for name in tuned:
        assert labels[name].threshold == 0.25, f"label {name} must inherit its predicate's threshold"

    joint = core.to_joint_schema(schema)
    specs = {s.name: s for s in joint.relation_specs}
    for name in tuned:
        assert getattr(specs[name], "threshold", None) == 0.25, (
            f"built JointSchema relation {name} must carry threshold 0.25, "
            f"got {getattr(specs[name], 'threshold', None)!r}"
        )
    assert getattr(specs["uses"], "threshold", None) is None
    assert getattr(specs["uses__2"], "threshold", None) is None


def test_task4_description_revisions_are_grounded():
    """Task 4: the five rewritten descriptions keep origin-style provenance —
    the pre-rewrite text plus the snapshot lines the revision was grounded
    against, each quote the exact frozen line."""
    schema = core.load_schema()
    snapshot_lines = SNAPSHOT_PATH.read_text(encoding="utf-8").splitlines()
    tuned = ["serves_as", "indexes", "excludes", "fuses", "degrades_to"]
    for name in tuned:
        pred = schema.predicates[name]
        assert pred.description_origin == "revised_task4", name
        assert pred.description_previous, f"{name} must keep the pre-rewrite description"
        assert pred.description != pred.description_previous, name
        assert pred.description_support, f"{name} must record grounding lines"
        for support in pred.description_support:
            assert support.source == "frozen-inputs/AGENTS.md"
            assert 1 <= support.line <= len(snapshot_lines)
            assert support.quote == snapshot_lines[support.line - 1], (
                f"{name} description support on line {support.line} must be the "
                "exact snapshot line"
            )
            assert support.reason.strip()
    # untouched predicates keep their Task 2 provenance only
    assert schema.predicates["uses"].description_origin is None
    assert schema.predicates["uses"].description_previous is None
    assert schema.predicates["uses"].description_support == ()


def test_independent_relation_labels():
    schema = core.load_schema()
    labels = schema.independent_labels()
    assert labels == {name: name for name in schema.predicates}
    assert labels == schema.model_mapping["independent_relation_labels"]


def test_example_triples_are_valid_sanity_checks():
    schema = core.load_schema()
    results = core.interpret_example_triples(schema)
    assert len(results) == 3
    for example, claim, verdict in results:
        assert verdict.accepted, f"seed example must validate under the schema: {example}"
    # the interpretation must use the example's recorded kinds
    assert all(claim.subject_kind == ex["subject"]["kind"] for (ex, claim, _) in results)
    assert all(claim.object_kind == ex["object"]["kind"] for (ex, claim, _) in results)


def test_daftprompt_is_application_and_akar_is_tool():
    schema = core.load_schema()
    assert "Application" in schema.entity_kinds
    assert "Tool" in schema.entity_kinds
    verdict = core.validate_claim(
        schema, _claim("daftprompt", "Application", "uses", "akar", "Tool")
    )
    assert verdict.accepted
    # the recorded budget-250 failure shape must stay rejected
    assert not core.validate_claim(
        schema, _claim("daftprompt", "Tool", "uses", "akar", "Tool")
    ).accepted


def main() -> int:
    tests = [(name, fn) for name, fn in sorted(globals().items())
             if name.startswith("test_") and callable(fn)]
    failures = 0
    for name, fn in tests:
        try:
            fn()
        except AssertionError as error:
            failures += 1
            print(f"FAIL {name}: {error}")
        else:
            print(f"ok   {name}")
    print(f"\n{len(tests) - failures}/{len(tests)} passed")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
