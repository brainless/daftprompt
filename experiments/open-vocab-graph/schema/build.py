"""Deterministic generator for the Epic 014 GLiNER2.5 experiment schema.

Reads the frozen seed (Epic 013 consolidation output, Task 1) plus the frozen
AGENTS.md snapshot, merges auditable Task 2 additions (Application, Subsystem,
new predicates, extra `uses` endpoint pairs), derives the JointIE label
mapping via `core.derive_joint_ie_labels`, and writes `gliner25-schema.json`.

Determinism: identical frozen inputs produce byte-identical output (no
timestamps, fixed insertion order, fixed JSON formatting). The build fails
closed if the frozen inputs' sha256 drifts or if any recorded support quote
no longer matches the snapshot line it cites.

Usage:
    python3 build.py           # regenerate gliner25-schema.json
    python3 build.py --check   # verify the checked-in artifact is current
"""
from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

try:
    from . import core  # package usage
except ImportError:
    import core  # plain script usage

HERE = Path(__file__).resolve().parent
FROZEN = HERE.parent / "frozen-inputs"
ARTIFACT_PATH = HERE / "gliner25-schema.json"

SEED_PATH = FROZEN / "seed-vocabulary.json"
SNAPSHOT_PATH = FROZEN / "AGENTS.md"

SEED_SHA256 = "4be96db29417a9bef53dc022c72fe14ca1ea3343ebb6482d2ff0e0a6e82e68f6"
SEED_XXH3 = "c776d7e579d8aed5"
SNAPSHOT_SHA256 = "a15dbdb069bcb58542f49bab0775004e62e792b52939aecd2c77886f33567423"

# Epic 014's checked-in artifact sha256. This Epic 015 Task 4 revision
# supersedes it; the relationship is recorded in the artifact's `revision`
# block and named by the task4 run label so run manifests carry the note.
SUPERSEDED_ARTIFACT_SHA256 = "366fa7e59096df4cad61d2306a49b259cf7b5632260861a1c5cc9d5c76136bbc"

SCHEMA_ID = "daftprompt-gliner25-experiment"
SCHEMA_VERSION = "1.0.0"

# Task 2 additions. Every support entry is (line_number, excerpt, reason); the
# generator reads the full physical line from the frozen snapshot as `quote`
# and fails if the excerpt is not contained in it.
KIND_ADDITIONS = [
    (
        "Application",
        "The daftprompt application itself or another named software application "
        "(an app, not an individual CLI command) that employs tools, subsystems, "
        "and commands.",
        [
            (3, "working on daftprompt", "names daftprompt as the subject software of this repository"),
            (7, "daftprompt is a Rust app", "states daftprompt is a Rust app that uses akar, gitoxide, and the hybrid search indexer"),
            (141, "daftprompt owns application state", "describes daftprompt as an application-level actor that owns application state"),
        ],
    ),
    (
        "Subsystem",
        "A named internal component, pipeline, or module of an application "
        "(e.g., the hybrid search indexer, the code indexer file-discovery path).",
        [
            (7, "a hybrid search indexer (SQLite FTS5 + sqlite-vec embeddings)", "names the hybrid search indexer as a component combined into the app"),
            (103, "git commit reader (gitoxide)", "names git_log.rs as the git commit reader module"),
            (127, "code_files`/`vec_code` pipeline", "names the shared code indexing pipeline"),
        ],
    ),
    (
        "ProcessRole",
        "A role or function that a command serves in the development workflow "
        "(e.g., the canonical correctness gate).",
        [
            (81, "canonical correctness gate", "names the process role served by cargo check --workspace"),
        ],
    ),
    (
        "FileExtensionSet",
        "A set of file extensions that a pipeline processes "
        "(e.g., .rs, .ts, .tsx, .js, .jsx).",
        [
            (127, "`.rs`, `.ts`, `.tsx`, `.js`, and `.jsx` files", "the extension set indexed by the shared code pipeline"),
        ],
    ),
    (
        "PathSet",
        "A set of directory or file patterns that a process excludes "
        "(e.g., node_modules/, vendor/, dist/, build/, .next/, coverage/, *.min.js).",
        [
            (127, "is_excluded_generated_path", "the generated/vendor/minified exclusion set enforced by the shared discovery path"),
        ],
    ),
    (
        "Feature",
        "A user-facing behavior or capability of an application "
        "(e.g., hybrid search, unified search).",
        [
            (92, "Hybrid search", "architecture-diagram row naming hybrid search as a feature"),
            (94, "Unified search", "architecture-diagram row naming unified search as a feature"),
        ],
    ),
    (
        "TechnologyPair",
        "A named pair of technologies combined by one mechanism "
        "(e.g., FTS5 (keyword) + sqlite-vec (vector KNN)).",
        [
            (92, "FTS5 (keyword) + sqlite-vec (vector KNN)", "the technology pair fused by hybrid search"),
        ],
    ),
    (
        "FailureMode",
        "A failure condition under which system behavior changes "
        "(e.g., embedding model load or download failure, a non-git folder).",
        [
            (135, "model download failure", "names the embedding-model failure condition"),
            (93, "embedding model load failure", "the same failure condition in the architecture diagram"),
        ],
    ),
    (
        "FallbackBehavior",
        "The degraded behavior a system falls back to under a failure condition "
        "(e.g., FTS5-only search, substring search).",
        [
            (135, "FTS5-only search", "the fallback target of the embedding-model failure"),
            (93, "FTS5-only", "the same fallback in the architecture diagram"),
        ],
    ),
]

