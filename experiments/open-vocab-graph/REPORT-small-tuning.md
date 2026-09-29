# Epic 015 Report — GLiNER2.5-small recall tuning (Task 5)

Comparison of every lever tried in Epic 015 (threshold/top-k sweep, optimizer,
`independent` variant, schema change) against Epic 014's canonical `small` run
as the fixed baseline. Every metric below cites a saved run artifact; no model
was run for this report.

## 0. Comparison labels (read before any table)

**Fixed baseline (like-for-like, schema `366fa7e5…`):** Epic 014's canonical
`small` run `gliner-runs/fastino_gliner2.5-small-v1/jointie/20260928T112911Z/`
— 1/9 recovery (R1), 1 relation candidate emitted/accepted, 1/0/0
supported/weak/misleading, 0% misleading, 1/1 citation-valid, 959,266,816 B
peak RSS, 112.7 s wall (includes cold-cache checkpoint download), 9.8 s
per-chunk extraction sum (5 chunks). Verified against the run's
`manifest.json` (schema `366fa7e5…`, seed `c776d7e579d8aed5`),
`summary.json`, `scoring/scorecard.json`, `scoring/review.json`. Same frozen
snapshot `a15dbdb0…`, reference set, chunking plan, and scoring contract as
every run in this epic (see each run manifest's `schema`/`snapshot` fields).

**Like-for-like (model-only deltas, schema `366fa7e5…`):** the four sweep runs,
greedy, the warm beam counterpart, and the `independent` run all executed on
the OLD schema `366fa7e5…` — deltas vs. the baseline are model/decoding-only.

**Cross-schema (labeled, never model-only deltas):** the Task 4 schema run
`20260928T160412Z-task4-schema-supersedes-366fa7e5` executed on the NEW schema
`9e687ed88f84a0fcb26d92596f8c3cbe31b85208e7f575e06b021cf96fa7cd11`
(per-relation thresholds 0.25 on the five zero-emission predicates + revised
descriptions; supersedes `366fa7e5…` per its manifest and the artifact's
`revision` block). Every comparison involving that run is cross-schema and is
labeled as such, mirroring Epic 014 Task 8's discipline.

**Wall-time comparability:** the canonical 112.7 s includes cold-cache
checkpoint download; all other runs are warm-cache and mutually comparable
(8.6-14.1 s). Per-chunk extraction sums (5 chunks) are the model-only cost
figure: 9.8 s canonical, 9.0-9.6 s warm jointie runs, 4.5 s independent
(each run's `chunks/chunk-NNN.json` `elapsed_seconds`).

**Peak-RSS convention:** MB = bytes / 10^6 of each manifest's
`resources.peak_memory_bytes` (matches Epic 014's "959 MB"). Epic 015's Task
3/4 tables labeled the same bytes in MiB ("1154 MB" = 1,210,040,320 B,
"1285 MB" = 1,347,223,552 B) — unit label only, same artifacts. Run-to-run
allocator variance dominates all RSS differences (Task 1 saw 1038-1349 MB
across output-identical runs).

## 1. Lever-by-lever results (all metrics vs. the fixed baseline)

Artifacts: recovery from each run's `scoring/scorecard.json`
(`reference_relations.facts`); support split from `scoring/review.json`;
counts from `relations.json`/`entities.json`; time/RSS from `manifest.json`.
Run dirs share `gliner-runs/fastino_gliner2.5-small-v1/`.

| Lever / Config | Schema | R1-R9 recovery | Candidates em. / acc. | Sup. / weak / misl. | Misleading rate | Wall time | Peak RSS | Run id (suffix) |
|---|---|---|---|---|---|---|---|---|
| baseline: canonical small (Epic 014) | `366fa7e5…` | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 112.7 s ¹ | 959 MB | `jointie/20260928T112911Z` |
| sweep-1 (0.02/0.02, 64/24) | `366fa7e5…` | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 13.6 s | 1134 MB | `jointie/20260928T125435Z-sweep-1` |
| sweep-2 (0.01/0.01, 64/24) | `366fa7e5…` | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 13.5 s | 1038 MB | `jointie/20260928T125506Z-sweep-2` |
| sweep-3 (0.005/0.005, 128/48) | `366fa7e5…` | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 13.5 s | 1339 MB | `jointie/20260928T125527Z-sweep-3` |
| sweep-4 (0.001/0.001, 256/64) | `366fa7e5…` | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 11.9 s | 1349 MB | `jointie/20260928T125549Z-sweep-4` |
| greedy optimizer | `366fa7e5…` | 1/9 (R1) ² | 2 / 2 ² | 2 / 0 / 0 ² | 0% | 13.3 s | 1128 MB | `jointie/20260928T130455Z-greedy` |
| beam (warm, baseline stand-in) | `366fa7e5…` | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 13.2 s | 1167 MB | `jointie/20260928T130515Z-beam` |
| independent variant (thr 0.5) | `366fa7e5…` | **0/9** | 0 / 0 | 0 / 0 / 0 | vacuous (no claims) | 8.6 s | 1210 MB | `independent/20260928T132158Z` |
| task4 schema (0.25 + revised desc.) — **cross-schema** | `9e687ed8…` (NEW) | 1/9 (R1) | 1 / 1 | 1 / 0 / 0 | 0% | 14.1 s | 1347 MB | `jointie/20260928T160412Z-task4-schema-supersedes-366fa7e5` |

¹ Includes cold-cache checkpoint download; only wall-time caveat.
² Greedy's second candidate is a span-duplicate of the same R1 fact
(`uses(Application:daftprompt, Tool:akar)` emitted once per `daftprompt`
mention span), scored as 1 duplicate group (C-001/C-002), not an additional
recovery — recall stays 1/9. Both claims review `supported`
(`…-greedy/scoring/review.json`, `…-greedy/relations.json`). Entity output is
unchanged (40 mentions in all jointie runs).

Verified against run manifests: thresholds/optimizer per-row as listed
(`manifest.json` `thresholds`); sweep and task4 schema hashes as labeled; the
warm beam run is extraction-JSON-equal to the canonical run (entities,
relations, all chunk records ignoring `elapsed_seconds`), so it is an exact
outcome stand-in for the fixed baseline at warm-cache cost.

## 2. Per-lever findings (short)

- **Threshold/top-k sweep (Task 1):** all four configurations produced output
  **JSON-equal to the canonical run** (40 entity mentions, exactly 1 relation
  candidate C-001 R1, 0 rejected, 0 misleading) — lowering
  `candidate_threshold`/`relation_role_threshold` 50× surfaces zero
  additional candidates, and the top-k caps never engage at 32/12. These four
  knobs **do not bind** for `small` on this snapshot. (Artifacts: each
  `…-sweep-N/{entities,relations}.json` + `chunks/`, epic 015 Task 1 Findings.)
- **Optimizer (Task 2):** greedy changes output in exactly one way — decodes
  R1 twice (once per mention span) where beam decodes it once. Recall and
  misleading rate unchanged (1/9, 0%), no speed difference (13.3 vs 13.2 s
  warm; chunk sums 9.0 vs 9.1 s), RSS indistinguishable. Noise, not coverage:
  **beam stays the default.** (Artifacts: `…-greedy/`, `…-beam/` + their
  `scoring/` dirs.)
- **`independent` variant (Task 3):** found the entities (32 dedup mentions,
  same 6/8 repeated-entity recall as jointie) but decoded **zero relations in
  every chunk** for all nine labels. R1-R4 diagnose `missing_relation`
  (including R1 — both endpoints were emitted), R5-R9 `missing_entity`.
  Recall **0/9 vs. baseline 1/9**; its 0% misleading is vacuous (no claims
  exist); citation validity untestable (0 claims) vs. 1/1. **Stop condition
  (lift supported-fact recall past 2/9): NOT MET** — 0/9 never approached the
  bar, so per the epic protocol no `independent` threshold sweep was run and
  `independent`-variant threshold tuning is stated here plainly: it does not
  move the ceiling. (Caveat kept on record: gliner2 gates head/tail spans at
  the extraction threshold, so a lower threshold could in principle surface
  candidates — foreclosed by the stop condition, not tested.)
  (Artifacts: `…/independent/20260928T132158Z/` incl. `manifest.json`
  (`thresholds: {threshold: 0.5}`), `relations.json` `count: 0`,
  `scoring/scorecard.json` facts R1-R9.)
- **Schema change (Task 4, cross-schema labeled):** on the NEW schema
  `9e687ed8…`, **none of the five previously-silent predicates**
  (`serves_as`, `indexes`, `excludes`, `fuses`, `degrades_to`) **were emitted
  at all** — zero candidates; the only relation decoded anywhere is `uses`
  (chunk 0). Output is JSON-equal to the canonical run (cross-schema):
  recall stays 1/9, 1/0/0, 0% misleading. Per-relation threshold 0.25 and
  snapshot-echoing description rewrites do not move `small`'s decoder.
  (Artifacts: `…-task4-schema-supersedes-366fa7e5/` incl. `manifest.json`
  (schema `9e687ed8…`), `relations.json`, `scoring/`.)

## 3. Best configuration and the explicit trade-off

**No configuration tried in this epic improved on the baseline.** The single
best configuration found is therefore the baseline configuration itself:
`fastino/gliner2.5-small-v1`, `jointie`, untuned JointIEConfig defaults
(candidate/role thresholds 0.05/0.05, top-k 32/12, `optimizer="beam"`), on
schema `366fa7e5…` — represented at warm-cache cost by
`jointie/20260928T130515Z-beam` (JSON-equal to the canonical run).

Trade-off stated on all three axes, not recall alone:

- **Recall:** baseline 1/9 (R1) is the maximum achieved by any configuration.
  Sweeps and the task4 schema are output-identical (1/9); greedy adds only a
  span-duplicate of R1 (still 1/9 facts); `independent` sinks to 0/9.
- **Misleading rate:** 0% for every configuration that emitted claims — but
  only the baseline-family runs earn that with a real supported claim; the
  sweeps/task4 add nothing to trade against it, greedy's extra claim is a
  duplicate of the same supported fact (dilutes nothing but adds graph noise),
  and `independent`'s 0% is vacuous (zero claims). No lever reproduced
  `base`'s misleading-claim problem — but none bought recall either.
- **Cost:** all warm jointie runs are equivalent (~12-14 s wall, ~9-10 s
  extraction, 1.0-1.35 GB peak RSS, differences within allocator variance);
  `independent` is slightly cheaper (8.6 s / 4.5 s extraction) but emits
  nothing, so cost is moot. Greedy buys no speed (13.3 vs 13.2 s). The
  widest sweeps trend higher RSS (1339-1349 MB) for zero output change.

Net: every lever traded at best noise (greedy's duplicate, sweep/task4's
identical output at identical cost) and at worst recall (independent). There
is no Pareto improvement over the baseline anywhere in this epic.

## 4. Recommendation

**(c) Conclude that decoding/schema tuning has a ceiling here and a different
approach is needed next.** Do **not** adopt any of this epic's configurations
as a new `small` baseline — keep the canonical configuration (Defaults,
`beam`, schema `366fa7e5…`) as the `small` baseline.

Reasoning: four decoding-level and schema-level levers — thresholds and top-k
(50× lowering, 8× cap-raising; JSON-equal outputs), optimizer (beam vs.
greedy; span-duplicate only), extraction variant (`independent`; 0/9, stop
condition not met at the 2/9 bar), and schema-level per-relation thresholds +
grounded description rewrites (cross-schema JSON-equal outputs, five silent
predicates still silent) — all either change nothing or strictly reduce
recall. `small`'s failure is upstream of everything this epic varied: the
relation head of this checkpoint family does not decode the missing facts on
this snapshot, and no decoding or schema knob surfaces them. Task 3's stop
condition was not met (0/9, never near 2/9), which the epic protocol directs
us to state plainly rather than sweep `independent`'s threshold indefinitely.

What remains open, in order of specificity (all from Epic 014's conclusions /
this epic's Goals):

1. **Fine-tuning `small` on relation examples** — the one lever neither epic
   tested (Epic 015 non-goal; examples and errors are preserved in the run
   artifacts). This directly targets the relation-decoding wall that survived
   every configuration change here.
2. **A different model family or an LLM relation extractor** with an
   entailment gate — if fine-tuning is out of budget. Untuned GLiNER2.5
   decoding maxes out at 1/9 here (small) / 2/9 at 79.5% misleading (base,
   Epic 014), i.e. no GLiNER-only configuration meets the recovery goals.
3. **The still-open "LLM-or-human entailment review" gate** — needed in any
   pipeline regardless of extraction choice (base's 101 citation-valid
   misleading claims, Epic 014, prove deterministic grounding cannot replace
   it), but it is a precision gate, not a recall lever; it does not substitute
   for (1) or (2).

Continuing tuning (option b) is explicitly not recommended: the only untried
decoding knob is `independent`'s threshold, and the stop condition deliberately
foreclosed it on evidence (0/9 at default, spans gated at threshold) that it
would not lift recall past 2/9. Integration into the production `graph` CLI
remains a separate epic.

## 5. Artifact index

| Artifact | Contains |
|---|---|
| `gliner-runs/fastino_gliner2.5-small-v1/jointie/20260928T112911Z/` | Fixed baseline (Epic 014 canonical): manifest, summary, entities, relations, chunks, `scoring/` |
| `…/jointie/20260928T125435Z-sweep-1/` … `20260928T125549Z-sweep-4/` | Task 1 threshold/top-k sweeps (4 dirs; manifests record varied thresholds; output JSON-equal to baseline) |
| `…/jointie/20260928T130455Z-greedy/` (+ `scoring/`) | Task 2 greedy run: 2 claims (R1 span-duplicate group), both supported |
| `…/jointie/20260928T130515Z-beam/` (+ `scoring/`) | Task 2 warm beam counterpart = warm-cache baseline stand-in (JSON-equal to canonical) |
| `…/independent/20260928T132158Z/` (+ `scoring/`) | Task 3 independent run: 0 relations, 0/9 recovery, stop condition not met |
| `…/jointie/20260928T160412Z-task4-schema-supersedes-366fa7e5/` (+ `scoring/`) | Task 4 run on NEW schema `9e687ed8…` (cross-schema vs. all rows above) |
| `schema/gliner25-schema.json` + `schema/build.py` | Current artifact `9e687ed8…` with `revision.supersedes_artifact_sha256: 366fa7e5…` |
| `epics/015-gliner25-small-recall-tuning-experiment.md` | Per-task runs, findings, and acceptance evidence (Tasks 1-4 tables are the source of the comparison rows above) |
| `REPORT-gliner25.md` | Epic 014 baseline report (not edited); canonical small numbers |
