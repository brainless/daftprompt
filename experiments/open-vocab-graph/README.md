# open-vocab-graph (Epic 013 experiment)

Standalone harness for the open-vocabulary graph schema experiment. It lives
here — outside `crates/daftprompt-indexer/src/` and the `graph` CLI — so the
experiment can iterate on prompts and sampling without touching production
prompts, validators, or storage. The crate has no dependency on
`daftprompt-indexer`; Task 1's file-change summary in
`epics/013-open-vocabulary-graph-schema-experiment.md` left the location open
and this directory is the decision. Later tasks (consolidation, sequential
extraction, measurement) are expected to join this crate as additional
subcommands or modules.

## Task 1: schema-discovery sampler

```bash
# Sample + prompt dump only (no LLM call) — used for determinism checks:
cargo run -p open-vocab-graph -- sample --repo . --source AGENTS.md --dry-run

# Full run: one batched LLM call proposing the graph vocabulary
# (requires XIAOMI_API_KEY in the shell or a git-ignored .env in the CWD):
cargo run -p open-vocab-graph -- sample --repo . --source AGENTS.md
```

`--source` accepts repo-relative paths or quoted globs with the same semantics
as the `graph` CLI (sorted, deduplicated, `.md`/`.markdown`/`.txt`/`.text`,
UTF-8). `--output` defaults to `experiments/open-vocab-graph/output` relative
to the current directory.

## Outputs

- `sampled-blocks.json` — per file: path, xxh3-64 content hash (the sole
  sampling seed), paragraph block count, and the sampled blocks with
  1-based inclusive line ranges and exact text. Same file set + unchanged
  content reproduces this file byte-for-byte.
- `prompt.txt` — the full system + user prompt sent to the model.
- `proposed-vocabulary.json` — the exact raw model response (one JSON object
  with `entity_kinds`, `predicates`, `example_relations`), consumed by Task 2.

## Task 2: consolidation pass

```bash
# Requires XIAOMI_API_KEY (same as Task 1):
cargo run -p open-vocab-graph -- consolidate \
  --input experiments/open-vocab-graph/output/proposed-vocabulary.json
```

`--output` defaults to
`experiments/open-vocab-graph/output/seed-vocabulary.json` and `--log` to
`experiments/open-vocab-graph/output/consolidation-log.json`.

Approach: a second LLM call proposes merge groups ("merge synonyms in this
list, keep distinct concepts separate"); a deterministic validator then
applies only safe groups — members must exist verbatim in the proposed
vocabulary, each entry may be assigned at most once, and a predicate group
must share one subject/object shape after entity-kind canonicalization. Every
applied or rejected group is recorded with the model's rationale. This was
chosen over pure string/embedding similarity because near-duplicates like
`uses`/`depends_on` are semantic synonyms that string similarity cannot see,
and embedding models would add a model download to this standalone crate. The
consolidation model can only group existing entries; it cannot rename, reword,
invent, or drop them, so the output vocabulary is smaller-or-equal to the
input by construction.

Outputs:

- `seed-vocabulary.json` — the consolidated seed for Task 3. It keeps the
  proposed vocabulary's base shape (`entity_kinds`, `predicates`,
  `example_relations`, with descriptions and subject/object kinds carried
  through) so one set of types loads both; entries that absorbed a synonym
  additionally carry `merged_from`. Example relations are rewritten to
  canonical names.
- `consolidation-log.json` — method, model, the exact raw model response
  (before validation), input/output counts, the explicit no-merge rationale
  when nothing was merged, and every applied or rejected merge decision with
  what each merged-away entry carried.

A zero-merge outcome (the model proposed nothing, or everything it proposed
was rejected) is a valid result and is recorded explicitly in
`no_merge_rationale` rather than left implicit. Against Task 1's `AGENTS.md`
output the run produced no merges: the five entity kinds are distinct and the
four predicates have four different relationship shapes. The merge code path
itself is exercised by unit tests on a synthetic `uses`/`depends_on`
vocabulary and by an end-to-end run of the same subcommand against a
synthetic input.

## Task 3: sequential carryover extraction loop

```bash
# Requires XIAOMI_API_KEY (same as Tasks 1-2). `--word-budget` is required;
# --output defaults to output/runs/budget-<word_budget>.
cargo run -p open-vocab-graph -- extract --repo . --source AGENTS.md --word-budget 150
cargo run -p open-vocab-graph -- extract --repo . --source AGENTS.md --word-budget 400
```

The loop splits each source into complete-line blocks bounded by the word
budget (paragraph units packed greedily; a single paragraph longer than the
budget becomes its own oversized block because a physical line is never
broken), then walks the blocks in order — one serial LLM call per block,
carrying the previous call's running summary and the accumulated open
vocabulary. The vocabulary is seeded from Task 2's `seed-vocabulary.json` and
is a soft prior: the prompt biases the model toward reuse but allows minting
new entity kinds/predicates (with recorded meaning and subject/object shape)
when nothing fits.

A run refuses to start unless `output/reference-set.json` exists and declares
`recorded_before_any_extraction_run`, so the Task 3 ordering (reference set
recorded before any extraction run is inspected) is enforced mechanically.
Evidence is validated with Epic 011/012's strict approach, reimplemented
locally to keep the crate independent of `daftprompt-indexer`: a citation is
valid only when its text equals the real snapshot lines exactly after
whitespace normalization, with no prefix-forgiveness or re-anchoring. Every
claim must also cite lines inside the current block only — a carried summary
is context, never evidence. Raw responses are written to disk before any
validation; failed claims are recorded with reasons in `invalid-claims.json`,
never silently dropped.

Per-run outputs (under `output/runs/budget-<N>/`):

- `run-manifest.json` — budget, sources, seed/reference-set/snapshot hashes,
  per-block records, counts (valid/invalid/parse errors/model errors),
  vocabulary summary, and the vocabulary-reuse report (predicates with valid
  relations in multiple blocks; minted predicates with valid use).
- `blocks/block-NN.request.json` — the exact system + user payload sent.
- `blocks/block-NN.raw.json` — the raw model response, recorded before any
  validation (`block-NN.error.json` if a model call failed after one retry).
- `blocks/block-NN.validation.json` — per-block valid/invalid split, mints,
  and rejected mints.
- `validated-claims.json` / `invalid-claims.json` — the complete claim set
  with block ids, endpoints, evidence, and (for invalid claims) reasons.
- `vocabulary-growth-log.json` / `final-vocabulary.json` — what was minted
  per block and the seed-plus-mints vocabulary the run ended with.
- `snapshots/<source>` — the exact source text used (Task 4 compares against
  the fixed-vocabulary build's stored snapshot).
- `reference-set.json` / `seed-vocabulary.json` — copies proving which
  reference set and seed the run was built against.