# (predicate, description, subject_kind, object_kind, support)
PREDICATE_ADDITIONS = [
    (
        "serves_as",
        "A command serves a named role or function in the development workflow.",
        "Command",
        "ProcessRole",
        [(81, "canonical correctness gate", "cargo check --workspace serves as the canonical correctness gate (reference fact R4)")],
    ),
    (
        "indexes",
        "A subsystem reads and indexes a defined set of files or content.",
        "Subsystem",
        "FileExtensionSet",
        [(127, "pipeline indexes Git-tracked", "the shared code pipeline indexes the .rs/.ts/.tsx/.js/.jsx extension set (reference fact R5)")],
    ),
    (
        "excludes",
        "A subsystem or process leaves a defined set of paths out of its operation.",
        "Subsystem",
        "PathSet",
        [(127, "is_excluded_generated_path", "the shared discovery path enforces the generated/vendor/minified exclusion (reference fact R6)")],
    ),
    (
        "fuses",
        "A feature combines named technologies or signals through a stated fusion method.",
        "Feature",
        "TechnologyPair",
        [(92, "via Reciprocal Rank Fusion", "hybrid search fuses FTS5 (keyword) + sqlite-vec (vector KNN) via Reciprocal Rank Fusion (reference fact R7)")],
    ),
    (
        "degrades_to",
        "Behavior under a stated failure condition falls back to a simpler mode.",
        "FailureMode",
        "FallbackBehavior",
        [(135, "model download failure", "model download failure degrades to FTS5-only search (reference fact R9)")],
    ),
]

# Extra allowed endpoint pairs for seed predicates.
# predicate -> [(subject_kind, object_kind, support)]
PAIR_ADDITIONS = {
    "uses": [
        (
            "Application",
            "Tool",
            [(7, "It uses **akar**", "daftprompt (an app) uses akar, a UI component library; the motivating Epic 014 fact (line 7, test input T0)")],
        ),
        (
            "Application",
            "Subsystem",
            [(7, "combined with a git commit reader (gitoxide) and a hybrid search indexer", "daftprompt is combined with the hybrid search indexer subsystem (reference fact R3)")],
        ),
    ],
}

