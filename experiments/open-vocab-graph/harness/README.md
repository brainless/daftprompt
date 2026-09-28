# Epic 014 Task 3 — local GLiNER2.5 extraction harness

A standalone Python adapter that loads a GLiNER2.5 checkpoint **by explicit
model ID**, passes the versioned schema (`../schema/gliner25-schema.json`) to
it, extracts from the frozen AGENTS.md snapshot in recorded whole-line chunks,
and writes per-chunk raw output plus normalized entity/relation candidates to
a fresh run directory. Nothing in `../frozen-inputs/`, `../schema/`, or Epic
013's `../output/` is ever modified.

## Usage

Prerequisite beyond `pip install "gliner2[local]"`: the venv needs
`protobuf` (GLiNER2.5 DeBERTa-v3 tokenizers load through transformers'
slow-tokenizer path, which raises `ImportError` without it; observed on
gliner2 2.0.0 / transformers 4.57.6 / Python 3.14).

```bash
cd experiments/open-vocab-graph

# chunk plan only (offline, no model): boundaries, overlaps, word counts
../../.venv/bin/python -m harness plan

# verify inputs + chunk plan + checkpoint relation head, offline, no weights
../../.venv/bin/python -m harness extract --plan-only \
    --model fastino/gliner2.5-small-v1

# full run (downloads the checkpoint into --cache-dir; Task 5+)
../../.venv/bin/python -m harness extract \
    --model fastino/gliner2.5-small-v1 \
    --cache-dir .model-cache

# variants: jointie (default, typed endpoints) | independent (untyped)
../../.venv/bin/python -m harness extract \
    --model fastino/gliner2.5-base-v1 --variant independent --run-label indep
```

One command selects any of the three named checkpoints
(`fastino/gliner2.5-small-v1`, `fastino/gliner2.5-base-v1`,
`fastino/gliner2.5-multi-v1`) via `--model`; no source changes. Other useful
flags: `--device auto|cpu|mps|cuda`, `--revision <sha>` (pin the checkpoint),
`--local-files-only` (offline rerun from cache), `--chunk-words` (default
384), `--overlap-lines` (default 2), and the JointIE decoding knobs
(`--optimizer`, `--beam-size`, `--candidate-threshold`,
`--relation-role-threshold`, `--entity-threshold`, `--top-k-entities`/
`--top-k-roles`) or `--threshold` for the independent variant.

Cache note (gliner2 2.0.0 limitation, Task 5): `AutoExtractor.from_pretrained`
drops hub options (`cache_dir`/`local_files_only`/`revision`) when delegating
to the architecture loader, and the tokenizer loader gets none at all, so
weights/tokenizer would silently land in the default `~/.cache/huggingface`
and `--local-files-only` would not be offline. The adapter therefore redirects
`HF_HUB_CACHE`/`HF_HUB_OFFLINE` from `--cache-dir`/`--local-files-only` before
loading (`adapter.apply_hub_env`). `--revision` is still resolved as `refs/main`
by the gliner2 weight path — the manifest records the resolved commit sha.

Exit codes: `0` completed · `1` startup/verification/load failure · `2`
argparse error · `3` infeasible chunks recorded · `4` per-chunk extraction
errors recorded. Empty chunks are normal and exit 0 — they are recorded, never
hidden.

## Layout

| File | Role |
|---|---|
| `chunking.py` | Pure whole-line chunker: boundaries, char offsets, word counts, overlap lines, oversized single-line chunks. No gliner2/torch imports. |
| `normalize.py` | Pure normalization: chunk→document span remapping + verification, label→canonical-predicate mapping, schema-pair validation verdicts, span-stable dedup. No gliner2/torch imports. |
| `adapter.py` | Checkpoint verification (relation-head gate from `config.json` before any weight download), device resolution, HF revision probe, `JointIE`/`AutoExtractor` loading. gliner2 imported lazily. |
| `runner.py` | Orchestrates one run: snapshot sha tripwire, chunk plan, per-chunk extraction records, run-directory writing. Engine is injectable for tests. |
| `cli.py` / `__main__.py` | `python -m harness {extract,plan}`. |
| `test_harness.py` | 17 plain-assert tests; no checkpoint needed: `.venv/bin/python experiments/open-vocab-graph/harness/test_harness.py` |

## Variants

- **`jointie`** (default): `gliner2.joint_ie.JointIE` + a `JointSchema` built
  from the artifact's `model_mapping.joint_ie.labels` (Task 2's mapping; e.g.
  `uses` = Application→Subsystem|Tool, `uses__2` = Command→Tool). Typed
  endpoints and JointIE's feasibility flag (`result.feasible`) are recorded
  per chunk; an infeasible chunk is a visible status, not an empty result.
