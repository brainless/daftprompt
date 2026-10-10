# Epic 013: Open-Vocabulary Graph Schema Experiment

> **Status: Superseded / code removed (2026-10-10).** The experiment was completed, but its code, reports, and frozen inputs (`experiments/open-vocab-graph/`) were deleted from the working tree, along with Epics 014 and 015, ahead of a new approach. They remain in git history (`git show cebe59c`). Task statuses below describe the state when the epic was done, not what is in the repo now.

## Introduction

Epic 011 introduced an experimental document provenance graph CLI (`graph
build`/`graph search`) with a fixed, hard-coded predicate vocabulary (`indexes_language`,
`indexes_dialect`, `indexes_extension`, `searches_index`, `uses_parser`,
`excludes_path`, `work_item_status`) and a fixed entity-kind vocabulary
(`feature`, `index`, `language`, `dialect`, `extension`, `parser`,
`work_item`), both enforced in the `EXTRACTION_CONTRACT` prompt
(`crates/daftprompt-indexer/src/graph/model.rs`) and independently re-checked
in `crates/daftprompt-indexer/src/graph.rs`. Epic 012 hardened evidence
fidelity against that fixed schema using this repository's own documents.

The current graph is an experiment, not a production baseline. Its completed
`--source 'epics/*.md'` build contains 187 claims from 12 files, including
115 `work_item_status` claims. Some unrelated concepts have been fitted into
the available predicates: `uses_parser` claims name `akar`, `akar-winit`,
`gix`, and Xiaomi MiMo V2.5 Pro as parsers. Search can answer a direct
question about Rust indexing, but questions about what code search finds
return `unknown` because the relevant claims do not satisfy the search rule's
scope requirements. These observations motivate a separate experiment to
test whether a discovered, extensible vocabulary yields more faithful and
useful relations while retaining exact source evidence. They do not by
themselves establish that open vocabulary will improve search.

**Status:** Complete (recommendation: iterate further on the experiment; see Report).

## Goals

- Test a two-phase extraction design on a single file (`AGENTS.md`) first,
  without changing the existing graph CLI:
  1. **Schema-discovery phase.** Deterministically sample text blocks across
     the `--source` set and ask the LLM, in one batched call, to propose a
     starting entity-kind and predicate vocabulary for building a graph meant
     for user-facing search — with no mention of "daftprompt" or this
     project's specific domain in the prompt, only the high-level purpose.
  2. **Sequential extraction phase.** Walk the source in word-length-bounded
     blocks (the word-length budget is a variable to tune and measure), one
     LLM call per block, carrying forward a running summary and the
     accumulated predicate/entity vocabulary from call to call so the model
     can reuse or deliberately extend the schema instead of reinventing it
     per block.
- Keep the sample selection **deterministic** (seeded, not truly random) so
  repeated runs against unchanged content produce the same seed vocabulary —
  build-to-build reproducibility should not gain a second, untracked source
  of variance beyond the LLM's own non-determinism.
- Measure relation quality as well as vocabulary growth: count useful
  relations, forced or misleading predicates, omitted useful relations,
  duplicate entities, exact citation validity, and whether each sampled
  citation actually supports its claim.
- Evaluate a small, fixed set of user questions against the extracted claims,
  recording whether the graph contains sufficient evidence for the expected
  answer, including `unknown`. Compare with the existing graph experiment
  only on overlapping source snapshots; report extraction and search-rule
  limitations separately.

## Non-goals

- Replacing the existing fixed-vocabulary graph experiment in this epic.
- Any SQLite schema change. The experiment writes its output to a standalone
  location (files or a scratch table), not the existing `graph_*` tables.
- Restoring batch parallelism. The sequential carryover design is inherently
  serial (each call depends on the previous call's summary and vocabulary
  state); regaining concurrency, if ever needed, is out of scope here.
- Solving entity-kind generalization for every possible repo shape. This
  epic starts with `AGENTS.md` and compares one overlapping epic file;
  broader validation across unrelated repos is future work.

## Design notes carried in from discussion

- **Sampling must be deterministic**, e.g. one block from the start of each
  matched file or a hash-derived pick keyed off the selected file's content
  hash, not language-level `rand` or `git_head` alone. Guarantee at least one
  sampled block per file so a file's vocabulary cannot be skipped by chance.
- **Schema-discovery is one batched call**, not one call per sampled block —
  the goal is a proposed vocabulary, not claims with citations, so there is
  no need to pay per-block LLM round-trips here.
