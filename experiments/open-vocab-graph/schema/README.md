# Epic 014 Task 2 — GLiNER2.5-compatible schema

A versioned schema for the GLiNER2.5 extraction experiment, converted from the
frozen Epic 013 seed (`../frozen-inputs/seed-vocabulary.json`, xxh3
`c776d7e579d8aed5`) with auditable Task 2 additions. Nothing in
`../frozen-inputs/` is ever modified.

## Files

| File | Role |
|---|---|
| `gliner25-schema.json` | The loadable, versioned artifact (schema 1.0.0). Checked in; regenerate with `build.py`. |
| `build.py` | Deterministic generator. Reads the frozen seed + snapshot, verifies their sha256 and every support quote, derives the JointIE mapping, writes the artifact. `python3 build.py --check` verifies the artifact is current. |
| `core.py` | Runtime API: `load_schema`, `validate_claim`, `derive_joint_ie_labels`, `interpret_example_triples`, `to_joint_schema` (builds a real `gliner2.joint_ie.JointSchema` offline — no model download). |
| `test_schema.py` | Plain-assert tests (pytest-compatible names): `.venv/bin/python test_schema.py` |

## What changed relative to the frozen seed

Every kind, predicate, and allowed pair carries `origin: "seed"` or
`"added_task2"`, and every addition records supporting snapshot lines (full
verbatim quotes + reasons) in its `support` list:

- **Added kind `Application`** (snapshot lines 3, 7, 141) and
  **`uses(Application, Tool)`** (line 7) — the motivating Epic 014 fact
  `daftprompt uses akar` was rejected in the baseline because the seed's only
  `uses` shape was `(Command, Tool)` and `daftprompt` got typed `Tool`.
- **Added kind `Subsystem`** (lines 7, 103, 127) and
  **`uses(Application, Subsystem)`** — the frozen R3 must-accept types the
  hybrid search indexer a Subsystem, not a Tool.
- **Documented relations got their own predicates** instead of being forced
  into the narrow `searches`/`uses` shapes: `serves_as(Command, ProcessRole)`
  (R4, line 81), `indexes(Subsystem, FileExtensionSet)` (R5, line 127),
  `excludes(Subsystem, PathSet)` (R6, line 127), `fuses(Feature,
  TechnologyPair)` (R7, line 92), `degrades_to(FailureMode, FallbackBehavior)`
  (R9, line 135).
- R8's `credential` hint maps to the seed kind `Configuration` (its
  description already names `XIAOMI_API_KEY`); no separate kind was added.
- The seed's `example_relations` are copied into the artifact as
  **sanity checks only** (`example_triples_role`); they are never few-shot
  inference examples.

## Multi-shape predicates → JointIE (recorded model-schema limitation)

gliner2 2.0.0's `JointIE` typed endpoints are a **Cartesian product**:
`TypedEndpoints.allows` accepts an edge iff head type ∈ head_types AND tail
type ∈ tail_types, and candidate pairing uses `itertools.product` over the two
type sets. One relation therefore cannot declare "these pairs only" — with
`uses` pairs `(Application, Tool)`, `(Application, Subsystem)`, `(Command,
Tool)`, a single multi-type relation would also admit `(Command, Subsystem)`.

Chosen mapping (implemented once in `core.derive_joint_ie_labels`, embedded in
the artifact by `build.py`):

1. If the declared pairs equal the product of distinct head × distinct tail
   kinds → one relation label with multi-type head/tail.
2. Otherwise group pairs by subject kind (each group is a lossless product of
   one head kind with its tail kinds). The largest group (ties: alphabetical
   head kind) keeps the bare predicate name; the rest become
   `predicate__2`, `predicate__3`, … For `uses`:
   - `uses`: head `Application`, tail `Subsystem|Tool`
   - `uses__2`: head `Command`, tail `Tool`

The experiment **validator** (`core.validate_claim`) always enforces the
canonical declared pairs, never the label-level product: a claim is accepted
iff the predicate is declared, both endpoint kinds are declared, and the exact
`(subject_kind, object_kind)` pair is one of the predicate's `allowed_pairs`.
`uses(Command, Subsystem)` is rejected even though `Command` and `Subsystem`
each appear in other declared pairs. Snapshot-span/citation grounding is a
later layer (Task 4).

For the independent `extract_relations` variant (tutorial 6), which has no
endpoint typing at all, canonical predicate names map 1:1 to labels
(`independent_relation_labels`) and pairs are enforced post-hoc by the same
validator.

Further limitations are recorded in the artifact under
`model_schema_limitations` (per-label graph-constraint fragmentation, JointIE
same-chunk relation locality, etc.).

## Regenerating

```bash
.venv/bin/python experiments/open-vocab-graph/schema/build.py          # regenerate
.venv/bin/python experiments/open-vocab-graph/schema/build.py --check  # verify byte-identical
.venv/bin/python experiments/open-vocab-graph/schema/test_schema.py    # 11 tests, no model
```

The build fails closed on any frozen-input sha256 drift or support-quote
mismatch. Identical frozen inputs always produce byte-identical output.