# Task 4 (Epic 015): per-relation decision thresholds and description rewrites
# for the five predicates no Epic 014 checkpoint ever emitted a candidate for
# (serves_as, indexes, excludes, fuses, degrades_to — 0 candidates across
# small/base/multi).
#
# Threshold: 0.25 for exactly these five labels. gliner2's per-relation
# decision bar defaults to 0.5 (RelationSpec.threshold=None falls back to 0.5
# in joint_ie/candidates.py); 0.25 is the JointIE tutorial's own demonstrated
# per-relation lowering (tutorial/15-joint_ie.md: .relation(..., threshold=0.25))
# and shifts each label's centered role logits up enough to test whether the
# five relations are merely gated, not absent.
# Rejected alternative: 0.1 — more headroom but re-opens the misleading-claim
# flood `base` showed under loose gating (101/127 misleading); rejected
# alternative: 0.4 — too timid to move predicates that currently emit nothing.
# Only the five zero-emission predicates are lowered; `uses` (the one relation
# `small` recovers, R1) keeps the default so the change isolates the silent
# relations and stays comparable to the canonical run.
#
# Descriptions: rewritten to echo the frozen snapshot's own phrasing — the
# vocabulary is pulled from the same `added_task2` support lines recorded on
# each predicate's allowed pair (Epic 014 Task 2 audit trail) — per the
# GLiNER2 tutorial's guidance to give relations clear, specific,
# example-grounded descriptions (tutorial/6 "Provide Descriptions for
# Ambiguous Relations" + "Clear and specific"). The pre-rewrite text is kept
# as `description_previous`; the grounding lines are recorded in
# `description_support` with Task 2's (line, excerpt, reason) shape.
# Rejected alternative: also rewriting entity-kind descriptions — out of
# scope; Epic 014 found entity recall already 6/8 and schema gaps 0.
#
# predicate -> (threshold, revised description, [(line, excerpt, reason), ...])
TASK4_PREDICATE_TUNING = {
    "serves_as": (
        0.25,
        "A command serves a named role or function in the development "
        "workflow — e.g. `cargo check --workspace` is the canonical "
        "correctness gate, run after every change.",
        [
            (81, "canonical correctness gate", "revision echoes the source's 'is the canonical correctness gate' (R4)"),
            (81, "Run it after every change", "revision echoes the source's 'Run it after every change' (R4)"),
        ],
    ),
    "indexes": (
        0.25,
        "A subsystem reads and indexes a defined set of files — e.g. one "
        "shared `code_files`/`vec_code` pipeline indexes Git-tracked `.rs`, "
        "`.ts`, `.tsx`, `.js`, and `.jsx` files.",
        [
            (127, "pipeline indexes Git-tracked", "revision echoes 'pipeline indexes Git-tracked .rs, .ts, .tsx, .js, and .jsx files' (R5)"),
            (127, "`.rs`, `.ts`, `.tsx`, `.js`, and `.jsx` files", "the indexed extension set quoted verbatim in the revision (R5)"),
        ],
    ),
    "excludes": (
        0.25,
        "A subsystem or process leaves a defined set of paths out of its "
        "operation — e.g. the shared discovery path enforces a centralized "
        "generated/vendor/minified exclusion (`is_excluded_generated_path`: "
        "`node_modules/`, `vendor/`, `dist/`, `build/`, `.next/`, "
        "`coverage/`, and `*.min.js`) so tracked-but-generated files cannot "
        "leak into the index.",
        [
            (127, "centralized generated/vendor/minified exclusion", "revision echoes 'a centralized generated/vendor/minified exclusion' (R6)"),
            (127, "is_excluded_generated_path", "the exclusion hook named verbatim in the revision (R6)"),
            (127, "so tracked-but-generated files cannot leak into the index", "revision echoes the source's exclusion rationale (R6)"),
        ],
    ),
    "fuses": (
        0.25,
        "A feature combines named technologies or signals through a stated "
        "fusion method — e.g. hybrid search fuses FTS5 (keyword) + sqlite-vec "
        "(vector KNN) via Reciprocal Rank Fusion.",
        [
            (92, "FTS5 (keyword) + sqlite-vec (vector KNN)", "the fused technology pair quoted verbatim in the revision (R7)"),
            (92, "via Reciprocal Rank Fusion", "revision echoes 'via Reciprocal Rank Fusion' (R7)"),
        ],
    ),
    "degrades_to": (
        0.25,
        "Under a stated failure condition behavior falls back to a simpler "
        "mode — e.g. model download failure degrades to FTS5-only search, a "
        "non-git folder degrades to substring search (failures degrade, "
        "never panic).",
        [
            (135, "model download failure", "revision echoes 'model download failure -> FTS5-only search' (R9)"),
            (135, "non-git folder", "revision echoes 'non-git folder -> substring search' (R9)"),
            (135, "Failures degrade, never panic", "revision echoes the source's 'Failures degrade, never panic' framing (R9)"),
        ],
    ),
}

