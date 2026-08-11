# OpenRouter helper experiment protocol v1

This fixed protocol prepares Epic 014 Task 5 plan steps 6–7. It does not
record a live result and does not satisfy either remaining Task 5 criterion.

## Frozen inputs

- Models: `ibm-granite/granite-4.1-8b`,
  `meta-llama/llama-3.1-8b-instruct`, and `mistralai/mistral-nemo`.
- Task 5 helper controls: `LAB-C01-REQUEST` (sufficient initial context),
  `LAB-C02-REQUEST` (missing/exhausted evidence), `LAB-C07-REQUEST`
  (scope/runtime control), and the controlled prompt-injection fixture.
  These all run against the daftprompt Task Zero graph.
- The three `LAB-*` controls reuse the exact **expert** request renderings and
  exact mutation constraints from `manifest.md` §3, but their repository,
  graph, and packet are not canonical C01/akar, C02/Keystone, or C07/dwata.
  Their rendering identities are `derived:daftprompt-graph:C01-expert:v1`,
  `derived:daftprompt-graph:C02-expert-sanitized:v1`, and
  `derived:daftprompt-graph:C07-expert:v1`; the controlled injection uses
  `controlled:helper-injection:v1` and is explicitly not a manifest case.
  Every replay artifact records the rendering identity, xxh3 request hash,
  mutation-constraints hash (constraints joined by `\n`), and provenance.
  Unit tests pin the exact strings and hashes.
- Repository revision, graph JSON, packet JSON, prompt-pattern files, helper
  protocol version, and policy JSON are frozen before the first call. Their
  xxh3 hashes are recorded in every replay artifact.
- Policy: `HelperPolicy::default()`; temperature 0; output limit 2,048; exact
  provider order pinned per model; fallback disabled; required parameters,
  denied data collection, and ZDR enabled.

## Repetition and ordering

Run three repetitions for every model/case pair (36 runs total). Use the same
case order for each model and rotate the starting model by repetition to
reduce ordering/time-window bias. A failed or rejected response counts as its
run; do not silently retry it. Record a replacement only as an additional,
explicitly numbered run after diagnosing the failure.

This matrix can support only Task 5's question of whether the three helpers
operate through the same closed interface over fixed inputs. It cannot count
as a canonical corpus run or Task 6 comparative evidence. Task 6 requires
true repository/revision-specific graph and packet fixtures for the manifest
cases.

## Sanitization and replay admission

Wrap the live adapter in `DecisionRecorder`; for each inference retain only
`InferenceRecord` plus the typed `HelperModelOutput` (adapter errors become a
generic malformed marker while the live host still receives the error). Never retain the API key, round prompt, raw response,
provider error body, or private repository excerpts. Write
`SanitizedReplayArtifact` JSON, verify its model/protocol/count invariants,
review it for accidental content disclosure, then replay with
`SanitizedReplayArtifact::scripted_model()` without credentials or network.
The raw-response and prompt hashes establish identity without preserving the
underlying bytes. Failure artifacts use an empty raw-response hash and the
adapter's generic suppressed-error diagnostic.

## Measurements and interpretation

Record calls, rounds, token counts, result bytes, elapsed time, duplicates,
stop reason, validation diagnostics, gap recall, unsupported claims, prompt
bytes, and intent preservation. Compare all helpers to the deterministic
baseline. Availability, a single successful completion, or schema compliance
alone is not evidence that a helper improves prompt quality or task outcome.

## 2026-08-11 smoke-run disposition

Exact provider pins discovered from the public endpoint catalog were
CoreWeave for Granite 4.1 8B and DeepInfra for both Llama 3.1 8B Instruct and
Mistral Nemo. The required first smoke run used Granite/CoreWeave and a newly
authored public control derived from C01, now identified as
`DERIVED-SMOKE-C01` / `derived:smoke-c01:v1` rather than represented as
canonical C01. Routing was fallback-disabled, parameter-required,
data-collection-denied, and ZDR-enabled. It failed before a completion was
returned: zero tokens, no returned model/provider identity, and the sanitized
`adapter_unavailable` classification. A separate read-only OpenRouter credits
query reported total credits `0`, which is the exact sanitized blocker. The
fixed 36-run matrix was not started and neither remaining Task 5 criterion is
satisfied. See `openrouter-smoke-blocker-2026-08-11.json`.