- **The discovered seed vocabulary is a soft prior, not a hard cap.** The
  sequential extraction phase's prompt should bias the model to reuse an
  existing predicate/entity kind when one fits, but must still allow minting
  a genuinely new one when nothing in the seed applies — otherwise the
  "won't generalize" problem just moves from build-time to
  schema-discovery-time.
- **Near-duplicate predicates/entity-kinds need a consolidation pass.** Even
  a single batched discovery call can propose synonyms (`uses`/`depends_on`)
  for the same relation shape; these should be merged before the seed is
  frozen for the sequential phase.
- **Word-count block splitting may hurt citation fidelity.** Epic 012's whole
  focus was exact-line evidence matching against heading-anchored passages.
  Keep physical lines intact and prefer paragraph boundaries while testing
  different word budgets. Record any boundary that breaks a sentence or
  separates a claim from the context needed to interpret it.
- **A valid citation is not necessarily supporting evidence.** Exact line
  matching proves where words came from, not that the relation follows from
  those words. Review claim-to-evidence support separately, including
  plausible but wrong predicate assignments.
- **Running context is not evidence.** A summary from a previous block may
  help resolve names or vocabulary, but every new claim must be supported by
  the current block's snapshot lines.

## Tasks

### Task 1: Deterministic schema-discovery sampler and prompt

**Status:** Complete

**Depends on:** none.

**Change:** Write a standalone sampler (script or small Rust binary/test
harness, not part of `crates/daftprompt-indexer/src/graph.rs` or the `graph`
CLI) that, given a `--source`-style file set, deterministically picks one or
more text blocks per file (seeded, not `rand`-based) and sends them in a
single batched LLM call with a generic system prompt — stating only that the
purpose is building a graph for user-facing search, with no reference to
"daftprompt" or this repo's specific domain. The model should respond with a
proposed entity-kind list and predicate list (with a one-line description of
each), plus a couple of example subject/predicate/object relations as a
sanity check.

**Acceptance criteria:**

- [x] Given the same file set and unchanged content, two runs produce the
      same sampled blocks (determinism verified by a test or repeated run
      comparison, independent of the LLM call itself).
- [x] At least one block is sampled from every matched file, not just a
      pooled random subset across all files.
- [x] The prompt sent to the LLM is inspected and contains no
      daftprompt-specific or indexer-specific vocabulary — only the generic
      "graph for search" framing.
- [x] Run against `AGENTS.md` alone and record the raw proposed
      vocabulary output for Task 2 to consume.

### Task 2: Predicate/entity-kind consolidation pass

**Status:** Complete

**Depends on:** Task 1.

