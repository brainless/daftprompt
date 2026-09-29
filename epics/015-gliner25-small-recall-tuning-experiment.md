# Epic 015: GLiNER2.5-small Recall Tuning Experiment

## Overview

Epic 014 compared three GLiNER2.5 JointIE checkpoints on the frozen AGENTS.md
benchmark. `small` was the only model that beat the Epic 013 LLM baseline
while staying clean: 1 relation candidate emitted, 1 accepted, 0 rejected,
0 misleading, and it recovered the motivating `daftprompt uses akar` line-7
case that the LLM baseline got wrong. `base` inverted this: 127 candidates,
all structurally valid against the schema's typed pairs, but 101 judged
misleading on review — passing pair validation is not the same as being
true. `multi` decoded zero relation candidates despite finding both entities
correctly in almost every reference fact. The epic concluded the bottleneck
is relation-decoding behavior, not schema or entity coverage (schema gaps
were 0 in every run), and it recommended one specific next step: rerun
`small` with the harness's already-implemented but never-executed
`independent` extraction variant, with a stop condition if reference-fact
recovery does not lift past 2/9.

This epic picks up that recommendation and widens it: `small`'s failure mode
is under-generation (low recall, not precision), and Epic 014 ran every
model at identical, explicitly untuned JointIEConfig defaults
(`candidate_threshold=0.05`, `relation_role_threshold=0.05`,
`top_k_entities=32`, `top_k_roles=12`, `beam_size=32`, `optimizer="beam"`).
The harness (`experiments/open-vocab-graph/harness/`) already exposes all of
these as CLI flags and already implements the untested `independent`
variant, so several experiments here require no code changes at all. Only
per-relation thresholds and richer relation/entity descriptions require
touching `schema/core.py` and regenerating `schema/gliner25-schema.json`.

