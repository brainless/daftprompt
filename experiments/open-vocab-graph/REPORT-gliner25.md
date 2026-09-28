# Epic 014 Report — GLiNER2.5 graph extraction experiment (Task 8)

Comparison of the three GLiNER2.5 runs against the Epic 013 LLM baseline and the
recommended next architecture. Every metric below cites a saved artifact; no model
was run for this report.

## 0. Comparison labels (read before any table)

**Like-for-like (model-only deltas allowed):** the three GLiNER2.5 runs share the
byte-identical frozen contract, verified in all three run manifests
(`gliner-runs/<model>/jointie/<stamp>/manifest.json` → `schema`/`snapshot`):
schema artifact sha256 `366fa7e5…` (seed xxh3 `c776d7e579d8aed5`, version 1.0.0),
snapshot sha256 `a15dbdb0…` (146 lines), reference set xxh3 `7dad11a105072baf`,
scorer schema version 1 (zero snapshot/verdict disagreements in every
`scoring/scorecard.json` `integrity`). Identical chunking plan (5 chunks) and
byte-identical JointIE thresholds across all three (untuned). Reruns
(`…T111610Z`/`…T112042Z` for small, `…T121442Z-warm` for multi) are
extraction-byte-identical to their canonical runs — determinism confirmed.

**Cross-schema context only (never model-only deltas):** the LLM baseline is
`frozen-inputs/llm-baseline/budget-250-rescore.{json,md}` — the Epic 013
budget-250 folder (`output/runs/budget-250/`) re-scored under its own recorded
seed. Per `frozen-inputs/comparison-plan.md` §1-2 that run shares the same
snapshot, reference set, and seed hash `c776d7e579d8aed5` with the GLiNER runs,
but its **schema and extraction contract are non-identical** (open-vocabulary
single-shape predicates + mid-run minting vs. the versioned multi-pair GLiNER2.5
schema; model-quoted evidence vs. snapshot-derived citations; 8 LLM blocks of
budget 250 vs. 5 fixed chunks). Every baseline-vs-GLiNER number below is
therefore cross-schema/cross-method context, not a model-only delta.

**Cross-seed, never comparable:** Epic 013's report column "budget 250" (31
claims) and the budgets 150/400/600 runs used seed xxh3 `9bd9ffae74304ce2`
(different kinds and predicate shapes). Per `comparison-plan.md` §1 stale-column
warning, no Epic 014 number may be compared with those.

## 1. Headline results

Artifacts: GLiNER columns from each run's `scoring/scorecard.{json,md}` +
`scoring/review.json` + `summary.json`; baseline column from
`frozen-inputs/llm-baseline/budget-250-rescore.md`.

| Metric | LLM baseline (cross-schema) | small | base | multi |
|---|---|---|---|---|
| Entity mentions (dedup spans) | — | 40 (40) | 123 (122) | 89 (86) |
| Relation candidates / accepted / rejected | 64 / 40 / 24 | 1 / 1 / 0 | 127 / 127 / 0 | 0 / 0 / 0 |
| Citation-valid | 40/40 | 1/1 | 127/127 | 0 (no claims) |
| Supported / weak / misleading | 28 / 4 / 8 | 1 / 0 / 0 | 18 / 8 / 101 | 0 / 0 / 0 |
| Misleading rate | 20.0% | 0% | 79.5% | 0% (silent) |
| Surface duplicates | 0 | 0 | 110 | 0 |
| R1-R9 recovery (valid **and** supported) | 0/9 | 1/9 (R1) | 2/9 (R1, R8) | 0/9 |
| R1-R9 both-endpoints coverage | — | 4/9 | 7/9 | 8/9 |
| Line-7 `daftprompt uses akar` | emitted, rejected | recovered | recovered ×2 | entities only |
| Q1-Q4 (Q5/Q6) | 2 defective / 2 absent (correct absence) | partial Q1 (correct absence) | Q1 covered (correct absence) | none (correct absence) |
| Repeated-entity recall | — | 6/8 | 5/8 | 6/8 |
| Wall time (cold / warm) | 8 serial LLM calls | 112.7 s / 16.9-22.4 s | 252.0 s | 356.6 s / 27.5 s |
| Per-chunk extraction sum (5 chunks, 1445 words) | — | 9.8 s | 16.6 s | 15.7 s (warm 14.8 s) |
| Peak RSS | — | 959,266,816 B | 2,262,417,408 B | 3,221,438,464 B cold / 2,565,095,424 warm |
| Model cache | — | 290 MB | 746 MB | 1.1 GB |

