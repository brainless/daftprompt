# Epic 002: Parsing refinements

## Outcome

Refine the Scribe parser from [Epic 001](001-text-parsing.md) based on its baseline evaluation. Epic 001 deliberately chose the simplest workable rules; this epic collects the changes that were deferred.

References: [PRD](../docs/PRD.md), [data model](../docs/data-model.md), and [Epic 001](001-text-parsing.md).

## How to use this epic

- **Improve if needed** items each have a trigger. Do them only when the Epic 001 evaluation (or later use) shows the trigger. Re-run the fixture evaluation before and after each change and record both results.
- **Good to have** items are not driven by a measured problem. Pick them up when convenient or when a later epic needs them.

## Improve if needed

### Retry with corrective feedback

- **Trigger:** a high invalid-output rate, or many parses needing more than one attempt.
- Epic 001 retries validation failures from scratch. Instead, send the specific validation error back to the model (Rig hooks support `retry_with_feedback`, see `crates/rig-agent/src/agent/hook.rs`) within the same attempt budget.
- Compare invalid-output rate, attempts per parse, and latency against the fresh-retry baseline.

### Sub-line segmentation

- **Trigger:** poor span accuracy on long lines, such as unwrapped prose paragraphs, minified JSON, or long email lines, where segment range plus quote is not enough.
- Split long lines further (for example, by sentence) and present sub-segments to the model, keeping the same byte-range and coordinate rules.

### Overlapping and nested spans

- **Trigger:** fixtures or real use show annotations that need overlapping or nested spans (for example, a question inside a larger answer).
- Relax the no-overlap rule, and define how the GUI renders nested or overlapping blocks in source order.

### Unreferenced spans

- **Trigger:** a meaningful share of invalid outputs is caused only by spans no annotation references.
- Drop unreferenced spans instead of rejecting the whole output, and record how many were dropped in the run metadata.

### Native JSON-schema output

- **Trigger:** the chosen provider or model handles the `submit` tool call poorly.
- Evaluate Rig's native structured-output mode against the output-tool mode on the same fixtures.

### Input limits and chunking

- **Trigger:** the default 32 KiB / 1,000-segment limit rejects realistic inputs, or is larger than the selected model handles reliably.
- Tune the default per model. If inputs routinely exceed the model's capacity, design chunked parsing with answer links across chunks.

### Second provider or model

- **Trigger:** the first model's baseline quality or latency is not good enough.
- Run the same fixture evaluation with another provider or model and compare the results.

## Good to have

- **Raw attempt logging:** store the raw model output of every attempt, including failed ones, as payloads linked to the run, to make failures easier to inspect.
- **Stable toolchain for non-GUI crates:** keep `daftprompt-types`, `daftprompt-core`, and `daftprompt-api` building on stable Rust, checked separately from the nightly toolchain akar needs.
- **Evaluation history:** write evaluation results to a report file per run so results can be compared across prompt versions and models.
- **Assignees beyond questions:** extract explicit assignment intent on non-question text (for example, "@bob please update the docs"). This needs a new label kind in the data model vocabulary, such as a task or action label.
- **GUI interaction:** click an answer to jump to the questions it answers, and allow cancelling an in-flight request.