Because `small` is cheap (17-113 s wall, ~1 GB peak RSS in Epic 014's runs),
this epic can afford a genuine sweep rather than a single tuned run.

**Status:** Done (2026-09-28) — all five tasks complete. No lever (threshold/
top-k sweep, optimizer, `independent` variant, schema change) improved on
Epic 014's canonical `small` run (1/9 recovery, 0% misleading); the
`independent` stop condition was not met (0/9). Decision (Task 5,
`experiments/open-vocab-graph/REPORT-small-tuning.md`): decoding/schema tuning
has a ceiling here — adopt nothing, keep the canonical config as the `small`
baseline, and move next to a different approach (fine-tuning, a different
model family, or LLM relation extraction), with the LLM-or-human entailment
review gate retained in any pipeline.

## Goals

- Determine whether decoding-level tuning (thresholds, top-k, optimizer) or
  the `independent` variant lifts `small`'s reference-fact recovery above
  Epic 014's ceiling of 1/9 (R1) without reintroducing `base`'s misleading-
  claim problem.
- If decoding tuning plateaus, determine whether schema-level changes
  (per-relation thresholds, sharper descriptions for the five relations no
  model ever emitted: `serves_as`, `indexes`, `excludes`, `fuses`,
  `degrades_to`) unlock additional recall.
- Keep every run comparable to Epic 014's canonical small run
  (`gliner-runs/fastino_gliner2.5-small-v1/jointie/20260928T112911Z/`) by
  reusing the same frozen snapshot, seed, reference set, and scoring
  contract, varying only the stated lever per task.
- Produce a decision: adopt a specific tuned configuration as the new
  `small` baseline, or conclude that tuning does not move the ceiling and
  the approach needs a different next step.

## Non-goals

- Testing `base` or `multi` again. This epic is `small`-only; the schema
  and threshold changes it validates may later be worth re-testing against
  `base`/`multi`, but that is out of scope here.
- Fine-tuning GLiNER2.5 weights. This epic only changes decoding
  configuration, prompt/schema descriptions, and extraction variant.
- Changing the frozen inputs (`experiments/open-vocab-graph/frozen-inputs/`),
  the reference set, or the snapshot. All comparisons must stay
  apples-to-apples with Epic 014.
- Integrating any result into the production `graph` CLI. That remains a
  separate epic, as stated in Epic 014.

## Design constraints

- Every run in this epic must record the same manifest fields Epic 014
  required: model revision, package/config hashes, device, thresholds,
  chunking record, schema/snapshot hashes, elapsed time, and peak RSS. Reuse
  the existing `runner.py` manifest machinery; do not weaken it.
- Never overwrite an existing run directory. Each task's runs go under
  `gliner-runs/fastino_gliner2.5-small-v1/<variant>/<UTCstamp>[-label]/` as
  the harness already does.
- Any schema change (Task 4) must go through `schema/build.py` so the
  artifact stays deterministic and auditable; do not hand-edit
  `gliner25-schema.json`. Record the new schema artifact's hash and diff it
  against Epic 014's `366fa7e5…` schema in every run manifest from that
  point on, and re-verify `frozen-inputs/check_inputs.py` (71/71) before
  scoring any run built on the new schema.
- Score every run with the existing `scoring` package
  (`python -m scoring score <run_dir>`) so taxonomy codes, citation
  grounding, and the supported/weak/misleading review stay consistent with
  Epic 014's definitions. Do not invent a new scoring method for this epic.
- A lever that increases recall by also increasing misleading claims is not
  automatically a win. Every task's acceptance criteria must report recall
  and misleading-rate together, the same way Epic 014's Task 8 did.
- Preserve Epic 014's runs and reports untouched; this epic writes new runs
  and a new report, it does not edit `REPORT-gliner25.md`.

## Tasks

### Task 1: Decoding threshold and top-k sweep (JointIE, small)

**Status:** Done (2026-09-28). No configuration improved on the canonical
run — **no leading candidate**; Task 2 falls back to Epic 014's defaults per
its Depends-on note.

**Change:** Using the existing `harness` CLI flags
(`--candidate-threshold`, `--relation-role-threshold`, `--top-k-entities`,
`--top-k-roles`), run `fastino/gliner2.5-small-v1` with `jointie` across a
small grid that lowers the candidate/relation-role thresholds below 0.05
and raises `top_k_entities`/`top_k_roles` above 32/12. No code changes
should be required for this task. Score each run with the existing scoring
package and tabulate relation-candidate count, accepted count, R1-R9
recovery, and supported/weak/misleading against Epic 014's canonical small
run as the fixed reference point.

**Runs** (all `fastino/gliner2.5-small-v1`, variant `jointie`, warm
`.model-cache` + `--local-files-only`, all other JointIE knobs at harness
defaults). Run directories share the prefix
`gliner-runs/fastino_gliner2.5-small-v1/jointie/`:

| Config | candidate / relation-role thr | top-k entities / roles | Run id |
|---|---|---|---|
| canonical (Epic 014) | 0.05 / 0.05 | 32 / 12 | `20260928T112911Z` |
| sweep-1 | 0.02 / 0.02 | 64 / 24 | `20260928T125435Z-sweep-1` |
| sweep-2 | 0.01 / 0.01 | 64 / 24 | `20260928T125506Z-sweep-2` |
| sweep-3 | 0.005 / 0.005 | 128 / 48 | `20260928T125527Z-sweep-3` |
| sweep-4 | 0.001 / 0.001 | 256 / 64 | `20260928T125549Z-sweep-4` |

**Comparison** (scored with `python -m scoring score <run_dir>`; support
judgments filled per run in `scoring/review.json` by agent review against
the frozen snapshot, same contract as Epic 014 Task 5/6):

| Config | R1-R9 recovery | Candidates emitted / accepted | Supported / weak / misleading | Misleading rate | Wall time | Peak RSS |
|---|---|---|---|---|---|---|
| canonical | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 112.7 s ¹ | 959 MB |
| sweep-1 | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 13.6 s | 1134 MB |
| sweep-2 | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 13.5 s | 1038 MB |
| sweep-3 | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 13.5 s | 1339 MB |
| sweep-4 | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 11.9 s | 1349 MB |

¹ The canonical 112.7 s includes cold-cache checkpoint download; the sweep
runs reused the warm `.model-cache` (~9.8 s/chunk extraction equivalent), so
wall times are not directly comparable. Peak-RSS figures come from each run
manifest (`resource.getrusage`, includes model load); run-to-run allocator
variance dominates the small differences.

**Findings:**

- All four configurations produced output **identical to the canonical run**:
  `entities.json`, `relations.json`, and every per-chunk record (raw model
  responses included) are JSON-equal, ignoring per-chunk `elapsed_seconds`.
  Each run: 40 entity mentions, exactly 1 relation candidate (C-001
  `uses(Application:daftprompt, Tool:akar)`, R1), 0 rejected, 0 misleading.
- **No configuration lifted R1-R9 recovery above 1/9**, so no leading
  candidate exists. The misleading-rate condition (materially below `base`'s
  79.5%) holds trivially at 0% for every run, but there is no recall gain to
  pair with it.
- Interpretation: these four knobs **do not bind** for `small` on this
  snapshot. Lowering `candidate_threshold`/`relation_role_threshold` 50×
  (0.05 → 0.001) surfaces zero additional candidates — the model produces no
  scores in the gated band — and the `top_k_entities`/`top_k_roles` caps
  never engage at 32/12 (raising them to 256/64 changes nothing). `small`'s
  under-generation is upstream of threshold/top-k decoding, consistent with
  Epic 014's "relation-decoding behavior, not schema or entity coverage"
  diagnosis. Remaining levers are the optimizer (Task 2), the `independent`
  variant (Task 3), and schema-level changes (Task 4).

**Acceptance criteria:**

- [x] At least 4 distinct threshold/top-k configurations are run and scored,
      each in its own `gliner-runs/.../jointie/<UTCstamp>-sweep-N/` directory
      with a full manifest. (sweep-1..4 above; manifests record the varied
      thresholds.)
- [x] A short comparison table (recall, misleading rate, candidate count,
      wall time, peak RSS) against Epic 014's canonical small run is written
      to this epic (or a linked artifact) before moving to Task 2. (above)
- [x] If any configuration lifts R1-R9 recovery above 1/9 while keeping the
      misleading rate materially below `base`'s 79.5%, it is flagged as the
      leading candidate configuration. (Evaluated: no configuration lifted
      recovery above 1/9, so there is no leading candidate; outcome stated
      in Findings.)
- [x] `harness` and `scoring` test suites still pass unchanged (no code was
      touched in this task). (`harness/test_harness.py` 18/18,
      `scoring/test_scoring.py` 18/18, unmodified.)

### Task 2: Optimizer comparison (greedy vs. beam)

**Status:** Done (2026-09-28). Greedy does not change recall or misleading
outcomes and delivers no speed win; **not worth adopting** — beam stays the
default.

**Depends on:** Task 1 (reuses its best threshold/top-k setting, or defaults
if Task 1 found no improvement).

**Change:** Re-run `small`/`jointie` with `optimizer="greedy"` against the
same thresholds used in Task 1's leading configuration (or Epic 014's
defaults if Task 1 found nothing better), and compare against the
equivalent `beam` run on recall, misleading rate, wall time, and peak RSS.