Runtime/memory/counts: each run's `summary.json` + `manifest.json`
(`resources.peak_memory_bytes`, `elapsed_seconds`); per-chunk times in
`chunks/chunk-NNN.json`; warm rerun `gliner-runs/fastino_gliner2.5-multi-v1/jointie/20260928T121442Z-warm/`.
All runs: 5/5 chunks `ok`/feasible, 0 errors, 0 empty. Model revisions:
small `7132dc45…` (deberta-v3-xsmall), base `ca906247…` (deberta-v3-base),
multi `2ca71aaf…` (mdeberta-v3-base) — `manifest.json` `model`.

Takeaway: **small is the only run with both non-zero recovery and zero
misleading**; base buys +1 fact at 79.5% misleading and 2.36× memory; multi is
the best entity extractor (8/9 both-endpoint facts) with an inert relation head
(0 candidates). No run exceeds 2/9 supported-fact recall.

## 2. Claim-level: `daftprompt uses akar` (AGENTS.md line 7)

Baseline (cross-schema context): emitted `daftprompt (Tool) uses akar (Tool)`
quoting only the second sentence of line 7 — rejected twice over (subject kind
`Tool` vs. seed shape `uses(Command, Tool)`; partial quote fails whole-line
matching), so the true fact never entered the valid graph
(`budget-250-rescore.md` §"motivating failure").