REFERENCE_KIND_HINTS = {
    "application": "Application",
    "library": "Tool",
    "subsystem": "Subsystem",
    "command": "Command",
    "credential": "Configuration",
    "process_role": "ProcessRole",
    "file_extension_set": "FileExtensionSet",
    "path_set": "PathSet",
    "feature": "Feature",
    "technology_pair": "TechnologyPair",
    "failure_mode": "FailureMode",
    "fallback_behavior": "FallbackBehavior",
}

DECISIONS = [
    "Application added as a new entity kind: the frozen seed typed the whole "
    "system only via Command/Tool, which is why the budget-250 run typed "
    "daftprompt a Tool and rejected 'daftprompt uses akar'. Line 7 states "
    "daftprompt is a Rust app that uses akar, gitoxide, and the hybrid search "
    "indexer; line 141 confirms application-level ownership.",
    "uses(Application, Tool) added: line 7 states the motivating fact and the "
    "frozen test input T0 records (subject Application, object Tool) as "
    "must-accept.",
    "Subsystem added and uses(Application, Subsystem) allowed: the frozen R3 "
    "must-accept types the hybrid search indexer a Subsystem, not a Tool; "
    "forcing it into Tool would conflate the indexer subsystem with the "
    "search behavior it enables (recorded R3 rejection).",
    "Documented relations R4, R5, R6, R7, R9 received their own predicates "
    "(serves_as, indexes, excludes, fuses, degrades_to) with their own kinds "
    "instead of being forced into the narrow seed shapes searches/uses; each "
    "predicate pair records its supporting snapshot line.",
    "R8's object kind hint 'credential' maps to the seed kind Configuration, "
    "whose description explicitly names XIAOMI_API_KEY; no separate Credential "
    "kind was added and requires(Command, Configuration) already covers the "
    "R8 shape.",
    "The seed's example_relations are copied into this artifact as schema "
    "sanity checks only (example_triples_role); they are never passed to the "
    "model as inference-time examples. No evidence exists that the local API "
    "supports relation examples at inference time.",
    "Reference-set kind_hints are informal lowercase hints; the schema kinds "
    "above are the authoritative typing vocabulary (mapping recorded in "
    "reference_kind_hints).",
    "No JointIE graph constraints (unique_head, acyclic, ...) are declared: "
    "they apply per relation label, not per canonical predicate, so variant "
    "labels would fragment them. All pair-level rules are enforced by "
    "validate_claim.",
    "Task 4 (Epic 015): per-relation decision threshold 0.25 is declared for "
    "exactly the five predicates no Epic 014 checkpoint ever emitted "
    "(serves_as, indexes, excludes, fuses, degrades_to) and forwarded verbatim "
    "to JointSchema.relation(threshold=...); every other predicate keeps "
    "gliner2's default 0.5 decision bar so the one recovered relation (uses, "
    "R1) stays comparable to the canonical run. 0.25 is the JointIE tutorial's "
    "own demonstrated per-relation threshold (tutorial/15-joint_ie.md). "
    "Rejected alternative: 0.1 — more recall headroom but re-opens the "
    "misleading-claim flood `base` showed under loose gating (101/127 "
    "misleading); rejected alternative: 0.4 — too timid to move predicates "
    "that currently emit nothing.",
    "Task 4 (Epic 015): the same five predicates' descriptions are rewritten "
    "to echo the frozen snapshot's own phrasing — the vocabulary is pulled "
    "from each predicate's added_task2 support lines (Epic 014 Task 2 audit "
    "trail) — per the GLiNER2 tutorial's clear, specific, example-grounded "
    "relation-description guidance. The pre-rewrite text is kept as "
    "description_previous and the grounding lines as description_support "
    "(description_origin: revised_task4).",
    "Task 4 (Epic 015): per-predicate thresholds live on predicates[]."
    "threshold and are propagated to every derived JointIE label by "
    "derive_joint_ie_labels at load time; the model_mapping.joint_ie.labels "
    "embed keeps its label-shape-only key set (canonical_predicate, head, "
    "tail, description) so Epic 014's loader-derivation equality holds "
    "unchanged. Thresholds are not JointIEConfig fields — the run manifest "
    "identifies them via the schema artifact sha256.",
]