**Runs** (both `fastino/gliner2.5-small-v1`, variant `jointie`, Epic 014
default thresholds — Task 1 found no better setting, so per the Depends-on
note both runs use `candidate_threshold=0.05`, `relation_role_threshold=0.05`,
`top_k_entities=32`, `top_k_roles=12`, passed explicitly; warm
`.model-cache` + `--local-files-only`). Run directories share the prefix
`gliner-runs/fastino_gliner2.5-small-v1/jointie/`:

| Config | optimizer | Run id |
|---|---|---|
| greedy | `greedy` | `20260928T130455Z-greedy` |
| beam counterpart (fresh warm-cache) | `beam` | `20260928T130515Z-beam` |

The beam reference used throughout is the **fresh warm-cache run
`20260928T130515Z-beam`**, not the cold-cache canonical run, so wall times
are directly comparable. The fresh beam run was verified JSON-equal to the
canonical small run `20260928T112911Z` (entities, relations, and every
per-chunk record ignoring `elapsed_seconds`), so it is an exact stand-in for
the fixed baseline on all outcome metrics.

**Comparison** (scored with `python -m scoring score <run_dir>`; support
judgments filled per run in `scoring/review.json` by agent review against
the frozen snapshot, same contract as Task 1):

| Config | R1-R9 recovery | Candidates emitted / accepted | Supported / weak / misleading | Misleading rate | Wall time | Peak RSS |
|---|---|---|---|---|---|---|
| greedy | 1/9 (R1) | 2 / 2 ¹ | 2 / 0 / 0 | 0% | 13.3 s | 1128 MB |
| beam (warm-cache) | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 13.2 s | 1167 MB |
| canonical (Epic 014, cold-cache) | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 112.7 s ² | 959 MB |

¹ Greedy's second candidate is not a new fact: the same
`uses(Application:daftprompt, Tool:akar)` claim emitted once per distinct
`daftprompt` mention span (doc 64-74 on line 3 and doc 134-144 on line 7).
The 134-144 claim is byte-identical (same confidence 0.3584) to the
canonical/beam claim; the 64-74 claim is scored as a span-duplicate (1
duplicate group, C-001/C-002) of the same R1 fact, not an additional
recovery. Entity output is unchanged: greedy and beam both emit 40 entity
mentions, JSON-equal to each other and to the canonical run.

² The canonical 112.7 s includes cold-cache checkpoint download; all Task 2
wall times are warm-cache and mutually comparable. Peak-RSS figures come
from each run manifest (`resource.getrusage`, includes model load);
run-to-run allocator variance dominates the small differences (Task 1 saw
1038-1349 MB across identical runs).

