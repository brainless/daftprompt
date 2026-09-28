# Epic 014 — Frozen Experiment Inputs (Task 1)

Immutable comparison inputs shared by the Epic 013 LLM baseline (re-scored here) and all
three Epic 014 GLiNER2.5 checkpoint runs. See `MANIFEST.json` for hashes and provenance
and `comparison-plan.md` for the baseline identification and schema-difference labels.

## Contents

| File | Role |
|---|---|
| `AGENTS.md` | Source snapshot (146 lines, sha256 `a15dbdb0…`, xxh3 `29d01e36b77b6d33`). All citations must resolve against this copy. Line 7 is the `daftprompt uses akar` evidence line. |
| `reference-set.json` | Scoring ground truth recorded before any extraction run existed (R1-R9 with verbatim passages, 8 repeated entities, Q1-Q6 with Q5/Q6 expecting `unknown`). xxh3 `7dad11a105072baf` — identical to all four Epic 013 runs' recorded hash. |
| `seed-vocabulary.json` | The one chosen discovered seed (xxh3 `c776d7e579d8aed5`): Epic 013 Task 2 consolidation output (zero-merge pass, `mimo-v2.5-pro`) consumed by the budget-250 extraction run. Seed kinds: Command/Configuration/Tool/DataSource; `uses` is `Command→Tool` — the narrow shape under which the motivating line-7 failure occurred. Task 2 converts this seed into the versioned GLiNER2.5 schema. |
| `MANIFEST.json` | sha256 (+ xxh3 provenance) for each artifact, origin documentation, immutability policy, cross-checks performed at freeze time. |
| `check_inputs.py` | Stdlib-only verifier (sha256, line-7 expectation, reference-set passages, repeated entities, question passages, test-input quotes/misquotes, seed fingerprint). Run before inspecting any new model output. |
| `check-inputs-run-2026-09-28.log` | Output of the pre-model verification run performed in Task 1 (71/71 checks passed). |
| `comparison-plan.md` | Which LLM run is the baseline; whether its schema is identical to the GLiNER2.5 schema (it is not — differences labeled); frozen comparison constants. |
| `test-inputs.json` | The AGENTS.md line-7 case plus targeted positive/negative inputs for R1-R9, including two recorded misquotes that strict matching must reject. |
| `llm-baseline/` | Re-score of the actual budget-250 folder as the LLM baseline: `rescore-budget-250.py` (generator/verifier), `budget-250-rescore.json` (per-claim judgments + mechanical citation re-checks), `budget-250-rescore.md` (scorecard). |

## Immutability policy

1. **Never edit, re-serialize, or overwrite anything in this directory.** Files are
   chmod 444, directories 555.
2. Derived artifacts go elsewhere: GLiNER runs → `experiments/open-vocab-graph/gliner-runs/`
   (Task 3+); anything else → a new clearly-named directory.
3. If an input ever must change (e.g. AGENTS.md moves ahead), freeze a **new versioned
   directory** (e.g. `frozen-inputs-v2/`), record the reason in the epic, and re-run the
   baseline decision — never modify these files in place.
4. `check_inputs.py` is the mechanical tripwire: it fails if any artifact hash, source
   line, quote, or seed fingerprint drifts.

## Why this seed (and not the other one)

Epic 013 produced **two** seeds. Budgets 150/400/600 used an earlier one (xxh3
`9bd9ffae74304ce2`; kinds CLI/Database/Model/SearchIndex/DataSource, `uses`=CLI→Database)
that survives only as re-serialized copies inside those run directories. The budget-250
run used the later consolidation output (xxh3 `c776d7e579d8aed5`), which is byte-identical
to the top-level `output/seed-vocabulary.json` and is frozen here. Consequences:

- The chosen baseline (the actual budget-250 folder, 40 validated claims) ran under
  exactly this seed — baseline and GLiNER runs are same-seed.
- The motivating Epic 014 failure (`daftprompt uses akar` rejected because the model
  typed `daftprompt` a `Tool` and quoted a partial line 7) happened under this seed.
- **Epic 013's report column “budget 250” (31 claims) is stale** — it describes a run
  under the *other* seed. No Epic 014 comparison may use that column. The 150/400/600
  runs are different-seed context only.

## Verification (run before any model work)

```bash
python3 experiments/open-vocab-graph/frozen-inputs/check_inputs.py   # 71 checks, exit 0
python3 experiments/open-vocab-graph/frozen-inputs/llm-baseline/rescore-budget-250.py  # regenerates the re-score deterministically
```
