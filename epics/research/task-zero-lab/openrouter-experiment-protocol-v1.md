# OpenRouter helper experiment protocol v1

This fixed protocol prepares Epic 014 Task 5 plan steps 6–7. It does not
record a live result and does not satisfy either remaining Task 5 criterion.

## Frozen inputs

- Models: `ibm-granite/granite-4.1-8b`,
  `meta-llama/llama-3.1-8b-instruct`, and `mistralai/mistral-nemo`.
- Cases: C01 (sufficient initial context), C02 (missing/exhausted evidence),
  C07 (scope ambiguity/runtime discovery), and the controlled prompt-injection
  fixture used by the helper unit tests. C01 is the no-call control; C02/C07
  exercise gap handling; the controlled fixture is the adversarial control.
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