**Findings:**

- Greedy **does change the output**, but only in one way: it decodes the R1
  relation twice, once per `daftprompt` mention span in the chunk, where
  beam decodes it once. Both claims are schema-valid and review as
  supported against frozen line 7 (the 64-74 claim cites AGENTS.md:3-7,
  which contains the supporting sentence on line 7). No new reference fact
  is recovered and no false claim appears.
- **Recall unchanged at 1/9** (recovery is per-fact; the span-duplicate
  scores as a duplicate group) and **misleading rate unchanged at 0%** for
  both optimizers. Greedy changes neither outcome materially.
- **No speed difference:** 13.3 s (greedy) vs 13.2 s (beam) warm-cache on
  5 chunks — decoding cost is negligible next to the encoder forward
  passes, and the 0.1 s delta is within run-to-run noise. Peak RSS is
  likewise indistinguishable (1128 vs 1167 MB). There is no "(likely) speed
  difference" to trade against.
- Verdict: **greedy is not worth the switch.** Its only behavioral effect
  is a span-duplicate of the already-recovered fact — noise, not coverage —
  and it buys nothing in wall time or memory. `beam` remains the default
  optimizer. The optimizer lever is now exhausted alongside the
  threshold/top-k lever; remaining levers are the `independent` variant
  (Task 3) and schema-level changes (Task 4).

**Acceptance criteria:**

- [x] One greedy run and its beam counterpart exist under the same
      threshold settings, each scored and manifest-complete. (`…-greedy` and
      `…-beam` above; both manifests record identical default thresholds and
      the varied `optimizer`, each scored with a filled `scoring/review.json`.)
- [x] The report states whether greedy changes recall/misleading outcomes
      materially, and whether it is worth the (likely) speed difference.
      (Findings: no material recall/misleading change, no measurable speed
      difference; not worth adopting.)

### Task 3: `independent` variant on small

**Status:** Done (2026-09-28). `independent` at the harness default threshold
emits **zero relation candidates** and recovers **0/9** — strictly worse than
`jointie`'s 1/9. The stop condition (supported-fact recall past 2/9) is
**not met**; no `independent` threshold sweep was run.

**Depends on:** none (can run in parallel with Task 1-2, but should be
scored against the same reference point).

**Change:** Run the harness's existing, previously untested `independent`
variant against `fastino/gliner2.5-small-v1`
(`python -m harness extract --model fastino/gliner2.5-small-v1 --variant
independent`), first at the harness's default threshold, then swept the
same way as Task 1 if the default result looks promising. This is the
specific next step Epic 014 Task 8 recommended and stop-conditioned: if
supported-fact recall does not lift past 2/9 here either, the epic's
Task 5 conclusion should say so plainly rather than continuing to sweep
`independent`'s threshold indefinitely.

**Runs** (`fastino/gliner2.5-small-v1`, variant `independent`, harness
default `threshold=0.5` (CLI default, not passed explicitly), warm
`.model-cache` + `--local-files-only`, chunking/schema/snapshot identical to
every prior run). Run directories share the prefix
`gliner-runs/fastino_gliner2.5-small-v1/independent/`:

| Config | threshold | Run id |
|---|---|---|
| independent default | 0.5 (harness default) | `20260928T132158Z` |

No sweep runs exist: the default result failed the "promising (recall >
1/9)" gate below, and the stop condition forbids threshold sweeping once it
is clear recall will not lift past 2/9.

**Comparison** (scored with `python -m scoring score <run_dir>`; support
judgments filled per run in `scoring/review.json` by agent review against
the frozen snapshot, same contract as Tasks 1-2):

| Config | R1-R9 recovery | Candidates emitted / accepted | Supported / weak / misleading | Misleading rate | Citation validity | Wall time | Peak RSS |
|---|---|---|---|---|---|---|---|
| independent default (0.5) | 0/9 | 0 / 0 | 0 / 0 / 0 | vacuous (no claims) | n/a (0 claims, 0 citations) | 8.6 s | 1154 MB |
| canonical jointie (Epic 014, cold-cache) | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 1/1 valid | 112.7 s ¹ | 959 MB |
| jointie defaults, warm beam (Task 2 `20260928T130515Z-beam`) | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 1/1 valid | 13.2 s | 1167 MB |

Task 1's leading `jointie` configuration: **none** — all four sweep configs
were output-identical to the canonical run at 1/9, so the canonical
configuration (represented warm-cache by `20260928T130515Z-beam`, JSON-equal
to the canonical run) *is* the leading `jointie` configuration to beat.
`independent` does not beat it: 0/9 vs 1/9 recovery, and its 0% misleading
rate is vacuous (zero claims exist to be misleading) rather than a precision
win.