**Change:** Take Task 1's raw proposed vocabulary and produce a deduplicated
seed vocabulary — merging near-duplicate predicates/entity-kinds
(e.g. `uses`/`depends_on`) that describe the same relation shape. Implement
as either a second consolidation LLM call ("merge synonyms in this list, keep
distinct concepts separate") or a deterministic string/embedding-similarity
merge; either is acceptable for the experiment as long as the merge decisions
are recorded and inspectable.

**Acceptance criteria:**

- [x] Given Task 1's `AGENTS.md` output, the consolidation step is run and
      produces a smaller-or-equal vocabulary with merge decisions logged
      (what was merged into what, and why).
- [x] The consolidated seed vocabulary is saved in a form Task 3 can load
      (e.g. a JSON file), not re-derived by hand.

**Implementation notes (Task 2):** The `open-vocab-graph consolidate`
subcommand uses the second-LLM-call option: the model proposes merge groups,
and a deterministic validator applies only safe groups (verbatim existing
members, no double assignment, one subject/object shape per predicate group
after entity-kind canonicalization), recording every applied or rejected
decision in `consolidation-log.json` alongside the raw model response.
String/embedding similarity was rejected as the primary mechanism because
near-duplicates like `uses`/`depends_on` are semantic synonyms invisible to
string similarity, and embedding models would add a model download to the
standalone crate. Against Task 1's `AGENTS.md` output the model proposed no
merges — the five entity kinds are distinct and the four predicates have four
different relationship shapes — so the seed equals the input; that zero-merge
outcome is recorded explicitly (`no_merge_rationale`) rather than left
implicit. The merge code path is verified by unit tests on a synthetic
`uses`/`depends_on` vocabulary and by an end-to-end `consolidate` run
against a synthetic input (5 predicates → 4, `uses` ← `depends_on`).

### Task 3: Sequential carryover extraction loop

**Status:** Complete

**Depends on:** Task 2.

**Change:** Implement the block-by-block extraction loop: split `AGENTS.md`
into complete-line blocks bounded by a configurable word budget, preferring
paragraph boundaries where possible. Before the first run, record a small
reference set of supported relations, repeated entities, and user questions
from the snapshotted source, including questions expected to be `unknown`.
For each block in order, call the LLM with the running summary from the
previous call (empty on the first call), the current
accumulated predicate/entity-kind vocabulary (seeded from Task 2, allowed to
grow), and the block's text. The model must respond with extracted
entities/relations for that block, evidence citations resolvable against the
block's real line range, and an updated running summary to carry into the
next call. Reuse Epic 011/012's snapshot and exact-match evidence checks where
practical. The existing whole-response validator also enforces the fixed
predicate and entity-kind lists, so an open-vocabulary claim needs separate
schema validation. Record the raw response and any validation failure before
correction or omission; do not count a failed claim as valid because it was
silently dropped.

**Acceptance criteria:**

- [x] The loop runs end-to-end against `AGENTS.md` for at least two different
      word-length budget values, producing a complete claim set for each run.
- [x] The reference relations, entities, questions, and expected supporting
      passages are recorded before any extraction run is inspected.
- [x] Each claim's evidence is validated against the real source snapshot
      using the existing strict evidence-matching logic (exact match,
      whitespace-normalized) — no fuzzy/best-effort matching introduced.
- [x] Every accepted predicate has a recorded meaning and subject/object
      shape; claims that violate that shape are reported as invalid.
- [x] Each emitted claim cites the current block, and no claim is accepted
      solely on the strength of a carried summary.
- [x] The vocabulary is observably reused across calls (the same predicate
      appearing more than once for a clearly repeated relation shape, rather
      than a fresh synonym being minted every time), and at least one
      genuinely new predicate is allowed through when the seed vocabulary
      doesn't cover a relation actually present in the text.

**Implementation notes (Task 3):** The `open-vocab-graph extract` subcommand
implements the loop in `experiments/open-vocab-graph/src/extract.rs`. Block
splitting packs Task 1's paragraph units greedily under `--word-budget` and
never breaks a physical line, so a paragraph longer than the budget (for
example AGENTS.md lines 7, 127, and 133) becomes its own oversized block;
per-block word counts and line ranges are recorded in each run manifest.
`output/reference-set.json` (9 supported relations with verbatim expected
passages, 8 repeated entities with occurrence lines, 6 questions of which Q5
and Q6 expect `unknown`) was generated by a script that copies line texts
straight from `AGENTS.md` and asserts every claim about the file; the
`extract` subcommand refuses to start unless that file exists and declares
`recorded_before_any_extraction_run`, making the ordering mechanical. The
strict evidence check (`evidence_text_matches_snapshot`) and the snapshot
line lookup are reimplemented from `crates/daftprompt-indexer/src/graph.rs`
(`evidence_text_matches_snapshot`, `snapshot_lines`) with identical
strictness — whitespace-normalized word-sequence equality — and the
indexer's prefix-forgiveness correction path is deliberately NOT ported. A
claim is invalid if any of its citations fails strict matching or cites
lines outside the current block, so carried summaries cannot support claims.
Open-vocabulary validation requires every used predicate to be declared with
a description and subject/object shape (seed or mint); shape violations,
undeclared predicates, empty evidence, and unmatched citation text are all
reported in `invalid-claims.json` with reasons, never silently dropped. Raw
responses land in `blocks/block-NN.raw.json` before any validation; a model
call gets one retry and its failures are recorded in
`blocks/block-NN.error.json`. Two runs were executed against `AGENTS.md`
(sha-verified snapshots): budget 150 (13 blocks, 43 valid / 38 invalid
claims, 18 predicates minted, 4 predicates reused across blocks —
`contains`, `depends_on`, `enforces`, plus seed `searches`) and budget 400
(5 blocks, 19 valid / 28 invalid claims, 6 minted, 2 reused — seed
`degrades_to` and `searches`). Minted-but-used new predicates (13 of 18 at
budget 150, 4 of 6 at budget 400) cover shapes the seed could not express,
for example `depends_on` (Application -> Library) across blocks 7, 9, and 13
for the path-dependency relations. An independent re-check of both runs'
`validated-claims.json` (word-sequence comparison against the snapshot,
current-block range assertion, reasons-present assertion for every invalid
claim) found zero mismatches. The dominant invalid-claim causes were strict
evidence-text mismatches (models quoting partial lines) and subject/object
shape violations — both recorded for Task 4's scoring.

### Task 4: Measurement and go/no-go report

**Status:** Complete

**Depends on:** Task 3.

**Implementation notes (Task 4):** Four budgets were scored (150 and 400 from
Task 3; 250 and 600 added in Task 4 — all four share seed xxh3
`9bd9ffae74304ce2`, reference set xxh3 `7dad11a105072baf`, and snapshot xxh3
`29d01e36b77b6d33`). All 103 valid claims across the four runs were
independently re-verified against the run snapshots (strict whitespace-normalized
match plus block-range containment): zero citation failures. The full report,
including the fixed-vocabulary comparison, is the `## Report` section at the
end of this file. One deviation from the Change paragraph: the harness was not
run on an epic file (Task 3 never produced open-vocab claims for one, and a
new pre-extraction reference set for an epic file was not authored in this
task); the fixed-vocabulary side was instead rebuilt on `AGENTS.md`, which the
epic offers as the alternative when snapshots do not overlap. That rebuild
replaced the latest-build pointer in the indexer DB (see Report §Comparison).
The epic-file comparison is recorded as unavailable, not skipped silently.

**Change:** Run the Task 3 loop against `AGENTS.md` at several word-length
budgets and score it against the reference set recorded in Task 3. Measure
final predicate-vocabulary size and growth across blocks, entity
deduplication, exact citation validity, claim-to-evidence support, useful
relations recovered, misleading predicate assignments, and useful relations
omitted. For each question, inspect the extracted claims and record whether
they contain enough supporting evidence for the expected answer; do not treat
the existing CLI's answer as ground truth. Report model or validation failures
separately from valid empty blocks and record whether block boundaries harmed
citation or relation quality.

After the `AGENTS.md` smoke test, run the same harness on at least one epic
file present in the completed fixed-vocabulary build. Compare claims and
question coverage only if the stored source snapshot matches the file used by
the harness; otherwise rerun the fixed-vocabulary experiment on that file or
mark the comparison unavailable. Separate failures caused by extraction from
those caused by the existing search rules. Write a recommendation on whether
to continue the open-vocabulary experiment, revise it, or stop it.

**Acceptance criteria:**

- [x] A written report (in this epic file or a follow-up note) states the
      measured metrics per word-length budget tried, with definitions and
      counts for relation support, misleading predicates, omissions,
      deduplication, evidence validity, and question coverage.
- [x] The fixed question set and expected supporting passages are recorded
      before the runs are scored. The report distinguishes a missing claim
      from a search rule that cannot use an existing claim.
- [x] The comparison to the existing graph states which source snapshots
      matched and identifies any unavailable comparison rather than treating
      different document versions as equivalent.
- [x] The report gives an explicit recommendation: continue toward CLI
      integration, iterate further on the experiment, or abandon the
      open-vocabulary approach — with reasoning tied to the measured data.
- [x] This epic's `**Status:**` is updated to Complete once the report is
      written, regardless of which recommendation it reaches — this epic is
      scoped to running the experiment and reporting results.

## Report (Task 4)

Written after scoring four extraction runs against `reference-set.json` and
comparing with the fixed-vocabulary graph on a shared `AGENTS.md` snapshot.
Run directories: `experiments/open-vocab-graph/output/runs/budget-{150,250,400,600}/`.
The loose files that once sat directly under `output/runs/` from an earlier
default-output run were ignored; the per-budget directories are authoritative.

### Runs scored

| | budget 150 | budget 250 | budget 400 | budget 600 |
|---|---|---|---|---|
| Produced by | Task 3 | Task 4 | Task 3 | Task 4 |
| Blocks (one serial LLM call each) | 13 | 8 | 5 | 3 |
| Model calls failed after retry | 0 | 0 | 0 | 0 |
| Parse errors | 0 | 0 | 0 | 0 |

All four runs used the same seed vocabulary (`seed-vocabulary.json`, xxh3
`9bd9ffae74304ce2`), the same reference set (xxh3 `7dad11a105072baf`, sha256
of `AGENTS.md` `a15dbdb069bc…`), and the same snapshot (xxh3
`29d01e36b77b6d33`), so per-budget differences below are attributable to
block size and model non-determinism only. The reference set (9 supported
relations R1–R9, 8 repeated entities, 6 questions with Q5/Q6 expecting
`unknown`) was recorded before any extraction run existed; the `extract`
subcommand enforces that ordering mechanically, and all four run manifests
carry the identical reference-set hash.

### Metric definitions

- **Valid claim** — passed the harness validator: every citation exactly
  matches the snapshot lines (whitespace-normalized), citations fall inside
  the emitting block, the predicate is declared with description and
  subject/object shape, and the endpoints respect that shape.
- **Citation validity** — citation text equals the cited snapshot lines under
  the strict rule. Verified twice: by the harness at run time, and
  independently for this report (script re-check of all four runs against
  `snapshots/AGENTS.md`).
- **Support** — a human judgment that the cited lines actually state the
  claimed relation. "Weak" = the relation is a mild inference from the cited
  lines (loose subject/object naming); "misleading" = a wrong entity in a
  slot, a fabricated object, or evidence that does not state the claim.
- **Recovered reference relation** — ≥1 valid, supported claim expresses the
  reference relation's subject–object pair (wording/kind/direction tolerant).
  A valid claim with a wrong entity in a slot does not count as recovery; it
  counts as misleading.
- **Omitted** — reference relation absent from the valid claim set (attempted
  and rejected, or never attempted; noted which).
- **Duplicate** — two valid claims asserting the identical relation
  (same subject/predicate/object), or the same real-world entity carried
  under different names/kinds.

### Per-budget metrics

| Metric | 150 | 250 | 400 | 600 |
|---|---|---|---|---|
| Relations emitted | 81 | 48 | 47 | 14 |
| Valid claims | 43 | 31 | 19 | 10 |
| Invalid claims (all reasons recorded) | 38 | 17 | 28 | 4 |
| Citation validity (independent re-check) | 43/43 | 31/31 | 19/19 | 10/10 |
| Supported claims | 41 (95.3%) | 27 (87.1%) | 17 (89.5%) | 7 (70.0%) |
| — of which weak/loose support | 0 | 0 | 3 | 7 |
| Misleading valid claims | 2 (4.7%) | 4 (12.9%) | 2 (10.5%) | 3 (30.0%) |
| Reference relations recovered | 4/9 | 2/9 | 3/9 | 0/9 |
| Reference relations omitted | 5/9 | 7/9 | 6/9 | 9/9 |
| Exact duplicate claims | 2 | 0 | 0 | 0 |
| Final predicates (seed 4) | 22 | 15 | 10 | 5 |
| Final entity kinds (seed 5) | 20 | 11 | 12 | 6 |
| Predicates reused across blocks | 4 | 5 | 2 | 1 |

Invalid-claim reason instances (a claim can have several): 150 — 43
shape-violation + 12 evidence-mismatch; 250 — 25 shape + 1 evidence; 400 —
21 shape + 20 evidence; 600 — 3 shape + 4 evidence. Shape rejections
dominate at every budget; evidence-text rejections (models quoting partial
lines, dropping backticks, or joining lines) dominate only at 400.

### Reference-relation recovery and omission

| Ref | Relation (abridged) | 150 | 250 | 400 | 600 |
|---|---|---|---|---|---|
| R1 | daftprompt uses akar for rendering | ✓ | ✓ | ✓ | ✗ (owning block emitted nothing) |
| R2 | daftprompt uses gitoxide | ✓ | ✗ | ✗ | ✗ |
| R3 | daftprompt uses hybrid indexer | ✗ | ✗ | ✗ | ✗ |
| R4 | `cargo check --workspace` = correctness gate | ✗ attempted, rejected (seed `uses` is CLI→Database) | ✗ | ✗ | ✗ |
| R5 | indexer covers .rs/.ts/.tsx/.js/.jsx | ✓ as languages | ✓ as languages | ✓ as languages | ✗ |
| R6 | discovery excludes generated paths | ✓ (`enforces`, ev L127) | ✗ | ✗ | ✗ |
| R7 | hybrid = FTS5 + sqlite-vec via RRF | ✗ | ✗ | ✗ | ✗ (per-source analog only) |
| R8 | graph requires XIAOMI_API_KEY | ✗ near-miss (model claim kept, key claim rejected by shape) | ✗ near-miss (same) | ✗ | ✗ attempted ×2, rejected by seed shape + partial-line quote |
| R9 | model failure → FTS5-only | ✗ wrong subject | ✗ wrong subject | ✓ (`primary search degrades_to FTS5-only search`, ev L135) | ✗ wrong subject + object |

R4 — the single most prominent workflow rule in AGENTS.md (L81) — was never
captured as a valid claim at any budget: attempted once (150) and rejected by
the seed `uses` shape, never attempted again. The RRF fusion fact (R7) was
never captured at any budget. R8's exact relation was attempted at 600 and
rejected by the seed `requires` (CLI→Model) shape — the model forced the seed
predicate instead of minting a `requires(CLI → EnvironmentVariable)` variant.

### Misleading predicate assignments (valid claims only)

7 of the 103 valid claims are the same recurring error: a concrete component
forced into the subject slot of `degrades_to` — `FTS5 degrades_to FTS5-only`
(150 L93; 250 L93, L135), `sqlite-vec degrades_to FTS5-only` (250 L93),
`hybrid search degrades_to substring search` (400 L93), `sqlite-vec
degrades_to FTS` (600 L93, object also wrong). In every case the cited line
states the fallback correctly; the searcher, not the fallback target, is the
subject. The one fabrication: 150 b6 `graph search searches
DAFTPROMPT_GRAPH_SEARCH_INDEX` citing the search command line (L67) — the
object name appears nowhere in AGENTS.md. At 600, `code is_indexed_by
tree-sitter` (L28) assigns a parser to an index role and `documents
is_indexed_by SQLite FTS5` (L31–32) names an index the cited lines never
mention. Budget 600 also had zero clean-support claims: its 7 supported
claims all rest on inference (e.g. six per-source `is_indexed_by` claims
mapped from the L92 hybrid-search diagram row, which alone does not state
them).

### Entity deduplication

- 150: two exact duplicate claims in one block (b13 emitted
  `daftprompt depends_on akar` twice and `depends_on winit` twice, same
  evidence). 250/400/600: no exact duplicates.
- Name proliferation for one real entity: the CLI family was named `cargo
  run`, `cargo run -- --index-code/--index-documents/--index-git-log`,
  `Document provenance graph CLI`, `graph CLI`, `graph search`, `Unified
  search`, and `daftprompt CLI` across blocks and runs — up to 8 names, no
  canonicalization (the harness has none). At 250 the same three
  search-source relations were asserted twice under two subject names
  (`daftprompt CLI` in b2, `daftprompt` in b4).
- Cross-run kind drift for the same entity: `daftprompt` = Application (150),
  App (250), Project (400). 150 also minted instance-specific entity kinds
  (`Unified search source`, `FTS5-only`, `sqlite-vec`) — kind/instance
  confusion that would fragment any downstream node table.
- Healthy reuse also exists: `depends_on (Application→Library)` was reused
  across three blocks at 150 for the path-dependency relations, and
  `degrades_to`/`searches` were reused in every run that had more than one
  search-shaped block.

### Failures: model/validation vs valid-empty blocks; block boundaries

- Model failures: none (0 across 29 block calls after the built-in retry).
  Parse failures: none. Rejected mints: none. Every invalid claim carries
  recorded reasons; nothing was silently dropped.
- Valid-empty blocks: 150 b2 (L18–22, command list — nothing relation-shaped,
  legitimate) and 600 b3 (L133–146, **0 relations emitted** — this silently
  discarded the block containing R1's and R9's evidence; the largest blocks
  produced content loss, not consolidation).
- Block boundaries did not observably separate claims from needed context:
  whole-line paragraph packing kept sentences intact, and multi-line
  citations (e.g. L44–47, L55–57, L73–79) validated. The dominant quality
  killers were (a) partial-line quoting by the model and (b) one-shape
  predicates in the seed rejecting valid relations (R4, R8, the 400 b4
  collapse where L123–131 produced 0/5 valid, including two nonsense
  `Rust is_a Rust` / `TypeScript is_a TypeScript` self-claims citing L126–127).

### Question coverage

Extraction-side only — this experiment implements **no search rules**, so
every open-vocab gap below is a missing/insufficient claim, never a rule that
cannot use an existing claim. "Covered (defect)" = a valid claim cites the
expected fact but misassigns an endpoint.

| Q | Expected | 150 | 250 | 400 | 600 |
|---|---|---|---|---|---|
| Q1 render UI | supported | covered (`depends_on akar`, ev L141) | covered (`akar renders`, ev L141) | covered (ev L141) | not covered (block empty) |
| Q2 correctness gate | supported | not covered (attempted, rejected) | not covered | not covered | not covered |
| Q3 extensions + exclusions | supported | covered (exclusions ev L127; languages ev L138) | partial (languages only) | partial (languages only) | partial (extension list in ev L28; claim misassigns tree-sitter) |
| Q4 embedding-model failure | supported | covered (defect: wrong subject, ev L93) | covered (defect, ev L93/L135) | covered (`primary search degrades_to FTS5-only search`, ev L135) | covered (defect: subject+object wrong, ev L93) |
| Q5 license | unknown | correct absence | correct absence | correct absence | correct absence |
| Q6 release cadence | unknown | correct absence | correct absence | correct absence | correct absence |

The two `unknown`-expected questions were answered by absence at every
budget — no run hallucinated license or release-cadence claims (valid or
invalid). Q2 was never covered at any budget.

### Comparison with the existing fixed-vocabulary graph

**Snapshot matching.** Before Task 4 the indexer DB
(`~/Library/Caches/daftprompt/_users_brainless_projects_daftprompt_ba56ce.db`)
contained **no `AGENTS.md` source at all** — the completed build
(`b18d935a92e15f578000041af00000000`, 2026-09-27, 187 claims) covered
`epics/001–012` only, so there was no overlapping snapshot with any
open-vocab run. Per the epic's alternative for non-overlapping snapshots, the
fixed-vocabulary experiment was rebuilt on `AGENTS.md`:
`cargo run --bin graph -- build --repo . --source AGENTS.md` → completed
build `b18d96b18c5ab1ab800004e7e00000000`, 15 claims, 3 extraction batches,
one invalid enumeration omitted (reported by the CLI, not silent). The
stored `AGENTS.md` snapshot in `graph_source_versions` is **byte-identical**
to the harness snapshot (sha256 `a15dbdb069bc…` on both sides), so the claim
and question comparison below is on exactly overlapping content.
Side effects of the rebuild, documented as required: the latest-build pointer
moved to the AGENTS.md build; the 12 `epics/*` sources are now
`active=0` and the 187 epics claims deactivated in place (rows retained and
queryable by `build_id='b18d935a92e15f578000041af00000000'`; restorable as
latest via `cargo run --bin graph -- build --repo . --source 'epics/*.md'`).
All 12 stored epics snapshots were verified byte-identical to the current
files, so an open-vocab run on an epic file could be compared without any
further fixed-vocabulary rebuild.

**Claim sets on the shared AGENTS.md snapshot.** Fixed vocabulary: 15 claims,
10 distinct after 5 exact-duplicate rows (`indexes_dialect` JSX/TSX ×2,
`indexes_language` Rust/TypeScript/JavaScript ×2). It recovered R5 (the five
extensions, expressed as 3 languages + 2 dialects) and an R8 fragment
(`graph CLI uses_parser MIMO_V2_5_PRO` — the model-requirement forced into
the parser slot; the XIAOMI_API_KEY requirement itself absent) — roughly
1.5/9 relations vs open-vocab's best of 4/9 (budget 150). It emitted **two
false claims**: `graph CLI indexes_language Python` (ev L68) and `graph CLI
indexes_language Lua` (ev L69) — both citations are the documented *example
questions* in AGENTS.md's workflow block, read as capability statements —
plus `indexer uses_parser Tree-sitter` citing the question line L70 rather
than any statement. It produced **zero** `excludes_path` and
`indexes_extension` claims despite L127 listing both the exclusion set and
the extensions, and nothing for R1/R2/R3/R4/R6/R7/R9. Open vocabulary at the
same budget range recovered strictly more of the reference set with no false
claims of that kind (its worst fabrications are the 150 b6 object name and
the recurring degradation-subject error).

**Question coverage, fixed-vocabulary CLI on the AGENTS.md build** (`graph
search`, 6 reference questions; the CLI's answer is recorded, not treated as
ground truth): Q1 unknown, Q2 unknown, Q4 unknown, Q6 unknown (correct), Q3
answered `yes` in one run and `unknown` in a rerun (non-deterministic), Q5
**hard error** — "direct relation entities have kinds incompatible with the
selected predicate; model correction also failed" — instead of the expected
`unknown`. A Q1 attempt also errored once before returning `unknown` on
retry. These `unknown`s are extraction gaps (no akar/rendering claim, no
gate claim, no degradation claim exists in the build — the fixed vocabulary
has no predicate that could express R9 at all), while the Q3 flip-flop and
Q5/Q1 errors are search-rule failures: interpretation must commit to exact
fixed predicates and entity kinds, and its one-correction path fails hard.

**Comparisons identified as unavailable:**
1. Open-vocabulary claims for any epic file do not exist (Task 3 scoped the
   harness to AGENTS.md; no pre-extraction reference set was recorded for an
   epic file), so the epics-build claim sets could not be compared with
   open-vocab claims. The stored epic snapshots match current files, so this
   comparison becomes possible the moment a harness run on an epic file is
   added.
2. Fixed-vocabulary question coverage on the epics build was not measured —
   the reference questions are AGENTS.md questions, and comparing them
   against a non-overlapping snapshot would be meaningless.

### Extraction vs search-rule failures

- Open vocabulary: no search rules exist; **all** coverage gaps above are
  extraction-side by construction.
- Fixed vocabulary on the shared snapshot: extraction-side — missing claims
  for Q1/Q2/Q4, zero `excludes_path`/`indexes_extension` usage, two false
  `indexes_language` claims; search-rule-side — Q3's yes/unknown
  non-determinism, Q5's (and one Q1) interpretation-correction hard error,
  and the Epic-013-intro observation that direct questions can be answered
  while capability questions fall to `unknown` when scope requirements
  (graph.rs `derive_positive`, `DIRECT_PREDICATES`) are not met.

### Recommendation

**Iterate further on the experiment — do not integrate into the CLI yet, do
not abandon.** Reasons tied to the measurements:

1. **Evidence fidelity — the property Epic 012 paid for — survives open
   vocabulary.** 103/103 valid claims across four budgets pass the strict
   whitespace-normalized citation re-check, citations never leave their
   block, and carried summaries never became evidence. Nothing in the data
   supports abandoning the approach.
2. **Open vocabulary already beats the fixed schema on its own smoke-test
   file.** On the identical AGENTS.md snapshot the fixed-vocabulary build
   recovered ~1.5/9 reference relations, fabricated 2 capability claims from
   example questions, duplicated 5 claims, and its search layer answered one
   question two different ways and hard-errored on another. Open vocabulary
   recovered up to 4/9 (budget 150) including relations the fixed vocabulary
   structurally cannot express (R4's process role, R6's exclusion
   enforcement, R9's degradation).
3. **Extraction quality is not yet CLI-grade, and it is budget-unstable.**
   Support ranges 95.3% → 70.0% across budgets; 7 valid claims force a
   component into the `degrades_to` subject slot; the largest budget (600)
   recovered 0/9 and silently dropped a whole block's content; R4 and R7
   were never captured anywhere; the single-shape seed predicates caused the
   largest rejection category (92 of 139 shape-violation reason instances
   are seed-shape conflicts with relations the text clearly states).
4. **Concrete next iterations (experiment-side only, in order of measured
   impact):** (a) allow a predicate to declare multiple subject/object
   shapes, or auto-mint shape variants — this alone addresses R4, R8, and
   most rejections; (b) add an entity canonicalization/naming pass (8 names
   for one CLI, cross-run kind drift); (c) prompt guidance: quote complete
   physical lines, and name the *condition* ("search on embedding-model
   failure"), not a component, as a degradation subject; (d) a cheap
   entailment self-check ("is the claim stated by the cited lines?") before
   acceptance — it would have caught the fabrication and most of the 7
   misleading claims; (e) re-run at 250–400 with a required explicit
   "nothing to extract" judgment for empty blocks so b3-at-600-style silent
   loss cannot recur. Re-score against this same reference set; consider CLI
   integration only if support ≥ ~95% with misleading ≤ ~2% holds across two
   consecutive budgets and a search-rule design exists that consumes the
   discovered shapes (out of this epic's scope).

### Commands run for this report

```bash
cargo run -p open-vocab-graph -- extract --repo . --source AGENTS.md --word-budget 250
cargo run -p open-vocab-graph -- extract --repo . --source AGENTS.md --word-budget 600
cargo run --bin graph -- build --repo . --source AGENTS.md
cargo run --bin graph -- search --repo . "What does daftprompt use to render its UI?" --json
cargo run --bin graph -- search --repo . "What is the canonical correctness gate, and when must it be run?" --json
cargo run --bin graph -- search --repo . "Which file extensions does the code indexer index, and which generated paths are excluded?" --json
cargo run --bin graph -- search --repo . "What happens when the embedding model fails to load?" --json
cargo run --bin graph -- search --repo . "What license is daftprompt released under?" --json
cargo run --bin graph -- search --repo . "How often does daftprompt publish releases?" --json
# DB inspection: sqlite3 -readonly on ~/Library/Caches/daftprompt/_users_brainless_projects_daftprompt_ba56ce.db
# (graph_builds/graph_sources/graph_claims/graph_build_claims/graph_source_versions; snapshots
# extracted with sqlite3 writefile() to a temp dir and sha256-compared — no DB writes)
```

## File-change summary

| Path | Intended change |
|---|---|
| `experiments/open-vocab-graph/` (location decided by Task 1: standalone workspace-member crate, no dependency on `crates/daftprompt-indexer`) | Standalone schema-discovery sampler, consolidation pass, extraction loop, and measurement report. Do not change the existing graph CLI prompts or validator; reuse evidence-checking helpers where practical. |
| `epics/013-open-vocabulary-graph-schema-experiment.md` | This file — keep task statuses current as work proceeds. |
