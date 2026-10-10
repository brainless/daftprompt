# Epic 014: GLiNER2.5 Graph Extraction Experiment

> **Status: Superseded / code removed (2026-10-10).** The experiment was completed, but its code, reports, and frozen inputs were deleted from the working tree ahead of a new approach. They remain in git history (`git show b351721`). Task statuses below describe the state when the epic was done, not what is in the repo now.

## Overview

Epic 013 tested LLM-discovered entity kinds and predicates followed by sequential
LLM extraction. It established a useful source-snapshot and reference-set harness,
but extraction still missed important facts. In the user's later `budget-250` run,
`daftprompt uses akar` was emitted and rejected: the seed defined `uses` only as
`Command -> Tool`, the model called `daftprompt` a `Tool`, and its citation quoted
only part of AGENTS.md line 7. Other accepted claims assigned the wrong action
to a command, such as treating indexing as searching. An exact citation match
does not establish that a claim follows from the cited text.

This epic tests a different division of labor: retain Epic 013's LLM-based
schema discovery/consolidation, then use **GLiNER2.5** for repeated entity and
relation extraction against the discovered schema. The seed's kinds,
predicates, and descriptions can condition GLiNER2.5 inference. Its example
triples are evaluation/training material unless the local API is shown to
support relation-extraction examples at inference time. Do not assume that
training `InputExample`s are few-shot inference examples.

The experiment remains separate from the production `graph` CLI. The local
GLiNER2 source at `~/Projects/GLiNER2` is the primary API reference: read its
`README.md`, `tutorial/6-relation_extraction.md`,
`tutorial/15-joint_ie.md`, and relevant implementation before coding. Use
`AutoExtractor` or `JointIE` for GLiNER2.5 boundary checkpoints; the legacy
`GLiNER2` loader is span-only. Confirm that each checkpoint enables relation
extraction. `JointIE` supports typed endpoints and graph constraints; compare
  it with independent relation extraction rather than assuming either wins.

**Status:** Done (Tasks 1-8 complete). Final comparison report:
`experiments/open-vocab-graph/REPORT-gliner25.md`.

## Goals

- Improve recovery of supported relations, especially application-to-tool use,
  the correctness gate, supported file extensions, generated-path exclusions,
  hybrid search components, the graph API-key requirement, and the embedding
  failure fallback.
- Reduce wrong endpoint kinds, misleading predicates, invented entities,
  duplicate names, and malformed evidence while preserving snapshot-grounded
  citations.
- Determine whether a small local schema-conditioned model gives better
  quality, latency, and repeatability than Epic 013's per-block LLM extractor.
  Test the exact models **in this order**: `fastino/gliner2.5-small-v1`, then
  `fastino/gliner2.5-base-v1`, then `fastino/gliner2.5-multi-v1`.
- Keep comparisons fair: use one frozen AGENTS.md snapshot, reference set,
  seed/schema revision, chunking policy, and scoring method for all three
  model runs. Record their hashes and settings with each result.

## Non-goals

- Replacing `graph build` or `graph search`, changing their SQLite tables, or
  claiming production readiness from an AGENTS.md-only result.
- Fine-tuning GLiNER2.5 in this epic. Preserve examples and errors so a later
  fine-tuning experiment is possible.
- Treating a missing relation as proof of a negative answer. Question coverage
  is scored from evidence, not from model confidence alone.

## Design constraints

- The experiment may change its own seed/schema and output formats to fit
  GLiNER2.5. Keep Epic 013's original run artifacts readable; write new runs
  to separate directories and never overwrite them.
- A predicate may have multiple **allowed endpoint pairs**. For example,
  `uses` may allow `(Application, Tool)` and `(Command, Tool)`. Avoid treating
  independent head/tail type lists as an unrestricted Cartesian product if
  that would admit invalid pairs. Translate pair variants into model schemas
  as needed, then enforce the intended pairs during validation.
- GLiNER2.5 returns mention spans. Resolve accepted spans against the exact
  stored source snapshot and derive line citations deterministically. A span
  or confidence score is not evidence of entailment; review whether cited
  text actually supports each subject-predicate-object claim. Record rejected
  candidates and reasons. Do not use a prior block or summary as evidence.
- Entity canonicalization must distinguish mention text from graph identity:
  `daftprompt`, `daftprompt CLI`, and a particular `cargo run` invocation
  should not be merged merely because their names are similar.
- GLiNER2.5 uses a supplied set of labels; retain an explicit route for facts
  whose kind or predicate was absent from the seed. Measure these schema gaps
  separately from model extraction misses. A future LLM discovery pass may
  propose additions, but do not silently relabel a missed fact as supported.
- JointIE's long-text relations require both mentions in the same chunk.
  Preserve complete physical lines where practical, record chunk boundaries
  and overlaps, and distinguish cross-chunk misses from extraction misses.

## Tasks

### Task 1: Freeze the comparison inputs and baseline

**Status:** Done

**Change:** Freeze a copy of AGENTS.md, Epic 013's pre-recorded reference set
(R1-R9, repeated entities, and six questions), and one chosen seed. Document
the chosen seed's origin. The current `budget-250` folder uses a different
seed hash and contains 40 validated claims, whereas Epic 013's report describes
a 31-claim run using the seed shared by budgets 150/400/600; its 250 column is
not the result in that folder. Do not compare model results with that stale
column. Either re-score the actual folder as the LLM baseline under its seed
or run a new LLM baseline on the frozen seed, and label any schema differences.