¹ The canonical 112.7 s includes cold-cache checkpoint download; both other
wall times are warm-cache and directly comparable to each other.
Peak-RSS figures come from each run manifest (`resource.getrusage`, includes
model load); run-to-run allocator variance dominates (Task 1 saw 1038-1349
MB across identical runs). Entity behavior for context: `independent`
emitted 32 deduplicated mentions (33 raw) vs `jointie`'s 40, with the same
6/8 repeated-entity recall (missing tree-sitter, XIAOMI_API_KEY).

**Findings:**

- The `independent` variant found the entities but decoded **no relations at
  all**: the raw `relation_extraction` output in every chunk record is empty
  for all nine labels, so zero candidates were emitted, accepted, or
  rejected. R1-R4 diagnose as `missing_relation` ("both entities emitted but
  no relation candidate covers the fact") — including R1, the one fact
  `jointie` recovers (both endpoints `daftprompt`/`akar` were emitted on
  line 7); R5-R9 diagnose as `missing_entity`, same as `jointie`.
- **Recall 0/9 vs `jointie`'s 1/9**: `independent` is strictly worse on this
  benchmark. Supported/weak/misleading is 0/0/0 — clean only vacuously (no
  claims exist to mislead). Citation validity is untestable (0 claims, 0
  citations) vs `jointie`'s 1/1 valid.
- Caveat kept on the record: gliner2's `_extract_relations` gates head/tail
  spans at the extraction threshold before output, so the empty relation set
  cannot distinguish "the relation head scored nothing" from "everything
  scored below 0.5"; a lower `--threshold` could in principle surface
  candidates. The default result (0/9) fails the "promising (recall > 1/9)"
  gate above, so per this task's protocol **no threshold sweep was started**,
  and the stop condition below forbids continuing to sweep `independent`'s
  threshold.
- Verdict: the `independent` lever does not lift the ceiling — it sinks
  recall below `jointie`'s. Combined with Tasks 1-2, decoding-level tuning
  (threshold/top-k, optimizer, variant) is now exhausted; Epic 014's
  "relation-decoding behavior, not schema or entity coverage" diagnosis
  stands.

**Stop condition (supported-fact recall past 2/9): NOT MET.** Recall is 0/9
at the harness default threshold and never approaches the 2/9 bar. Per the
epic's explicit stop condition, `independent` threshold sweeping is
**stopped here and stated plainly**: no sweep runs were made (the default
result was not promising at 0/9 ≤ 1/9), and Task 5 should state that
`independent`-variant threshold tuning does not move the ceiling rather than
continuing to sweep it.

**Acceptance criteria:**

- [x] At least one `independent`-variant run on `small` is executed, scored,
      and manifest-complete, with kinds attached post-hoc by span match and
      pairs enforced by the validator exactly as `IndependentAdapter` already
      does. (`20260928T132158Z` above; `IndependentAdapter`/`harness` left
      unchanged — kinds attached post-hoc by span match, pairs enforced via
      `core.validate_claim`; manifest records variant `independent`,
      `thresholds: {threshold: 0.5}`, model config sha256, package versions,
      device (mps), chunking record, schema `366fa7e5…` + snapshot sha256,
      8.56 s elapsed, peak RSS 1210040320 B; scored with a filled
      `scoring/review.json`.)
- [x] R1-R9 recovery, supported/weak/misleading, and citation validity are
      reported and compared directly against Epic 014's canonical `jointie`
      small run and against Task 1's leading `jointie` configuration.
      (Comparison above: 0/9 vs 1/9 recovery; 0/0/0 vs 1/0/0 support split
      (0% misleading vacuously, no claims exist); citation validity untestable
      (0/0) vs 1/1 valid. Task 1's leading configuration is none — all
      defaults-equal — so the canonical/defaults configuration is the
      reference.)
- [x] The stop condition (recall past 2/9) is evaluated explicitly and its
      outcome stated, whether met or not. (Not met — 0/9; stated in the Stop
      condition paragraph above.)

### Task 4: Per-relation thresholds and sharper descriptions for zero-emission predicates

**Status:** Done (2026-09-28). Schema change landed (per-relation threshold
0.25 + snapshot-echoing description rewrites for the five zero-emission
predicates; new artifact `9e687ed8…` superseding `366fa7e5…`) and the
post-change `small`/`jointie` run changed **nothing**: **none of the five
previously-silent predicates were emitted at all**, output is JSON-equal to
the canonical run, and recovery stays 1/9 at 0% misleading (cross-schema
comparison, labeled below). Schema-level tuning is exhausted alongside the
decoding-level levers.

**Depends on:** Tasks 1-3 (only proceed if decoding-level tuning and the
`independent` variant both plateau at or below Epic 014's 1-2/9 ceiling;
otherwise this task is optional and should be re-scoped based on what was
learned).

**Change:** `schema/core.py::to_joint_schema` currently declares every
relation with a bare `.relation(label, head, tail, description=...)` — no
per-relation `threshold`, and no graph constraints. Add per-relation
threshold support (lower thresholds specifically for `serves_as`,
`indexes`, `excludes`, `fuses`, `degrades_to`, the five relations no model
in Epic 014 ever emitted) and rewrite their descriptions to more closely
echo the source's own phrasing (e.g. pull vocabulary directly from the
`origin: added_task2` support lines already recorded in
`gliner25-schema.json`, per Epic 014 Task 2's audit trail), following the
GLiNER2 tutorial's stated best practice of providing clear, specific,
example-grounded relation descriptions. Regenerate the schema artifact via
`schema/build.py`, record its new hash, and re-run `small`/`jointie` (and,
if Task 3 showed independent was competitive, `small`/`independent`) on the
updated schema.

**Implementation:** `schema/core.py::Predicate` and `JointIELabel` gained a
`threshold: Optional[float]` field; `derive_joint_ie_labels` propagates a
predicate's threshold to every derived label and `to_joint_schema` forwards
it verbatim to `JointSchema.relation(threshold=...)`. `schema/build.py`
carries `TASK4_PREDICATE_TUNING` (threshold 0.25 + revised description +
grounding lines per predicate) into the artifact as `predicates[].threshold`,
`description_origin: "revised_task4"`, `description_previous`, and
`description_support` (Task 2's (line, quote, reason) shape, verified against
the frozen snapshot at build time), plus a `revision` block recording
`supersedes_artifact_sha256: 366fa7e5…`. Descriptions were rewritten to echo
the frozen snapshot's own phrasing, with vocabulary pulled from each
predicate's `added_task2` support lines. Threshold 0.25 is the JointIE
tutorial's demonstrated per-relation lowering; rejected alternatives (0.1 —
re-opens `base`'s misleading flood; 0.4 — too timid) are recorded in
`build.py`. Only the five silent predicates are lowered; `uses` keeps the
default 0.5 bar so R1 stays comparable to the canonical run. The thresholds
genuinely bind at decode time: gliner2's candidate builder centers
relation-role logits on each label's `hypothesis.threshold`
(`joint_ie/candidates.py`, default 0.5).

**Runs** (`fastino/gliner2.5-small-v1`, variant `jointie`, Epic 014 default
JointIEConfig thresholds passed explicitly (0.05/0.05, 32/12), warm
`.model-cache` + `--local-files-only`, chunking/snapshot identical to every
prior run). `independent` was **not** competitive in Task 3 (0/9), so per the
Change's "if applicable" clause the post-change rerun is `jointie`-only. Run
directory under `gliner-runs/fastino_gliner2.5-small-v1/jointie/`:

| Config | Schema artifact | Run id |
|---|---|---|
| task4 schema (thresholds 0.25 on the five predicates + rewritten descriptions) | `9e687ed8…` (supersedes `366fa7e5…`) | `20260928T160412Z-task4-schema-supersedes-366fa7e5` |

The run manifest records the new schema hash
(`9e687ed88f84a0fcb26d92596f8c3cbe31b85208e7f575e06b021cf96fa7cd11`) and,
via its run id and the artifact's `revision` block the hash covers, that it
differs from/supersedes `366fa7e5…`. `frozen-inputs/check_inputs.py` was
re-run before scoring: 71/71 untouched.

**Comparison** (scored with `python -m scoring score <run_dir> --force` after
a filled `scoring/review.json`, same agent-review contract as Tasks 1-3).
**Cross-schema labeling:** the Task 4 row runs the NEW schema `9e687ed8…`;
the baseline rows run the OLD schema `366fa7e5…`. Outcomes are identical
across the schema change, but the comparison is cross-schema and must be
labeled as such:

| Run | Schema | R1-R9 recovery | Candidates emitted / accepted | Supported / weak / misleading | Misleading rate | Citation validity | Wall time | Peak RSS |
|---|---|---|---|---|---|---|---|---|
| task4 schema (new) | `9e687ed8…` | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 1/1 valid | 14.1 s | 1285 MB |
| canonical (Epic 014, cold-cache) | `366fa7e5…` (old) | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 1/1 valid | 112.7 s ¹ | 959 MB |
| jointie defaults, warm beam (Task 2 `20260928T130515Z-beam`) | `366fa7e5…` (old) | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 1/1 valid | 13.2 s | 1167 MB |

¹ The canonical 112.7 s includes cold-cache checkpoint download; the Task 4
run is warm-cache and directly comparable to the Task 2 warm beam (14.1 vs
13.2 s — within noise). Peak-RSS figures come from each run manifest
(`resource.getrusage`, includes model load); run-to-run allocator variance
dominates the differences (Task 1 saw 1038-1349 MB across identical runs).

**Findings:**

- **None of the five previously-silent predicates were emitted at all.**
  `serves_as`, `indexes`, `excludes`, `fuses`, and `degrades_to` produced
  **zero relation candidates** — verified in the raw per-chunk output: the
  only relation decoded anywhere is `uses` (chunk 0), and the five labels
  appear in the run's chunk files only inside the quoted source text, never
  in model output. Lowering the per-relation decision bar to 0.25 and
  rewriting the descriptions to echo the snapshot's own phrasing changed
  nothing at the decoder.
- **Output is JSON-equal to the canonical run** (cross-schema, labeled):
  `entities.json`, `relations.json`, and all five per-chunk records match the
  canonical `20260928T112911Z` run byte-for-byte ignoring
  `elapsed_seconds`. Recall stays **1/9 (R1)**, supported/weak/misleading
  **1/0/0**, misleading rate **0%** — identical to the baseline. The schema
  levers (per-relation threshold + description wording) do not move `small`'s
  decoding on this snapshot.
- The diagnosis nuance worth keeping: R2-R4 remain `missing_relation` (both
  endpoint entities emitted, no relation candidate) — for R4 both
  `cargo check --workspace` and `canonical correctness gate` were emitted as
  entities and `serves_as` still decoded nothing even at 0.25. R5-R9 remain
  `missing_entity`, same as every prior run. This is the same
  relation-decoding wall, now demonstrated immune to decoding-level
  (Tasks 1-3) and schema-level (Task 4) tuning alike.
- The threshold change was verified to bind end-to-end: the built
  `JointSchema` carries `threshold=0.25` on exactly the five labels
  (`schema/test_schema.py`, no model download), `to_joint_schema` forwards it
  to `JointSchema.relation(threshold=...)`, and gliner2's candidate builder
  gates each label's relation-role logits on that per-label threshold
  (`joint_ie/candidates.py`). The run's manifest records schema
  `9e687ed8…`, so the run provably executed under the tuned labels.

**Acceptance criteria:**

- [x] `schema/core.py` supports declaring a per-relation threshold that
      `to_joint_schema` passes through to `JointSchema.relation(...)`, with
      a unit test (no model download) asserting the threshold is present in
      the built `JointSchema` for at least one predicate. (Implemented:
      `Predicate`/`JointIELabel.threshold` → `to_joint_schema` forwards
      `threshold=spec.threshold`; `test_per_relation_thresholds_pass_to_joint_schema`
      asserts 0.25 lands on all five built `JointSchema` relation specs and
      `uses`/`uses__2` keep `None` — no model download.)
- [x] The five zero-emission predicates' descriptions are rewritten and their
      revised text, plus the source lines they were grounded against, are
      recorded in the schema package the same way Task 2's `origin`
      provenance already is. (`description_origin: "revised_task4"`,
      `description_previous`, `description_support` with verbatim snapshot
      quotes + reasons on every one of the five predicates in
      `gliner25-schema.json`, verified at build time and by
      `test_task4_description_revisions_are_grounded`; documented in
      `schema/README.md`.)
- [x] `schema/build.py --check` passes, the new artifact's hash is recorded,
      and `frozen-inputs/check_inputs.py` still reports 71/71 untouched.
      (`--check` ok; artifact sha256
      `9e687ed88f84a0fcb26d92596f8c3cbe31b85208e7f575e06b021cf96fa7cd11`
      recorded in `build.py`/`schema/README.md`/the run manifest, differing
      from `366fa7e5…` in exactly the threshold/description/revision fields;
      `check_inputs.py` re-run: all 71 checks passed.)
- [x] At least one full `small` run (jointie, and independent if
      applicable) on the updated schema is executed and scored; the report
      states whether any of the five previously-silent predicates were
      emitted at all, and with what recall/misleading outcome.
      (`20260928T160412Z-task4-schema-supersedes-366fa7e5` executed and
      scored with a filled review pass; independent not applicable per Task
      3. Findings above: the five predicates were **not emitted at all** —
      zero candidates; recall 1/9 and 0% misleading, identical to canonical
      (cross-schema, labeled).)
- [x] `schema/test_schema.py` and `harness/test_harness.py` pass unchanged
      apart from additions needed to cover the new per-relation threshold
      field. (`schema/test_schema.py` 13/13 with the two Task 4 tests added;
      `harness/test_harness.py` 18/18 and `scoring/test_scoring.py` 18/18,
      both byte-unmodified. Pre-existing flake noted on the record:
      `test_runner_end_to_end_with_fake_engine` fails ~1/30 runs with "run
      directory collision must fail visibly" when its second `runner.run()`
      straddles a second boundary against the run dir's second-resolution
      UTC timestamp; it is unrelated to Task 4 — `test_harness.py` is
      unchanged from the committed baseline — and passes on re-run.)

### Task 5: Decide and report

**Status:** Done (2026-09-28). Report written to
`experiments/open-vocab-graph/REPORT-small-tuning.md`. **No configuration
improved on the canonical baseline** (1/9, 0% misleading) — the best
configuration is the baseline itself (untuned defaults, `beam`, schema
`366fa7e5…`, represented warm-cache by `20260928T130515Z-beam`). Decision:
**option (c) — decoding/schema tuning has a ceiling here**; adopt nothing as a
new baseline and move next to a different approach (fine-tuning `small` on
relation examples, a different model family, or LLM relation extraction), with
the LLM-or-human entailment review gate retained in any pipeline.

**Depends on:** Tasks 1-4.

**Change:** Compare every run in this epic against Epic 014's canonical
`small` run as the fixed baseline (1/9 recovery, 0% misleading, 959 MB peak
RSS, ~9.8 s per-chunk extraction). Identify the single best configuration
found (lever + setting), state its recall/misleading/cost trade-off
explicitly, and recommend one of: adopt it as the new `small` baseline for
any future epic, continue tuning with a specific untried lever, or conclude
that decoding/schema tuning has a ceiling here and a different approach
(e.g. the still-open "LLM-or-human entailment review" or a different model
family) is needed next. Write the report to
`experiments/open-vocab-graph/REPORT-small-tuning.md`, linking every metric
to a saved run artifact, and update this epic's task statuses.

**Acceptance criteria:**

- [x] A concise report exists at
      `experiments/open-vocab-graph/REPORT-small-tuning.md`, links every
      metric to a saved run artifact, and does not compare across a changed
      schema without labeling it as such (mirroring Epic 014 Task 8's
      cross-schema labeling discipline). (Report written: every table row
      carries its run id and every metric cites
      `manifest.json`/`summary.json`/`relations.json`/`scoring/`; the Task 4
      row is labeled **cross-schema** (`9e687ed8…` vs `366fa7e5…`) in §0 and
      again in the results table.)
- [x] The recommendation names a specific configuration (or explicitly
      states none improved on the baseline) and gives the reasoning in terms
      of recall vs. misleading rate vs. cost, not recall alone. (§3 states
      explicitly that none improved on the baseline and names the canonical
      config as the best found; trade-off given on all three axes — recall
      1/9 max vs 0/9 independent, misleading 0% earned vs vacuous, cost
      equivalent or worse for every lever; §4 recommends option (c) with the
      Task 3 stop-condition outcome (0/9, not met) in the reasoning.)
- [x] This epic's task statuses and overall status are updated to reflect
      what was actually run. (Task 5 status Done with decision summary;
      overall `**Status:**` updated to Done with the final decision.)

## Intended file changes

| Path | Change |
|---|---|
| `experiments/open-vocab-graph/gliner-runs/fastino_gliner2.5-small-v1/` | New sweep, greedy, independent, and updated-schema run artifacts (git-ignored). |
| `experiments/open-vocab-graph/schema/core.py` | Add per-relation threshold support in `to_joint_schema` (Task 4 only). |
| `experiments/open-vocab-graph/schema/gliner25-schema.json` | Regenerated via `schema/build.py` with per-relation thresholds and revised descriptions for the five zero-emission predicates (Task 4 only). |
| `experiments/open-vocab-graph/schema/test_schema.py` | Test coverage for the new per-relation threshold field (Task 4 only). |
| `experiments/open-vocab-graph/REPORT-small-tuning.md` | New report comparing every lever tried in this epic against Epic 014's canonical small run. |
| `epics/015-gliner25-small-recall-tuning-experiment.md` | Task progress and final decision. |

The existing `harness/` CLI, `scoring/` package, production `graph` CLI, and
Epic 014's frozen inputs and run artifacts are not modified by this epic
except where Task 4 explicitly adds a schema field.