- **small — recovered (C-001).** JointIE chunk 0 decoded exactly one relation:
  `uses` between exact line-7 slices `daftprompt` 134-144 (Application, entity
  conf 0.871) and `akar` 270-274 (Tool, conf 0.831), relation conf 0.358 (above
  the 0.05 candidate threshold). The validator accepts `uses(Application, Tool)`
  (Task 2's added pair — the exact repair the baseline needed); citation
  AGENTS.md:7-7 is snapshot-derived. Reviewed `supported`
  (`relations.json`, `scoring/review.json` C-001). Chunks 1-4 decoded zero
  relations (`chunks/chunk-NNN.json`).
- **base — recovered twice (C-001, C-002).** Same triple at two span
  configurations: C-002 both endpoints at the exact line-7 slices (conf 0.822,
  citation 7-7) and C-001 subject anchored at its L3 mention 64-74 (conf 0.839,
  citation 3-7). Both reviewed `supported` (`relations.json`,
  `scoring/review.json`). Claim-level cause of the duplication: the same entity
  appears twice in chunk 0's window (L3 + L7) and base's relation head bound both
  mentions to the L7 `akar` span; dedup keys are span-distinct so both survive.
- **multi — not recovered (entities only).** Both endpoints were emitted at the
  exact expected slices — `Application(daftprompt)` 134-144 (conf 1.0) and
  `Tool(akar)` 270-274 (conf 0.595) — but JointIE's multilingual relation head
  decoded **0 candidates in all 5 chunks** (`relations.json` `count: 0`), so the
  claim was never formed. Cause at claim level: pure relation-decoding silence —
  not an entity miss (both spans present), not a schema gap
  (`uses(Application, Tool)` exists), not chunking (both spans share chunk 0).
  R1 = `missing_relation` with both endpoints emitted
  (`scoring/scorecard.json` `reference_relations.facts` R1).

## 3. R1-R9 misses per model

Per-fact diagnoses below are the saved `scoring/scorecard.json`
`reference_relations.facts` records (span offsets included there). Schema gaps
are **0 in all three runs**: every fact's shape exists in
`schema/gliner25-schema.json` (incl. Task 2 additions `serves_as`, `indexes`,
`excludes`, `fuses`, `degrades_to`), so every miss is an extraction or
relation-decoding miss. Every fact lies wholly inside one chunk in every run —
**zero cross-chunk boundary losses** (chunking effects 0).

**small (1/9):** R2 `missing_relation` — `gitoxide` surfaced only as a
broken-boundary TechnologyPair span "git commit reader (gitoxide" (363-390),
and no relation was decoded in chunk 0 beyond the R1 pair. R3
`missing_relation` — components only (`SQLite FTS5`, `sqlite-vec` as
TechnologyPair), no `uses` pair. R4 `missing_relation` — object `canonical
correctness gate` emitted as ProcessRole (4504-4530) but subject `cargo check`
(4472-4483) typed ProcessRole, not Command, and no `serves_as` pair decoded.
R5-R9 `missing_entity` — subjects `code indexer`, `code indexer file
discovery`, `hybrid search`, object `XIAOMI_API_KEY`, and R9's
FailureMode/FallbackBehavior pair never emitted (5 of the 8 omissions).

**base (2/9, R1 + R8):** R8 recovered by C-074 `Command(graph build)
-requires-> Configuration(XIAOMI_API_KEY)` citing 45-45 (the `requires` shape
small never produced). R2 `missing_entity` — `gitoxide` never emitted (a
regression vs. small's span). R4 `missing_entity` — `the canonical correctness
gate` never emitted (regression vs. small). R3/R5/R6/R7/R9 `missing_relation` —
endpoints present, no candidate; chunk 3 (L125-140) decoded 0 relations
(`chunks/chunk-003.json`), so R5/R6/R9 (L127/L135) are in-chunk decoding
misses. Caveat recorded in the scorecard: several "present" subjects match only
via bare `search`-token containment; R5/R6's extension/path entities are genuine
L127 spans.

**multi (0/9):** R4 `missing_entity` — subject `cargo check --workspace` never
emitted (L81's object was emitted as ProcessRole). R1-R3/R5-R9
`missing_relation` with **both endpoints emitted in all eight** — the best
entity coverage of the three (8/9 vs base 7/9, small 4/9): `gitoxide` 382-390
clean Tool on L7, R3's exact `Subsystem(hybrid search indexer)` L7 span,
all five extensions + `node_modules/` PathSet on L127, `graph build`/`graph
search` + `XIAOMI_API_KEY` on L45, `substring search` on L135. All eight are
relation-decoding misses (`scoring/scorecard.json` facts; `entities.json`).

## 4. Assessment

**Did JointIE's typed decoding help?** Partly — conformance and determinism,
not recall or binding truth. Pro: every emitted relation was schema-pair
conformant (the pair validator rejections were 0 in all runs and the scorecards
record zero verdict disagreements with the schema artifact); the recovered
claims are exactly the intended shapes with snapshot-derived line citations;
typed same-chunk endpoints made chunk locality checkable. Con: structural
acceptance ≠ entailment — base passed 127/127 with 101 misleading
(`scoring/review.json`; the validator's rejection path never fired), and
candidate generation is checkpoint-dominated at identical thresholds
(1 / 127 / 0 candidates). The Cartesian typed-endpoint limitation forced the
`uses`/`uses__2` label split (recorded in `schema/gliner25-schema.json`). The
harness's `independent` variant (Task 3) — the built-in alternative — was never
run live, so the decoding-layer question is not yet closed.

**Does schema discovery need another pass?** No — schema gaps were 0 in all
three runs; no reference fact was blocked by a missing label. What the schema
needs is deterministic tightening, not discovery: 89 of base's 101 misleading
claims fall in classes enabled by broad `searches` argument descriptions — 44
wrong-action (GUI/debug/index/reindex/no-index invocations), 39
question/negation reads (L67-L70 example questions, L57 "not graph authority"
/ "Search citations"), 5 flag-scope violations, 1 inverted evidence (grouped
from `scoring/review.json` notes). A scoped description/argument-constraint
revision (mark example questions and negated sentences as non-claim text)
targets those classes. A new LLM discovery pass is not justified by these
measurements.

**Is any LLM step still needed?**
- *Entailment review: yes, non-negotiable.* Citation validity was 100%
  everywhere (baseline 40/40, small 1/1, base 127/127) while misleading ranged
  0-79.5%. Wrong-action, wrong-binding, scope, and negation classes are
  invisible to deterministic grounding; base's 101 citation-valid misleading
  claims are the proof (and the baseline's L133 `owns` claims show the LLM
  extractor has the same failure). Any pipeline needs an LLM-or-human review
  gate before claims enter a graph; Q5/Q6 correct-absence also needs review
  confirmation.
- *Relation extraction: still needed for broad recall.* Untuned GLiNER2.5
  relation heads max out at 2/9 supported-fact recovery here (base) with a
  79.5% misleading rate; small is precise but recalls one fact. No GLiNER-only
  path meets the epic's recovery goals. Note the local model does beat the
  baseline extractor on both axes (small: 1/9 and 0% misleading vs. 0/9 and
  20%) — just at near-zero recall. Fine-tuning remains open (epic non-goal;
  examples and errors preserved in the run artifacts).
- *Schema discovery (Epic 013's LLM pass): droppable* on this evidence.

## 5. Recommendation — one next step

**Revise the decoding step and rerun once (option: revise schema/model/chunking
and rerun).** One controlled experiment, one variable changed:

- Hold the frozen contract byte-identical (schema, snapshot, reference set,
  scorer, chunking). The measurements rule out the other levers: schema gaps 0,
  chunking effects 0 (every fact is single-chunk), thresholds non-binding
  (base's misleading confidence floor 0.316 ≫ 0.05; multi's decode is empty at
  any threshold).
- Change only the decoding layer: rerun **`fastino/gliner2.5-small-v1`** with
  the harness's `independent` relation variant (`python -m harness extract
  --model fastino/gliner2.5-small-v1 --variant independent`) on the same frozen
  inputs, scored with the same scorer.
- **Smallest model meeting the measured quality bar: `fastino/gliner2.5-small-v1`
  (small).** The bar (beat the LLM baseline on R-recovery and misleading rate,
  hold 100% citation validity and Q5/Q6 correct absence, at acceptable local
  cost) is met by small only: 1/9 vs. 0/9 recovery, 0% vs. 20% misleading,
  1/1 citation-valid, 959 MB peak RSS / 290 MB cache / ~9.8 s extraction —
  the smallest and cheapest of the three. base fails the misleading bar
  (79.5%, 2.36× memory); multi fails recovery and the line-7 case (0/9, inert
  relation head, 3.36× memory). Caveat named explicitly: no model meets the
  epic's absolute recovery goals (ceiling 2/9), so none is production-ready and
  "meeting the bar" means beating the baseline, not solving the task.
- **Stop condition:** if the independent variant does not lift supported-fact
  recall past 2/9 on the frozen reference set (or returns another misleading
  mass), stop the GLiNER-only approach — the remaining lever would be
  fine-tuning, a separate decision.
- Whatever the outcome, an experimental backend retains an LLM/human
  entailment-review gate (base's 101 citation-valid misleading claims prove it
  cannot be omitted). Integration into the production `graph` CLI is a separate
  epic and is not recommended here.

## 6. Artifact index

| Artifact | Contains |
|---|---|
| `frozen-inputs/llm-baseline/budget-250-rescore.{json,md}` | Baseline: 40 claims, 28/4/8, R 0/9, Q coverage, motivating rejection |
| `frozen-inputs/comparison-plan.md` | Baseline identity, stale-column warning, schema differences, frozen hashes |
| `gliner-runs/fastino_gliner2.5-small-v1/jointie/20260928T112911Z/` | small run: `manifest.json`, `summary.json`, `entities.json`, `relations.json`, `chunks/` |
| `…/20260928T112911Z/scoring/{scorecard.json,scorecard.md,review.json}` | small claims, R1-R9 facts, Q coverage, support review |
| `gliner-runs/fastino_gliner2.5-base-v1/jointie/20260928T113433Z/` (+ `scoring/`) | base run + 127-claim review (misleading classes) |
| `gliner-runs/fastino_gliner2.5-multi-v1/jointie/20260928T120352Z/` (+ `scoring/`) | multi run + empty claims list, 8/9 endpoint coverage |
| `gliner-runs/fastino_gliner2.5-multi-v1/jointie/20260928T121442Z-warm/` | multi warm-cache rerun (byte-identical extraction) |
| `schema/gliner25-schema.json` | Schema artifact (hash `366fa7e5…`, pairs, `uses__2` mapping, provenance) |