**Implementation notes (Task 1):** All frozen inputs live in
`experiments/open-vocab-graph/frozen-inputs/` (chmod 444 files / 555 dirs;
immutability policy in its README and MANIFEST.json). Chosen seed: the
budget-250 seed (xxh3 `c776d7e579d8aed5`, kinds Command/Configuration/Tool/
DataSource, `uses`=Command→Tool) — Epic 013 Task 2's zero-merge consolidation
output, byte-identical to the current top-level `output/seed-vocabulary.json`
and to the seed hash recorded in the budget-250 run manifest. The other Epic
013 seed (xxh3 `9bd9ffae74304ce2`, used by budgets 150/400/600, kinds CLI/
Database/Model/SearchIndex/DataSource) is documented as different-seed context
only. Baseline choice: **re-scored the actual `output/runs/budget-250/` folder**
under its seed (offline, deterministic, contains the motivating line-7
rejection); no new LLM run was made. Re-score: 40 valid claims → 28 clean /
4 weak / 8 misleading (70/10/20%), 0 duplicates, 40/40 citations re-verified
against the frozen snapshot, R1-R9 recovery 0/9, Q2 covered-with-defect, Q3
partial-with-defect, Q1/Q4 not covered, Q5/Q6 correct absence
(`frozen-inputs/llm-baseline/budget-250-rescore.{json,md}`, generated by a
deterministic script). Schema differences vs the GLiNER2.5 schema are labeled
non-identical in `frozen-inputs/comparison-plan.md` (Application kind missing,
single-shape predicates vs allowed endpoint pairs, model-quoted vs
snapshot-derived evidence). `check_inputs.py` (71 checks: hashes, line 7,
R1-R9 passages, repeated entities, question passages, test-input quotes and
recorded misquotes, seed fingerprint) was run before any model output exists;
its log is stored alongside.

**Acceptance criteria:**

- [x] The snapshot, reference set, seed, and their hashes are stored in an
      immutable experiment-input directory; source-line expectations are
      checked before looking at new model output.
- [x] The comparison plan states exactly which LLM run is the baseline and
      whether its schema is identical to the GLiNER2.5 schema.
- [x] The test inputs include AGENTS.md line 7 (`daftprompt uses akar`) and
      targeted positive and negative examples for the nine reference facts.

### Task 2: Build a GLiNER2.5-compatible schema

**Status:** Done

**Depends on:** Task 1.

**Change:** Convert the frozen discovered seed into a versioned schema with
entity descriptions, relation descriptions, and allowed endpoint pairs. Add
`Application` and the supported `uses(Application, Tool)` pair; handle other
documented relations without forcing them into `searches` or `uses` merely
because the seed is narrow. Keep additions and their supporting source lines
auditable. Determine how best to express multi-shape predicates in `JointIE`
and in the experiment validator. Record any model-schema limitation and the
chosen mapping. Use the seed examples as schema sanity checks, not as an
unverified inference feature.

**Implementation notes (Task 2):** Schema package at
`experiments/open-vocab-graph/schema/` (`__init__.py`, `core.py`, `build.py`,
`test_schema.py`, `README.md`). The checked-in artifact
`gliner25-schema.json` (schema_id `daftprompt-gliner25-experiment`, version
1.0.0) is generated deterministically by `build.py` from the frozen seed +
snapshot (`build.py --check` verifies byte-identity; the build fails closed on
frozen-input sha256 drift or support-quote mismatch). Every kind, predicate,
and allowed pair carries `origin: seed | added_task2`; additions record full
verbatim snapshot lines as support. Added: `Application` (lines 3/7/141),
`Subsystem` (7/103/127), `ProcessRole` (81), `FileExtensionSet` (127),
`PathSet` (127), `Feature` (92/94), `TechnologyPair` (92), `FailureMode`
(135/93), `FallbackBehavior` (135/93); pairs `uses(Application, Tool)` and
`uses(Application, Subsystem)` (line 7); predicates `serves_as` (R4),
`indexes` (R5), `excludes` (R6), `fuses` (R7), `degrades_to` (R9) so the
documented relations keep their own shapes instead of being forced into
`searches`/`uses`. R8's `credential` hint maps to seed `Configuration`
(recorded decision; its description names `XIAOMI_API_KEY`). Model-schema
limitation recorded in the artifact: gliner2 2.0.0 JointIE typed endpoints are
a Cartesian product (`TypedEndpoints.allows` + `itertools.product` candidate
pairing), so one relation cannot declare "these pairs only". Chosen mapping
(`core.derive_joint_ie_labels`): product-expressible pair sets stay one
multi-type relation; otherwise pairs group by subject kind — `uses` becomes
`uses` (Application → Subsystem|Tool) plus `uses__2` (Command → Tool). The
validator (`core.validate_claim`) enforces the canonical declared pairs, never
the label-level product. Seed example triples are copied into the artifact as
`example_triples_role: schema_sanity_checks_only`. Tests (11, no model
download) cover determinism/hash tripwires, seed-vs-added provenance, support
quotes vs snapshot, pair acceptance/rejection (including the
endpoint-membership trap `uses(Command, Subsystem)`), JointIE mapping +
offline `JointSchema` build, independent labels, and example-triple
interpretation; `daftprompt`→Application and `akar`→Tool asserted. Frozen
inputs re-verified untouched (71/71 `check_inputs.py`).

**Acceptance criteria:**

- [x] Schema generation is deterministic, checked in as code/config, and
      records seed-derived versus later-added labels.
- [x] The validator accepts `uses(Application, Tool)` and
      `uses(Command, Tool)`, rejects disallowed pairs, and never accepts a
      claim solely because each endpoint appears in some allowed pair.
- [x] `daftprompt` can be typed as an application and `akar` as a tool.
- [x] Tests cover schema conversion, pair validation, and example-triple
      interpretation without requiring a model download.

### Task 3: Implement the local extraction adapter

**Status:** Done

