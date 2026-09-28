# Epic 014 Comparison Plan (Task 1)

Written before any GLiNER2.5 model output exists. All hashes below were verified at
freeze time (see `MANIFEST.json` and `check-inputs-run-2026-09-28.log`).

## 1. Which LLM run is the baseline

**The baseline is the actual `experiments/open-vocab-graph/output/runs/budget-250/`
folder**, re-scored in this task under its own seed: `llm-baseline/budget-250-rescore.json`
(human-readable scorecard: `budget-250-rescore.md`).

- Run identity: `open-vocab-graph extract --repo . --source AGENTS.md --word-budget 250`,
  model `mimo-v2.5-pro`, 8 serial block calls, run on 2026-09-28 (Epic 013 Task 3/4 harness).
- Recorded hashes in its `run-manifest.json`: seed xxh3 `c776d7e579d8aed5`,
  reference-set xxh3 `7dad11a105072baf`, AGENTS.md snapshot xxh3 `29d01e36b77b6d33`
  (sha256 `a15dbdb0…`). All three equal the frozen inputs in this directory, so the
  baseline and every GLiNER2.5 run share **the same snapshot, reference set, and seed**.
- Contents: 64 relations emitted, **40 valid claims**, 24 invalid claims (all reasons
  recorded), 0 model errors, 0 parse errors, 0 rejected mints.

### Stale-column warning (do not skip this)

Epic 013's report table column “budget 250” (31 valid claims, 2/9 relations recovered)
does **not** describe this folder. That column describes a 31-claim run under the other
seed (xxh3 `9bd9ffae74304ce2`, the seed shared by the 150/400/600 runs). The folder
actually on disk used seed `c776d7e579d8aed5` and produced 40 valid claims. **No Epic 014
result may be compared with the stale 31-claim column.** The 150/400/600 runs may be
cited only as labeled, different-seed context (their seed differs in entity kinds and
predicate shapes: kinds CLI/Database/Model/SearchIndex/DataSource, `uses`=CLI→Database,
`degrades_to`(SearchIndex→SearchIndex) — versus the frozen seed's
Command/Configuration/Tool/DataSource, `uses`=Command→Tool).

### Why re-scoring rather than a new LLM baseline run

The Epic 013 budget-250 folder is the only complete, hash-verified LLM run under the
frozen seed, and it contains the motivating Epic 014 failure (`daftprompt uses akar`
emitted at line 7 and rejected). Re-scoring it is deterministic, requires no API key or
new model call, and keeps the baseline fixed while the three GLiNER2.5 checkpoints are
run later. A fresh LLM run would add sampler non-determinism and a second
non-reproducible artifact without changing the comparison's meaning.

## 2. Is the baseline schema identical to the GLiNER2.5 schema?

**No — the schemas are NOT identical, and every comparison must be labeled as
cross-schema.** Mapping happens through the frozen reference set (R1-R9, Q1-Q6) and the
per-fact test inputs, never through raw predicate-name equality.

| Aspect | Baseline (Epic 013 open-vocab claim schema) | GLiNER2.5 schema (Task 2, to be built from the frozen seed) |
|---|---|---|
| Entity kinds | Open-vocabulary strings; budget-250 final vocabulary: seed `Command`, `Configuration`, `Tool`, `DataSource` + minted `File`, `Directory` | Versioned schema with descriptions; seed-derived kinds **plus additions**, including a new `Application` kind |
| `uses` shape | Single pair `uses(Command, Tool)` — this is why the true fact was rejected | Multiple **allowed endpoint pairs**, at minimum `uses(Application, Tool)` and `uses(Command, Tool)`; validator must reject disallowed pairs |
| Predicates | Each predicate carries exactly one `(subject_kind, object_kind)` shape at a time; new shapes force mid-run minting (budget-250 minted 5) | Predicates declare multiple allowed endpoint pairs; pair variants translated into model labels as needed, then enforced at validation |
| Extraction unit | Whole-relation JSON per block, subject/object are named entity strings | Mention spans + types from GLiNER2.5; graph identity resolved separately from mention text (Epic 014 constraint) |
| Evidence | Model-quoted line ranges + text, strict whole-line match against snapshot | Deterministic: character offsets mapped onto the frozen snapshot; the model can never supply citation text |
| Examples in seed | Model could see seed `example_relations` in every prompt | Evaluation/schema-sanity material only; not assumed to be few-shot inference examples |

Concretely, three differences will dominate the comparison and must be reported per
claim: (1) endpoint-kind assignment (`daftprompt` was `Tool` in every baseline emission;
it must be `Application` in the GLiNER runs), (2) evidence derivation (model-quoted vs
snapshot-mapped), (3) schema-gap routing (facts whose kind/predicate the frozen seed
lacks are scored as *schema gaps*, separately from model extraction misses — the frozen
seed has no shape for R4's process role, R7's fusion, or R9's failure-mode subject).

## 3. Frozen comparison constants

| Constant | Value | File |
|---|---|---|
| Snapshot | sha256 `a15dbdb069bc…`, xxh3 `29d01e36b77b6d33`, 146 lines | `AGENTS.md` |
| Reference set | sha256 `be87f537…`, xxh3 `7dad11a105072baf` (R1-R9, 8 repeated entities, Q1-Q6) | `reference-set.json` |
| Seed | sha256 `4be96db2…`, xxh3 `c776d7e579d8aed5` | `seed-vocabulary.json` |
| Scoring definitions | Epic 013 Task 4 metric definitions (valid / supported / weak / misleading; R1-R9 recovery requires a valid **and supported** claim; question coverage from evidence) | `epics/013-…md` §Report |
| Chunking policy | Whole physical lines, paragraph-preferred packing; baseline used word budget 250 (8 blocks). GLiNER runs record their own chunk boundaries and overlaps (Task 3) and must distinguish cross-chunk misses from extraction misses | — |

## 4. Baseline scorecard summary (full detail in `budget-250-rescore.md`)

- Support: 28/40 clean (70.0%), 4/40 weak (10.0%), **8/40 misleading (20.0%)**; citation
  validity 40/40 (strict whole-line match, re-verified mechanically in the re-score).
- R1-R9 recovery: **0/9**; Q coverage: Q2 covered-with-defect, Q3 partial-with-defect,
  Q1/Q4 not covered, Q5/Q6 correct absence.
- The 8 misleading claims split into: 4 index-commands mislabeled as `searches`, 1 wrong
  directory object (`schema.sql`), 2 `owns` claims whose cited line (L133, comments
  policy) does not state the relation, 1 gate mislabeled as `searches` (L81).
- The headline failure is visible in the folder itself: `daftprompt (Tool) uses akar
  (Tool)` citing a partial line-7 quote — rejected twice over (kind + evidence), so the
  true fact never entered the valid graph.