- **`independent`**: one `AutoExtractor` schema with all entity kinds
  (descriptions from the artifact) plus untyped `relations`. The relation head
  carries no endpoint typing, so kinds are attached post-hoc by exact span
  match against the same chunk's entity mentions; unmatched endpoints keep
  kind `""` and the candidate is recorded as rejected (`unknown_*_kind`),
  never dropped. Endpoint pairs are enforced by the same validator
  (`core.validate_claim`). Run it only if it addresses a measured JointIE
  limitation — same inputs, same scoring contract.

## Chunking policy

Chunks are packed from **whole physical lines** (default budget 384
whitespace-split words; line 7 of the snapshot fits whole). Deliberate
deviation from gliner2's native word-token chunker, which can cut a chunk
mid-line and would break Task 4's line-citation mapping. Each chunk records
`start_line`/`end_line` (1-based, inclusive), `start_char`/`end_char` (so
`snapshot[start:end] == chunk.text`), `word_count`, `overlap_lines` (the
trailing whole lines re-sent from the previous chunk, default 2), and
`oversized` (a single line longer than the budget gets its own chunk, kept
whole, with no overlap recorded after it). JointIE relations require both
endpoint mentions in the same chunk, so recorded overlap lines let the
analysis separate cross-chunk misses from extraction misses.

## Normalization and dedup

- Model chunk-local spans are remapped to document offsets and verified
  against the snapshot text; clamped/mismatched spans are flagged in `notes`,
  never silently trusted.
- Entity mentions deduplicate on the stable key `(kind, doc_start, doc_end)`;
  relations on `(canonical_predicate, subject_span_key, object_span_key)`.
  Chunk-overlap copies collapse (max confidence wins, `occurrences` and
  `source_chunks` record the merges). Distinct spans stay distinct even when
  the surface text is identical (e.g. repeated `cargo run`), and distinct
  kinds at the same span stay distinct.
- Every relation candidate is validated against the schema's declared
  endpoint pairs; rejected candidates are kept with verdict codes
  (`disallowed_pair`, `unknown_*_kind`, ...). A relation whose endpoint id is
  missing from its chunk's entities goes to the chunk record's `dropped` list
  with its raw record.
- Note: relation rows carry endpoint `occurrences` counted over relation
  mentions, which may differ from the same mention's row in `entities.json`;
  join on `(kind, start, end)` when correlating (Task 4).

## Run outputs

`gliner-runs/<model_slug>/<variant>/<UTCtimestamp>[-label]/` — never
overwritten (a collision fails the run):

- `manifest.json` — command line, model ID + HF revision (best-effort probe)
  + `config.json` sha256, package versions (gliner2/torch/transformers/
  python/platform), resolved device, thresholds, chunking record, schema
  artifact sha256 + seed xxh3, snapshot sha256 + line count, elapsed time,
  counts, run status, and `resources` (peak process RSS from
  `resource.getrusage`, plus CUDA peak when applicable — Task 5).
- `chunks/chunk-NNN.json` — per chunk: boundaries/overlaps, status
  (`ok`/`empty`/`infeasible`/`error`), the **raw model response verbatim**,
  normalized counts, dropped records, notes, per-chunk elapsed time, error.
- `entities.json`, `relations.json` — deduplicated candidates (relations with
  accepted/rejected verdicts).
- `summary.json` — counts and status (`completed`,
  `completed_with_infeasible_chunks`, `failed_infeasible`,
  `completed_with_chunk_errors`).

## Fails-visibly gates

- Snapshot sha256 must match the schema artifact's frozen
  `source_snapshot.sha256` (and line count), else `SnapshotMismatchError`.
- The checkpoint's `config.json` is inspected before weight download: a span
  checkpoint, or a boundary checkpoint with
  `boundary_head.enable_relations=false` (including pre-`config_version` 3
  configs, which migrate to disabled), raises `CheckpointCapabilityError`
  naming the checkpoint and the missing head. This applies to both variants.
- Engine load failures abort before any run directory is written. Per-chunk
  extraction errors are recorded in that chunk's record (status `error`) and
  the run continues; the CLI exits 4 and prints a warning.

## What is deferred (Task 5+)

Live checkpoint download and the real model runs (small → base → multi),
threshold tuning if needed (reported separately per Task 6), and
citation/line grounding and reference-set scoring (Task 4). Peak-memory
measurement landed with Task 5 (`manifest.resources.peak_memory_bytes`).
The live path is verified offline as far as possible: `--plan-only` checks
schema loading, snapshot sha, the chunker, and the relation-head gate without
weights; `--local-files-only` produces a clear offline failure until a
checkpoint is cached.