MODEL_LIMITATIONS = [
    "gliner2 2.0.0 JointIE typed endpoints are a Cartesian product: "
    "joint_ie/constraints.py TypedEndpoints.allows accepts an edge iff the "
    "head type is in head_types AND the tail type is in tail_types, and "
    "joint_ie/candidates.py CandidateBuilder pairs role spans with "
    "itertools.product over head x tail types. A single relation cannot "
    "declare 'these pairs only'.",
    "Chosen mapping: when a predicate's allowed pairs equal the product of "
    "distinct head x distinct tail kinds, one JointIE relation is emitted "
    "with multi-type head/tail. Otherwise pairs are grouped by subject kind "
    "(each group is a lossless product of one head type with its tail types); "
    "the largest group (ties: alphabetical head kind) keeps the bare "
    "predicate name and the rest become predicate__2, predicate__3, ... "
    "(e.g. uses -> uses [Application -> Subsystem|Tool], uses__2 "
    "[Command -> Tool]).",
    "Validation always enforces the canonical declared pairs, never the "
    "label-level product: uses(Command, Subsystem) is rejected even though "
    "the model labels uses and uses__2 jointly mention Command and Subsystem.",
    "Independent extract_relations (tutorial 6) carries no endpoint typing at "
    "all; canonical predicate names map 1:1 to relation labels and the typed "
    "pairs are enforced post-hoc by the validator.",
    "JointIE long-text extraction keeps a relation only when both endpoint "
    "mentions fall in the same chunk; chunk boundaries/overlaps are recorded "
    "by the Task 3 adapter, not by this schema.",
]