**Depends on:** Task 2.

**Change:** Add a standalone Python adapter/harness under
`experiments/open-vocab-graph/` that loads a model selected by explicit ID,
passes the versioned schema to GLiNER2.5, processes snapshotted text in
recorded chunks, and writes raw model output plus normalized entity/relation
candidates. Evaluate `JointIE` first for typed extraction; allow an independent
`extract_relations` variant under the same input and scoring contract if it
addresses a measured limitation. Fail visibly when a checkpoint lacks the
required relation head or a result is infeasible. Do not hide empty chunks.

The agent may create a repo-local `.venv`, install GLiNER2 from the local
checkout or a pinned release, and download the named checkpoints. Add
`.venv/` and local model/output caches to `.gitignore` as appropriate; record
package and checkpoint revisions, device, thresholds, chunking, and command
lines for reproducibility. Avoid committing model weights or environment files.

**Implementation notes (Task 3):** Harness package at
`experiments/open-vocab-graph/harness/` (`__init__` with sys.path bootstrap,
`chunking.py`, `normalize.py`, `adapter.py`, `runner.py`, `cli.py`,
`__main__.py`, `test_harness.py`, `README.md`); runs via
`python -m harness extract --model <id> [--variant jointie|independent]` from
any cwd using the pre-existing repo `.venv` (gliner2 2.0.0, torch 2.14.0 —
byte-identical to the local `~/Projects/GLiNER2` checkout). No checkpoints
were downloaded (Task 5 does live runs); no run directories were created.
Chunking policy: whole physical lines packed to a soft word budget (default
384, the GLiNER long-context default), overlap recorded as trailing whole
lines re-sent in the next chunk (default 2), oversized single lines get their
own whole-line chunk with no overlap after them; the frozen 146-line snapshot
yields 5 chunks (375/317/382/344/133 words) with line 7 wholly inside chunk 0.
Deliberate deviation from gliner2's native word-token chunker (it cuts
mid-line) recorded in `chunking.py`. Normalization: chunk-local spans remapped
to document offsets and verified against the snapshot text (clamped/mismatch
flagged in notes); model labels map to canonical predicates via the artifact's
JointIE mapping (`uses__2`→`uses`); every relation candidate is validated
against declared endpoint pairs and rejected candidates are retained with
verdict codes; an ungroundable relation (endpoint id missing) moves to the
chunk record's `dropped` list. Dedup keys: entities `(kind, doc_start,
doc_end)`, relations `(predicate, subject span key, object span key)` — max
confidence wins, `occurrences`/`source_chunks` record merges, distinct spans
stay distinct even with identical surface text. Variants: `jointie` (default;
`JointIE` + `core.to_joint_schema`, per-chunk `feasible` flag recorded,
infeasible chunks are a visible status and an all-infeasible run fails with
exit 3) and `independent` (one `AutoExtractor` schema with typed entity
descriptions + untyped relations; kinds attached post-hoc by exact span match,
unmatched endpoints kept as rejected `unknown_*_kind` candidates — never
dropped). Visible-failure gates: snapshot sha256/line-count must match the
artifact's frozen record (`SnapshotMismatchError`); `config.json` is inspected
before any weight download and span checkpoints / boundary checkpoints with
`enable_relations=false` (including pre-config_version-3 configs, which
migrate to disabled) raise `CheckpointCapabilityError` naming the checkpoint;
engine-load failures abort before any run directory is written; per-chunk
errors are recorded per chunk (status `error`, run continues, exit 4). Empty
chunks always get a chunk record with their raw response. Run outputs go to
`gliner-runs/<model_slug>/<variant>/<UTCstamp>[-label]/` (never overwritten;
`manifest.json`, `summary.json`, `chunks/chunk-NNN.json`, `entities.json`,
`relations.json`), with command line, HF revision (best-effort probe) +
`config.json` sha256, package versions, device, thresholds, chunking record,
schema/snapshot hashes, and elapsed times. Gitignore: repo-root `/.venv/`,
experiment-local `gliner-runs/` and `.model-cache/`. Verification offline:
17/17 harness tests (chunk mapping/boundaries/overlaps, span remap clamping,
jointie + independent normalization, label mapping, verdict retention, dedup,
chunk-status visibility, capability gate on local config fixtures, full runner
pass with an injected fake engine reproducing the line-7 `daftprompt`
(Application) `uses` `akar` (Tool) fact with exact snapshot offsets, snapshot
drift tripwire, offline CLI `--plan-only`), 11/11 schema tests unchanged,
71/71 `check_inputs.py` (frozen inputs untouched), `cargo check --workspace`
clean. The only untested-offline path is actual weight loading/inference,
which is exactly Task 5's scope.

**Acceptance criteria:**

- [x] One command can select any of the three named checkpoint IDs without
      changing source code.
- [x] Outputs contain per-chunk raw responses, normalized candidates,
      confidence scores when available, model/config revisions, elapsed time,
      and explicit failure/empty-result states.
- [x] Repeated mentions and chunk overlaps are deduplicated by stable spans,
      while distinct entities remain distinct.
- [x] Unit tests of normalization and chunk mapping run without loading a
      checkpoint. The canonical `cargo check --workspace` passes after repo
      changes.

### Task 4: Ground evidence and implement scoring

**Status:** Done

**Depends on:** Task 3.

**Change:** Map model character offsets to the frozen snapshot, derive exact
line ranges and text from that snapshot, and validate endpoint types, span
containment, chunk locality, and allowed relation pairs. Record unsupported
  or ambiguous candidates with reasons. Score accepted claims against Epic
  013's reference set, including human-reviewed claim-to-evidence support and
  question coverage. Separate failure categories: absent schema label, missing
  entity, missing relation, wrong type/predicate/direction, boundary loss,
  evidence mismatch, and unsupported-but-accepted claim.

**Implementation notes (Task 4):** Scoring package at
`experiments/open-vocab-graph/scoring/` (`__init__.py` with sys.path
bootstrap + default artifact paths, `grounding.py`, `taxonomy.py`,
`reference.py`, `accept.py`, `review.py`, `metrics.py`, `report.py`,
`score.py`, `cli.py`, `__main__.py`, `test_scoring.py`); run via
`python -m scoring score <run_dir> [--review ...] [--out ...] [--force]
[--no-write]` — pure stdlib, no gliner2/torch import, no model load.
Grounding: character offsets map to 1-based inclusive physical-line ranges
of the frozen snapshot and citation text is always the snapshot's own lines;
a mention must equal the snapshot slice **exactly** (whitespace-normalized
equality is recorded as an ambiguity signal and still rejects), and the
snapshot sha256 is verified against both the schema artifact's frozen record
and the run manifest before scoring (fail closed). Acceptance re-validates
every emitted relation against the schema artifact — never trusting the
run's stored verdict; disagreements are recorded — requires both endpoints
grounded exactly plus JointIE chunk locality (a shared containing chunk),
and keeps every candidate including rejections with taxonomy codes/categories
(`absent_schema_label`, `missing_entity`, `missing_relation`, `wrong_type`,
`wrong_predicate`, `wrong_direction`, `boundary_loss`, `evidence_mismatch`,
`unsupported_but_accepted`). Citation validity (deterministic exact-line
resolvability) is reported separately from semantic support (human review:
supported/weak/misleading/unreviewed via `review.json`; the generated
`review-template.json` is never mistaken for a filled review; unrecognized
support values warn and stay unreviewed). R1-R9 recovery additionally
requires reviewed support == "supported", so no model output counts as
recovered. Matching tiers: strong/weak name match plus a deliberately weaker
`kind_evidence` tier requiring **both** endpoints to share a lowercase token
with the fact's names (rejected `or`: the three parallel line-7 `uses` facts
share the subject "daftprompt" and would cross-match); per-fact statuses
separate schema gaps, missing entity/relation, rejected attempts, evidence
mismatch, and pending review. Metrics: R1-R9 recovery + omissions,
valid/supported/weak/misleading/unreviewed claims, surface duplicates
(distinct spans), reference-supported relation precision/recall bounds,
repeated-entity recall/precision on recorded occurrence lines, six-question
evidence coverage (covered/partial/not_covered/pending_review), run + scoring
runtime, and peak memory when the manifest records `peak_memory_bytes`.
Focused regression: a fake-engine Task 3 harness run represents
`daftprompt (Application) uses akar (Tool)` and asserts acceptance with a
snapshot-derived line-7 citation while explicitly asserting support stays
"unreviewed" (no model recovery claimed pre-review). Tests (18, no model
download) cover offset→line grounding incl. boundary crossings,
snapshot-derived-only citation text, name/predicate resolution, fact-shape
schema gaps, the line-7 regression end-to-end through the harness, reviewed-
support recovery + misleading/unsupported-but-accepted, evidence mismatch on
wrong lines, wrong-type rejection retention, cross-chunk boundary loss,
duplicate surface triples, empty-run omissions, snapshot-drift and
missing-file fail-closed paths, scorecard/review-template writing (existing
scorecards refuse silent overwrite), and the CLI. Verification: 18/18
scoring tests, 17/17 harness, 11/11 schema, 71/71 `check_inputs.py` (frozen
inputs untouched), `cargo check --workspace` clean.

**Acceptance criteria:**

- [x] All accepted citations resolve to the stored snapshot; a model cannot
      supply citation text independent of the snapshot.
- [x] The report distinguishes exact citation validity from semantic support
      and records every emitted candidate, including rejections.
- [x] Metrics include R1-R9 recovery, valid/supported/misleading claims,
      duplicates, omissions, entity and relation precision/recall where the
      reference set supports them, six-question evidence coverage, runtime,
      and peak memory if measurable.
- [x] A focused regression asserts that `daftprompt uses akar` can be
      represented and cited from line 7, without asserting that a model has
      recovered it before the live tests.

### Task 5: Test `fastino/gliner2.5-small-v1`

**Status:** Done

**Depends on:** Tasks 1-4.

**Change:** Download/load the small checkpoint first and execute the frozen
AGENTS.md benchmark. Inspect its entity and relation outputs, the line-7
case, all nine reference facts, and unsupported or empty results. If model
load or relation extraction fails, diagnose and document it before moving to
the next model; keep any harness fix model-neutral and rerun small.

**Implementation notes (Task 5):** Canonical run
`gliner-runs/fastino_gliner2.5-small-v1/jointie/20260928T112911Z/` (variant
`jointie`, the default; two preliminary runs `20260928T111610Z` /
`20260928T112042Z` kept alongside — extraction output is byte-identical
across all three, confirming determinism). Pre-load diagnosis: the venv's
`gliner2[local]` install lacks `protobuf`, and gliner2 2.0.0's tokenizer path
raises `ImportError` without it (DeBERTa-v3 slow-tokenizer conversion;
the checkpoint's legacy list-valued `extra_special_tokens` metadata is a
benign quirk gliner2 normalizes). Fixed in the environment
(`pip install protobuf==7.36.2`) and documented in `harness/README.md` — no
harness code change needed for the failure. Two model-neutral harness
changes, each followed by a small rerun: (1) `runner.peak_memory_record`
records `manifest.resources.peak_memory_bytes` per the Task 4 scorecard's
request (process RSS high-water via `resource.getrusage`; test-asserted);
(2) `adapter.apply_hub_env` redirects `HF_HUB_CACHE`/`HF_HUB_OFFLINE` from
`--cache-dir`/`--local-files-only`, because `AutoExtractor.from_pretrained`
(gliner2/auto.py) drops hub options when delegating to the architecture
loader and the tokenizer loader gets none — without this, weights silently
land in `~/.cache/huggingface` and `--local-files-only` is not offline.
`--revision` is still resolved as `refs/main` by gliner2's weight path; the
manifest records the resolved sha. Tests: 18/18 harness, 18/18 scoring,
11/11 schema, `cargo check --workspace` clean.

Run result: extraction succeeded — 5/5 chunks `ok`/feasible, 0 errors, 0
empty; 40 entity mentions (40 deduplicated spans), 1 relation candidate, 1
accepted, **0 rejected, 0 dropped** (JointIE emitted no relation candidates
in chunks 1-4 at all). Line-7 case **recovered**: C-001
`Application(daftprompt) -uses-> Tool(akar)` (model label `uses`, relation
conf 0.358 over the 0.05 candidate threshold), both endpoints exact snapshot
slices on line 7 (134-144 / 270-274), citation AGENTS.md:7-7 derived from
the snapshot — the exact claim the budget-250 LLM baseline emitted and
rejected now validates against the schema's `uses(Application, Tool)` pair.
Claim-to-evidence support judged `supported` in Task 5's inspection
(`scoring/review.json`, agent review; a human re-review can rescore).
R1-R9: **1/9 recovered (R1)**; R2-R4 `missing_relation` (entities present,
no relation candidate: `gitoxide` surfaced only as a broken-boundary
TechnologyPair span "git commit reader (gitoxide", `cargo check` typed
ProcessRole not Command, `canonical correctness gate` emitted but no
`serves_as` pair); R5-R9 `missing_entity` (subjects `code indexer`,
`code indexer file discovery`, `hybrid search`, `XIAOMI_API_KEY`, and R9's
FailureMode/FallbackBehavior pair never emitted — object fragments were
(PathSet paths individually, `.rs`/`.tsx`/`.jsx` with confused
DataSource/FileExtensionSet kinds, R7's object as TechnologyPair,
`substring search` as FallbackBehavior). Every fact lies wholly inside one
chunk, so no miss is cross-chunk boundary loss; schema gaps 0 — all misses
are extraction misses (5 missing_entity, 3 missing_relation). Questions:
Q1 partial (line 7 cited, line 141 not), Q2/Q3/Q4 not_covered, Q5/Q6
correct_absence (no false claims). Repeated-entity recall 6/8 (missing
`tree-sitter`, `XIAOMI_API_KEY`).

Reproducibility (manifest-verified): model `fastino/gliner2.5-small-v1` HF
revision `7132dc4561c3f94563c6147e75ffa8ef34c4964a`, `config.json` sha256
`0b7d9e14…`, boundary architecture + relation head enabled (gate passed
pre-download), encoder `microsoft/deberta-v3-xsmall`; gliner2 2.0.0 / torch
2.14.0 / transformers 4.57.6 / huggingface_hub 0.36.2 / Python 3.14.7;
macOS-15.3.1 arm64, device auto→mps (no CUDA); JointIE thresholds recorded
(beam, beam_size 32, candidate/relation-role 0.05, top-k 32/12); schema
artifact sha256 `366fa7e5…` (seed xxh3 `c776d7e579d8aed5`), snapshot sha256
`a15dbdb0…` (146 lines). Latency: 112.7 s wall including a 290 MB cache
fill (16.9-22.4 s on warm cache; per-chunk extraction 1.5-2.6 s, sum 9.8 s
for 5 chunks / 1445 words); peak RSS 959,266,816 bytes. Frozen inputs
re-verified untouched (71/71 `check_inputs.py`).

**Acceptance criteria:**

- [x] A complete, inspectable small-model run and per-fact scorecard exist,
      or a reproducible checkpoint/runtime incompatibility is documented.
- [x] Record exact model revision, package revision, hardware/device, runtime,
      thresholds, and schema/snapshot hashes.

### Task 6: Test `fastino/gliner2.5-base-v1`

**Status:** Done

**Depends on:** Task 5.

**Change:** Run the base checkpoint second using the same frozen inputs and
scoring contract. Any threshold tuning must be reported separately from the
untuned, like-for-like result and applied consistently when comparing models.

**Implementation notes (Task 6):** Canonical run
`gliner-runs/fastino_gliner2.5-base-v1/jointie/20260928T113433Z/` (variant
`jointie`, the default; **untuned** — JointIE thresholds byte-identical to
small's, so this is the like-for-like result; no harness/scoring/schema code
changes and no environment changes were needed). Pre-download gate passed:
boundary architecture, relation head enabled, encoder
`microsoft/deberta-v3-base`; the 746 MB checkpoint filled `.model-cache/`.
Frozen contract verified identical to Task 5's manifest: schema artifact
sha256 `366fa7e5…` (seed xxh3 `c776d7e579d8aed5`), snapshot sha256 `a15dbdb0…`
(146 lines) — byte-identical, no deviations.

Run result: extraction succeeded — 5/5 chunks `ok`/feasible, 0 errors, 0
empty (chunk 3 = lines 125-140 decoded 27 entities but **0 relations**); 123
entity mentions (122 deduplicated spans), 127 relation candidates, 127
accepted by the pair validator, **0 rejected, 0 dropped** (vs small's single
candidate — JointIE typed decoding only emits schema-allowed pairs, so every
defect passed the validator and only entailment review catches them). 110/127
are surface duplicates (21 unique span-distinct triples; the `cargo run`
Cartesian explosion over the L20-L37 CLI block's repeated tokens and the
chunk-0/1 overlap). Line-7 case **recovered twice over**: C-002
`Application(daftprompt) -uses-> Tool(akar)` (label `uses`, conf 0.822) with
both endpoints exact line-7 slices (134-144 / 270-274), citation AGENTS.md:7-7
— plus C-001, same triple with the subject at its L3 mention (citation 3-7,
conf 0.839). Both judged `supported` in Task 6's inspection
(`scoring/review.json`, agent review; a human may re-review). R1-R9: **2/9
recovered (R1 via C-001/C-002, R8 via C-074 `Command(graph build) -requires->
Configuration(XIAOMI_API_KEY)` citing 45-45)** vs small's 1/9 (R1) — R8's
`requires` shape was never emitted by small. Remaining misses: R2 `missing_entity`
(`gitoxide` never emitted — small at least produced a broken-boundary span),
R4 `missing_entity` (`the canonical correctness gate` never emitted — small
emitted it), R3/R5/R6/R7/R9 `missing_relation` (entities present, no relation
candidate: chunk 3's zero-relation decode covers L127/L135, so R5/R6/R9 are
extraction misses inside one chunk, not cross-chunk boundary loss; several
"present" subjects match only via containment of bare `search` tokens in
"hybrid search" names, but R5/R6's extension/path entities are genuine L127
spans). Schema gaps 0. Review (calibrated to the Task 1 baseline ladder):
**18 supported / 8 weak / 101 misleading** — the misleading mass is 49
wrong-action `searches` claims anchored on GUI/debug/index/reindex/no-index
`cargo run` lines (index-as-search class), 12 `requires` claims
misattributing XIAOMI_API_KEY from graph commands to plain CLI/GUI
invocations, 5 flag-scope violations (`--search-code`→documents,
`--search-documents`→code), and 40 `searches` claims reading capability off
the L67-L70 example questions ("Can we search Python code?", "Lua code") and
L57's negated "source code is not graph authority" (including `Command(Search)`
minted from the noun phrase "Search citations" at L57). Weak = 2 `uses(UI,
akar)` paraphrases of L141 "UI is rendered by akar" (UI typed Application;
subject anchored at L142) plus 6 true-but-defectively-anchored claims (e.g.
`code` inside `code.rs`/`code vectors`/example questions). Questions: Q1
**covered** (L7 by supported C-002 + L141 cited; small was partial),
Q2/Q3/Q4 not_covered (same as small), Q5/Q6 correct_absence. Repeated-entity
recall 5/8 (missing SQLite, tree-sitter, gitoxide) vs small 6/8: base recovered
`XIAOMI_API_KEY` and lost `gitoxide`/`SQLite`/`tree-sitter`.

Differences vs small, classified: **relation effects dominate** — small decoded
1 candidate, base 127 (raw per chunk 73/46/6/0/2) at identical thresholds, a
decoding-permissiveness shift that gains `requires` (R8) and the
search-source bindings but whose label prior still never produced
`serves_as`/`indexes`/`excludes`/`fuses`/`degrades_to` (shared R3-R7/R9
relation misses). **Entity effects**: 122 spans vs 40 — recovers XIAOMI_API_KEY,
L127 extension/path sets (now correctly FileExtensionSet/PathSet typed) and
R9's FailureMode, but drops gitoxide/canonical-correctness-gate/SQLite/
tree-sitter; kind confusions shift (UI→Application, question tokens→Command,
`akar-core`→Configuration). **Schema effects: 0** (same artifact, gaps 0 in
both). **Evidence effects**: both 127/127 and 1/1 citation-valid with exact
snapshot slices; base's defects are wrong bindings (question fragments, negated
text as objects), i.e. entailment not grounding. **Chunking effects: 0** (same
5-chunk plan; every R-fact's lines wholly inside one chunk). Costs: 252.0 s
wall including the 746 MB cache fill (vs small 112.7 s / 290 MB; per-chunk
extraction 2.0-6.0 s, sum 16.6 s for 5 chunks / 1445 words, vs small 1.5-2.6 s
/ 9.8 s); peak RSS 2,262,417,408 bytes (2.36× small's 959,266,816).
Threshold tuning: none performed; noted for Task 8 — the relation-confidence
floor (0.316) is far above the 0.05 candidate threshold, so raising thresholds
within the recorded range would not remove the misleading set (binding-level
defects). If tuning is tried later it must be reported separately and applied
to all three models. Reproducibility (manifest-verified): model
`fastino/gliner2.5-base-v1` HF revision
`ca906247640776a07753514055be9726f9080ead`, `config.json` sha256 `0eb92d00…`,
gliner2 2.0.0 / torch 2.14.0 / transformers 4.57.6 / huggingface_hub 0.36.2 /
Python 3.14.7; macOS-15.3.1 arm64, device auto→mps (no CUDA); JointIE
thresholds recorded identical to small (beam, beam_size 32, candidate/
relation-role 0.05, top-k 32/12). Tests: 18/18 harness, 18/18 scoring, 11/11
schema, `cargo check --workspace` clean; frozen inputs re-verified untouched
(71/71 `check_inputs.py`).

**Acceptance criteria:**

- [x] A complete base-model run and per-fact scorecard exist, or a
      reproducible checkpoint/runtime incompatibility is documented.
- [x] Differences from small are classified as entity, relation, schema,
      evidence, or chunking effects, with time and memory costs recorded.

### Task 7: Test `fastino/gliner2.5-multi-v1`

**Status:** Done

**Depends on:** Task 6.

**Change:** Run the multilingual checkpoint third on the same English source
and settings. This tests whether its broader language coverage changes
quality or cost on the current corpus; do not assume it will outperform base.

**Implementation notes (Task 7):** Canonical run
`gliner-runs/fastino_gliner2.5-multi-v1/jointie/20260928T120352Z/` (variant
`jointie`, the default; **untuned** — JointIE thresholds byte-identical to
small/base, so this is the like-for-like result; no harness/scoring/schema
code changes and no environment changes were needed). One warm-cache rerun
`20260928T121442Z-warm` is kept alongside — raw chunk responses and
normalized entities/relations are byte-identical to the canonical run
(determinism confirmed). Pre-download gate passed: boundary architecture,
relation head enabled, encoder `microsoft/mdeberta-v3-base` (multilingual
DeBERTa); the 1.1 GB checkpoint filled `.model-cache/`. Frozen contract
verified identical across all three manifests (small, base, multi): schema
artifact sha256 `366fa7e5…` (seed xxh3 `c776d7e579d8aed5`), snapshot sha256
`a15dbdb0…` (146 lines), scoring schema version 1 with both snapshot
verifications true and zero verdict disagreements in every scorecard —
byte-identical, **no deliberate deviations**.

Run result: extraction succeeded — 5/5 chunks `ok`/feasible, 0 errors, 0
empty; 89 entity mentions (86 deduplicated spans), **0 relation candidates**
(JointIE decoded an empty relation set in every chunk despite `feasible`
true), 0 accepted, **0 rejected, 0 dropped** (nothing to reject: no candidate
was ever emitted). Line-7 case **not recovered**: both endpoints were emitted
at the exact expected line-7 slices — `Application(daftprompt)` 134-144 (conf
1.0) and `Tool(akar)` 270-274 (conf 0.595) — but with no relation candidate
the motivating `daftprompt uses akar` claim exists only as unbound entities.
R1-R9: **0/9 recovered** vs small 1/9 (R1) and base 2/9 (R1, R8) — R4
`missing_entity` (subject `cargo check --workspace` never emitted; the L81
object `canonical correctness gate` was emitted as ProcessRole conf 1.0);
R1-R3/R5-R9 `missing_relation` with **both endpoints emitted in all eight** —
multi has the best entity coverage of the three models (8/9 facts with both
entities vs base 7/9, small 4/9): R2's `gitoxide` is a clean Tool span on L7
(small had a broken-boundary span, base omitted it), R3's object phrase is an
exact `Subsystem(hybrid search indexer)` L7 span plus `SQLite FTS5`/
`sqlite-vec` TechnologyPair spans, R5's `.rs`/`.ts`/`.tsx`/`.js`/`.jsx` are
all five FileExtensionSet spans on L127 (with `node_modules/` PathSet for
R6), R8's `graph build`/`graph search` Command + `XIAOMI_API_KEY`
Configuration on L45, R9's `substring search` FallbackBehavior on L135.
Every fact lies wholly inside one chunk, so every miss is a relation-decoding
miss, not cross-chunk boundary loss; schema gaps 0. Questions: Q1-Q4
not_covered, Q5/Q6 correct_absence (`scoring/review.json`, Task 7 inspection;
claims list empty — no claim exists to judge). Repeated-entity recall 6/8
(missing `tree-sitter`, `cargo check --workspace`), precision 14/14 on
recorded lines; multi recovers `gitoxide`, `SQLite`, and `XIAOMI_API_KEY`
together (small lost XIAOMI_API_KEY, base lost gitoxide/SQLite).

Differences vs small/base, classified: **relation effects dominate and
invert** — at identical thresholds multi decoded 0 candidates (small 1, base
127, raw per chunk 0/0/0/0/0): the multilingual relation head produced
nothing on this English corpus, losing both R1 (small+base) and R8's
`requires` (base). **Entity effects**: 86 spans vs small 40 / base 122 — best
endpoint coverage and cleanest spans (gitoxide, exact R3 object phrase), with
the familiar kind confusions harmless at entity level (`db.rs`/`code.rs`/
`src/ui/render.rs` → FileExtensionSet, bare question-token `search` →
Feature, `production CLI/UI paths` → PathSet, `wgpu pipeline`/`draw list` →
Subsystem) since no relations bind them. **Schema effects: 0** (same
artifact, gaps 0). **Evidence effects**: no claims at all, so 0 citation-valid
and — unlike base — no misleading mass to review (base 101 misleading from
wrong bindings; multi's failure mode is silence, not fabrication).
**Chunking effects: 0** (same 5-chunk plan). Costs: 356.6 s wall including
the 1.1 GB cache fill (vs small 112.7 s / 290 MB, base 252.0 s / 746 MB);
warm-cache rerun 27.5 s wall (vs small 16.9-22.4 s warm); per-chunk
extraction 2.1-4.0 s, sum 15.7 s for 5 chunks / 1445 words (warm 2.0-3.6 s /
14.8 s; vs small 1.5-2.6 s / 9.8 s, base 2.0-6.0 s / 16.6 s) — multi sits
between small and base on per-chunk cost; peak RSS 3,221,438,464 bytes cold /
2,565,095,424 warm (1.42× base's 2,262,417,408 cold; 3.36× small's
959,266,816). Threshold tuning: none performed (untuned like-for-like); the
zero-candidate decode means threshold choice is not the binding constraint
for multi — noted for Task 8. Reproducibility (manifest-verified): model
`fastino/gliner2.5-multi-v1` HF revision
`2ca71aafb3446d9014e1c55c7ff51c9bc7209c47`, `config.json` sha256 `8b59a0f4…`,
gliner2 2.0.0 / torch 2.14.0 / transformers 4.57.6 / huggingface_hub 0.36.2 /
Python 3.14.7; macOS-15.3.1 arm64, device auto→mps (no CUDA); JointIE
thresholds recorded identical to small/base (beam, beam_size 32, candidate/
relation-role 0.05, top-k 32/12). Tests: 18/18 harness, 18/18 scoring, 11/11
schema, `cargo check --workspace` clean; frozen inputs re-verified untouched
(71/71 `check_inputs.py`).

**Acceptance criteria:**

- [x] A complete multi-model run and per-fact scorecard exist, or a
      reproducible checkpoint/runtime incompatibility is documented.
- [x] The same snapshot/schema/scorer hashes are verified across small,
      base, and multi runs; any deliberate deviation is called out.

### Task 8: Compare results and decide the next architecture

**Status:** Done

**Depends on:** Tasks 5-7.

**Change:** Compare the three GLiNER2.5 runs with each other and with the
clearly identified Epic 013 LLM baseline. Give a claim-level explanation of
the `daftprompt uses akar` result and the remaining R1-R9 misses. Assess
whether JointIE's typed decoding helped, whether schema discovery needs an
additional pass, and whether any LLM step is still needed for relation
extraction or entailment review. Recommend one next step: integrate an
experimental backend, revise schema/model/chunking and rerun, or stop this
approach. Integration into the production CLI is a separate epic.

**Implementation notes (Task 8):** Analysis + documentation only — no models
downloaded, no extraction rerun, no changes to frozen-inputs/, run artifacts,
harness/, schema/, scoring/, or the production CLI. Full report:
`experiments/open-vocab-graph/REPORT-gliner25.md` (every metric linked to a
saved artifact path). Comparison labels enforced in the report: the three
GLiNER2.5 runs are like-for-like (byte-identical schema artifact `366fa7e5…`,
snapshot `a15dbdb0…`, seed `c776d7e579d8aed5`, reference set `7dad11a105072baf`,
scorer v1 — verified across the three `manifest.json`s), so model-only deltas
are allowed between them; the Epic 013 budget-250 baseline
(`frozen-inputs/llm-baseline/budget-250-rescore.{json,md}`) shares snapshot/
reference set/seed but uses a non-identical schema and extraction/evidence
contract, so all baseline comparisons are labeled cross-schema context, and
Epic 013's stale 31-claim column / budgets 150/400/600 (seed `9bd9ffae74304ce2`)
are labeled cross-seed and never compared. Headline (GLiNER scorecards):
supported/weak/misleading = 1/0/0 of 1 (small), 18/8/101 of 127 (base), 0
(multi, silent); R1-R9 = 1/9 (R1), 2/9 (R1, R8), 0/9 vs baseline 0/9;
line-7 claim `daftprompt uses akar` recovered by small (C-001, conf 0.358,
citation 7-7) and base twice (C-001 conf 0.839 / C-002 conf 0.822) but not by
multi (both endpoints emitted at the exact slices — Application 134-144 conf
1.0, Tool 270-274 conf 0.595 — with 0 relation candidates decoded in all 5
chunks: relation-decoding silence, not entity/schema/chunking failure).
R-miss classes per model recorded at claim level in the report (small: 3
missing_relation + 5 missing_entity incl. broken-boundary `gitoxide` span;
base: 2 missing_entity regressions + 5 missing_relation incl. chunk-3
zero-relation decode; multi: 1 missing_entity + 8 missing_relation despite best
both-endpoint coverage 8/9). Schema gaps 0 and cross-chunk misses 0 in all
three runs. Assessment: JointIE typed decoding bought schema conformance and
determinism but not recall or binding truth (base 127/127 structurally accepted
with 101 misleading — citation validity ≠ entailment); schema discovery needs
no additional pass (gaps 0) but deterministic tightening of `searches`
descriptions targets 89/101 base misleading claims; an LLM-or-human entailment
review step remains mandatory and an LLM relation step is still needed for
broad recall (GLiNER ceiling 2/9), while the Epic 013 discovery pass is
droppable. Recommendation (one next step): revise the decoding step and rerun
once — run `fastino/gliner2.5-small-v1` with the harness's untested
`independent` variant on the byte-identical frozen contract (schema/chunking/
thresholds held fixed as measured non-levers), with a stop condition if
supported-fact recall does not lift past 2/9. Smallest model meeting the
measured quality bar (beat the baseline on recovery and misleading rate, hold
100% citation validity and Q5/Q6 correct absence, at lowest cost): **small**
(1/9 vs 0/9, 0% vs 20% misleading, 959 MB peak RSS / 290 MB cache / ~9.8 s) —
with the explicit caveat that no model meets the epic's absolute recovery goals
(ceiling 2/9), so none is production-ready; base fails the misleading bar
(79.5%, 2.36× memory) and multi fails recovery and the line-7 case (0/9, 3.36×
memory). Production-CLI integration is not recommended from this evidence and
remains a separate epic.

**Acceptance criteria:**

- [x] A concise report links every metric to saved run artifacts and does
      not compare different seeds as if only the model changed.
- [x] The recommendation weighs supported-fact recall and misleading claims
      alongside speed and memory; it names the smallest model meeting the
      measured quality bar, if any.
- [x] Update this epic's task statuses and overall status when finished.

## Intended file changes

| Path | Change |
|---|---|
| `experiments/open-vocab-graph/` | Frozen inputs, schema adapter, Python extraction harness (`harness/`), scoring, tests, and per-model run artifacts (`gliner-runs/`, git-ignored). |
| `.gitignore` | Ignore the repo-local `.venv` and downloaded model/cache artifacts. |
| `epics/014-gliner25-graph-extraction-experiment.md` | Task progress and final comparison report. |

The existing graph CLI, its prompts, validator, and SQLite tables are outside
this epic's implementation scope.