def _sha256(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def _support(entries, snapshot_lines):
    resolved = []
    for line_no, excerpt, reason in entries:
        line = snapshot_lines[line_no - 1]
        if excerpt not in line:
            raise ValueError(
                f"support excerpt {excerpt!r} not found on snapshot line {line_no}"
            )
        resolved.append(
            {
                "source": "frozen-inputs/AGENTS.md",
                "line": line_no,
                "quote": line,
                "reason": reason,
            }
        )
    return resolved


def build_schema(
    seed_bytes: bytes | None = None,
    snapshot_bytes: bytes | None = None,
) -> dict:
    seed_bytes = SEED_PATH.read_bytes() if seed_bytes is None else seed_bytes
    snapshot_bytes = SNAPSHOT_PATH.read_bytes() if snapshot_bytes is None else snapshot_bytes

    if _sha256(seed_bytes) != SEED_SHA256:
        raise ValueError("frozen seed sha256 drift; refusing to build the schema")
    if _sha256(snapshot_bytes) != SNAPSHOT_SHA256:
        raise ValueError("frozen AGENTS.md snapshot sha256 drift; refusing to build the schema")

    seed = json.loads(seed_bytes.decode("utf-8"))
    snapshot_lines = snapshot_bytes.decode("utf-8").splitlines()

    entity_kinds = []
    for raw in seed["entity_kinds"]:
        entity_kinds.append(
            {
                "kind": raw["kind"],
                "description": raw["description"],
                "origin": "seed",
            }
        )
    for kind, description, support in KIND_ADDITIONS:
        entity_kinds.append(
            {
                "kind": kind,
                "description": description,
                "origin": "added_task2",
                "support": _support(support, snapshot_lines),
            }
        )

    predicates = []
    for raw in seed["predicates"]:
        pairs = [
            {
                "subject_kind": raw["subject_kind"],
                "object_kind": raw["object_kind"],
                "origin": "seed",
            }
        ]
        for subject, obj, support in PAIR_ADDITIONS.get(raw["predicate"], []):
            pairs.append(
                {
                    "subject_kind": subject,
                    "object_kind": obj,
                    "origin": "added_task2",
                    "support": _support(support, snapshot_lines),
                }
            )
        predicates.append(
            {
                "predicate": raw["predicate"],
                "description": raw["description"],
                "origin": "seed",
                "allowed_pairs": pairs,
            }
        )
    for name, description, subject, obj, support in PREDICATE_ADDITIONS:
        if any(p["predicate"] == name for p in predicates):
            raise ValueError(f"predicate addition {name!r} collides with an existing predicate")
        entry = {
            "predicate": name,
            "description": description,
            "origin": "added_task2",
        }
        tuning = TASK4_PREDICATE_TUNING.get(name)
        if tuning is not None:
            threshold, revised_description, revision_support = tuning
            entry["description"] = revised_description
            entry["description_origin"] = "revised_task4"
            entry["description_previous"] = description
            entry["description_support"] = _support(revision_support, snapshot_lines)
            entry["threshold"] = threshold
        entry["allowed_pairs"] = [
            {
                "subject_kind": subject,
                "object_kind": obj,
                "origin": "added_task2",
                "support": _support(support, snapshot_lines),
            }
        ]
        predicates.append(entry)
    missing_tuning = set(TASK4_PREDICATE_TUNING) - {p["predicate"] for p in predicates}
    if missing_tuning:
        raise ValueError(f"Task 4 tuning targets unknown predicates: {sorted(missing_tuning)}")

    schema = core.ExperimentSchema(
        {
            "schema_id": SCHEMA_ID,
            "schema_version": SCHEMA_VERSION,
            "entity_kinds": entity_kinds,
            "predicates": predicates,
        }
    )
    labels = schema.joint_ie_labels()
    joint_labels = {
        spec.label: {
            "canonical_predicate": spec.canonical_predicate,
            "head": list(spec.head),
            "tail": list(spec.tail),
            "description": spec.description,
        }
        for spec in labels.values()
    }

    return {
        "schema_id": SCHEMA_ID,
        "schema_version": SCHEMA_VERSION,
        "experiment": "Epic 014 GLiNER2.5 graph extraction (Task 2)",
        "generator": "experiments/open-vocab-graph/schema/build.py",
        "deterministic": True,
        "revision": {
            "epic": "015-gliner25-small-recall-tuning-experiment",
            "task": "Task 4",
            "supersedes_artifact_sha256": SUPERSEDED_ARTIFACT_SHA256,
            "note": "Epic 015 Task 4 revision of Epic 014's artifact "
            "(366fa7e5…): adds per-relation decision thresholds (0.25) for the "
            "five zero-emission predicates (serves_as, indexes, excludes, "
            "fuses, degrades_to) and rewrites their descriptions to echo the "
            "frozen snapshot's own phrasing. Differs from 366fa7e5… in exactly "
            "those fields (plus this revision block); kinds, pairs, seed, "
            "snapshot, and all other predicates are byte-identical.",
        },
        "seed": {
            "origin": "Epic 013 Task 2 consolidation output (zero-merge pass, "
            "mimo-v2.5-pro), frozen in Epic 014 Task 1",
            "path": "../frozen-inputs/seed-vocabulary.json",
            "sha256": SEED_SHA256,
            "xxh3": SEED_XXH3,
        },
        "source_snapshot": {
            "path": "../frozen-inputs/AGENTS.md",
            "sha256": SNAPSHOT_SHA256,
            "line_count": len(snapshot_lines),
        },
        "entity_kinds": entity_kinds,
        "predicates": predicates,
        "example_triples_role": "schema_sanity_checks_only",
        "example_triples": seed["example_relations"],
        "model_mapping": {
            "joint_ie": {
                "labels": joint_labels,
                "declared_constraints": [],
            },
            "independent_relation_labels": {
                name: name for name in schema.predicates
            },
        },
        "validation": {
            "rule": "A typed claim (subject_kind, predicate, object_kind) is "
            "accepted iff the predicate is declared, both endpoint kinds are "
            "declared, and the exact (subject_kind, object_kind) pair is one "
            "of the predicate's allowed_pairs.",
            "rejects_endpoint_membership_only": True,
            "scope_note": "Text/span/citation grounding against the frozen "
            "snapshot is a later validation layer (Task 4); this schema "
            "validates types and pairs only.",
        },
        "reference_kind_hints": REFERENCE_KIND_HINTS,
        "decisions": DECISIONS,
        "model_schema_limitations": MODEL_LIMITATIONS,
    }


def serialize(schema: dict) -> str:
    return json.dumps(schema, indent=2, ensure_ascii=False) + "\n"


def main(argv: list[str]) -> int:
    check = "--check" in argv
    schema = build_schema()
    serialized = serialize(schema)
    if check:
        current = ARTIFACT_PATH.read_text(encoding="utf-8")
        if current != serialized:
            print("drift: gliner25-schema.json does not match the generator", file=sys.stderr)
            return 1
        print(f"ok: {ARTIFACT_PATH.name} matches the deterministic generator")
        return 0
    ARTIFACT_PATH.write_text(serialized, encoding="utf-8")
    print(f"wrote {ARTIFACT_PATH}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
