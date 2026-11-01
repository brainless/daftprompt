# Epic 001: Text parsing with Scribe

## Outcome

Paste text into a native akar app, submit it through an API, and inspect the original text as blocks with labels and assignees. This experiment tests whether a small LLM can turn ordinary text into useful graph annotations. **Scribe** is the proposed name of the first agent.

Use a Rust workspace with `daftprompt-types` (shared serializable types), `daftprompt-core` (shared library), `daftprompt-api` (HTTP service), and `daftprompt-gui` (akar app). All parsing, prompts, validation, and graph construction belong in the shared library so a future CLI can reuse them. Organize agents into separate modules with separate system prompts; implement only Scribe now.

References: [PRD](../docs/PRD.md) and [data model](../docs/data-model.md). Deferred refinements are tracked in [Epic 002](002-parsing-refinements.md).

## Scope and decisions

- Accept text regardless of origin: chat, file contents, clipboard, or email. This epic accepts submitted text and optional source metadata; source connectors, file watching, and email fetching are later work.
- No chat history, database, or persistence. Each submission produces an independent, in-memory graph fragment and replaces the previous result.
- **Output contract:** use a typed extraction schema with Rig and `serde`/`schemars`. The local Rig extractor uses a schema-defined `submit` output tool. Use that single result submission, rather than separate tools that mutate nodes or labels. No filesystem, search, or other action tools are needed.
- The model proposes source spans, labels, explicit assignee handles, and relationships between identified spans. Rust validates these and creates Payload, Node, NodeContext, Label, and run metadata. Rust owns IDs, hashes, timestamps, label statuses, and reference resolution; the model does not generate database records directly.
- Preserve the complete original text. Unlabelled content is valid. Derive displayed blocks from source ranges, never from model paraphrases. Treat submitted instructions as text to classify, not commands to execute.
- Start with `question` and locally identifiable `answer` relationships. Do not infer `stale`, `conflict`, or `supersedes` from isolated text. Clarification branch creation and linking to previous submissions are outside this epic.

### Span referencing

Small models are unreliable at character or line/column offsets, so the model never produces offsets.

- Rust splits the input into **segments**: lines split on `\n`, numbered from 1, including empty lines. A trailing `\r` belongs to the line terminator, not the segment text. The prompt presents each segment as `[n] text`.
- The model refers to a span by an inclusive segment range (`start_segment`, `end_segment`) and an optional verbatim `quote`.
- Without a quote, the span covers the segment range from the first to the last non-whitespace character.
- With a quote, Rust searches for it in the original text of the segment range, including the original line terminators between segments. The match is exact (no case folding, no Unicode normalization) except that whitespace runs, including line terminators, match one or more whitespace characters, so a quote may write a line wrap or CRLF as a single space; a gap never matches zero characters. The span is always the original bytes, and the match must occur exactly once. Zero matches or more than one match makes the output invalid. This resolves repeated text without ambiguous anchoring.
- Extraction shape (indicative):

  ```text
  spans:       [{ id: "s1", start_segment: 3, end_segment: 3, quote: "Should we cache tokens?" }]
  annotations: [{ kind: "question", span: "s1", assignee: "@infra-team" },
                { kind: "answer",   span: "s2", answers: ["s1"] }]
  ```

  Span IDs are local to one extraction. Rust maps them to generated Node IDs.

### Coordinates

- Internally and in the API result, a span is a byte range `[start, end)` into the payload's UTF-8 text. Both ends must fall on `char` boundaries.
- In NodeContext, ranges use 1-based lines and 1-based columns counted in Unicode scalar values (`char`s). The end position is exclusive: it is the position just after the last character, so a span ending at the end of a 5-character line has `end_col = 6`. For a ranged context all four fields are set; a NULL range means the whole payload or file.
- Lines are counted by `\n`. CRLF text is preserved unchanged.

### Graph construction rules

- **Root node:** the submitted text becomes one payload and one root node (`parent_id IS NULL`, no `run_id`). The root's `author` is the source author from the optional source metadata if one was supplied, otherwise the literal `unknown`. Never derive an author from the text itself.
- **Span nodes:** each validated span becomes a child node of the root with `author = 'agent'`, `run_id` set to the Scribe run, and one NodeContext pointing at a range of the root payload. Text is never copied.
- **Only referenced spans:** every span must be referenced by at least one annotation. An unreferenced span makes the output invalid. All other text stays unlabelled and is rendered from the root payload.
- **No overlaps:** spans must not overlap or nest. Overlapping spans make the output invalid. One span may carry several annotations (for example, a question that also answers an earlier question).
- **Statuses are derived by Rust:** the model never outputs a status. A `question` with at least one validated local `answer` link is `resolved`; otherwise it is `open`. `answer` labels have NULL status.
- **Assignees:** an assignee attaches only to a `question` label, and only when the text explicitly assigns the question to `@username` or `@team-name`. A mention alone is not an assignment. Assignment intent attached to anything other than a question is not extracted in this epic. Handles must match the data model's rule: `@` followed by a non-empty name without whitespace.
- **Run metadata:** each parse produces an in-memory `run` (agent `scribe`, model and provider ID, prompt version, start/end times, status, attempt count), one `run_input` row for the root node, and one `tool_call` row for the accepted `submit` call with its arguments as the JSON input. Labels have `created_by = 'agent'` and the run's `run_id`.

### Limits and failures

- Input is limited to a configurable maximum, by default 32 KiB of UTF-8 and 1,000 segments. Oversized input is rejected before any model call; never silently truncate it.
- One configurable attempt budget (default: 3 attempts in total) covers deserialization failures and validation failures only. Provider errors and per-attempt timeouts fail fast: they are returned immediately without retrying, so worst-case latency is one timeout. Rig's `TypedRun::retries` only retries runs that error, return nothing, or return output that does not parse, so validation failures are retried by Scribe's own loop around the extractor. Each retry is a fresh attempt without corrective feedback.
- When the budget runs out (or a provider error or timeout occurs), return an explicit error (invalid output, oversized input, timeout, provider failure). Never return a partly invalid graph.

## Tasks

### 1. Establish workspace and contracts

- Create four crates. Compile-time dependencies: `daftprompt-types` depends only on `serde`/`schemars`; `daftprompt-core` depends on `daftprompt-types` and Rig; `daftprompt-api` depends on `daftprompt-core` and `daftprompt-types`; `daftprompt-gui` depends on `daftprompt-types`, akar, and an HTTP client. The GUI never depends on `daftprompt-core` or `daftprompt-api`, and calls parsing only over HTTP.
- Define request, extraction, graph-result, and error types in `daftprompt-types`. Represent optional source kind, reference, and author separately from text; pasted file contents remain a captured payload, not a claim to a verified file on disk.
- Align in-memory graph types with the data model. Update [data-model.md](../docs/data-model.md) with the decisions above: payloads may capture non-chat text, a standalone import can be a tree root, the root `author` is the source author or `unknown`, column units and exclusive end positions, and Rust-derived question status.
- Implement segmentation, quote anchoring, and conversion from byte ranges to 1-based line/column ranges in core.
- akar requires a nightly toolchain with `rustc-codegen-cranelift` (see akar's `rust-toolchain.toml`). Add a root `rust-toolchain.toml` pinned to a dated nightly with that component, verified to build akar, Rig, and the rest of the workspace. Record the date.
- Inspect dependency manifests before choosing versions; record dependency revisions and reproducible build commands. Keep GUI dependencies out of core and types.
- Dependencies not published on crates.io (Rig, akar) use their upstream Git URL pinned to an explicit revision. crates.io dependencies are declared with versions in the workspace `Cargo.toml`, pinned exactly by the committed `Cargo.lock`, and their resolved versions are recorded in `docs/development.md`. Local checkouts are reference-only, never external path dependencies or path patches; relative paths are reserved for this project's own workspace crates.

### 2. Implement Scribe in the shared library

- Give Scribe its own prompt file (with a version identifier) and Rig configuration. Keep the public async parsing interface independent of HTTP and GUI details; make model/provider settings configurable and credentials external to source code.
- Prompt for actionable questions, supported answer links, and explicit assignees, using segment references and quotes as defined above. Include examples of unlabelled text, rhetorical questions, quoted questions that are already answered, and a mention that is not an assignment. Allow an empty annotation list.
- Validate schema, segment bounds, quote anchoring, overlap, unreferenced spans, label shapes, assignees, and relationship targets before constructing the graph. Preserve text once in a payload and reference its ranges from child nodes.
- Implement the attempt budget, input limits, timeouts, and explicit errors defined above.
- Add deterministic tests for segmentation, quote anchoring, coordinate conversion, graph construction, and rejection of invalid output: Unicode (multi-byte and combining characters), CRLF, repeated text, multiline spans, overlapping and unreferenced spans, bad references, and explicit versus incidental mentions. Ordinary tests must not require an LLM or credentials; use a stub extractor.

### 3. Build fixtures and evaluate Scribe

Do this before the API and GUI work, so prompt tuning is measured.

- Create a small, manually annotated fixture set spanning chat, prose, pasted code/documents, email quotations, JSON text, and text with no actionable content. Include locally answered and unanswered questions, and explicit and incidental mentions.
- Provide an opt-in live evaluation using the same core parser. Record model/provider, prompt version, span and label precision/recall, answer-link and assignee correctness, invalid-output rate (with failure reasons), attempts per parse, and latency.
- Matching rules: a predicted span matches an expected span when their byte ranges are equal after trimming surrounding whitespace; also report a lenient score where overlap / union ≥ 0.5. A label is correct when its kind matches on a matched span. An answer link is correct when both ends match. An assignee is correct when the handle is identical on a matched question.
- Report failures as well as successes; valid JSON alone is not parsing quality. Record the baseline result in the repository.

### 4. Expose the parser through the API

- Add a stateless `POST /parse` endpoint using the shared request/result types. Use a small async HTTP stack, such as Axum with Tokio; inspect available dependency sources/examples before implementation.
- Configure provider credentials in the API process and bind locally by default. Validate requests and map errors to clear HTTP responses (for example, 413 for oversized input, 422 for invalid model output after the attempt budget, 502 for provider failure, 504 for timeout).
- Return the validated graph, the payload text, and byte ranges for each span node so the GUI can render blocks, labels, assignees, and answer targets without recomputing anything.
- Test success and each failure mapping with a stub parser. Document how to start the API and make one sample request. No conversation/session endpoints or storage.

### 5. Build the akar GUI and demonstrate end to end

- Implement two columns: a left column with a multiline textarea anchored at the bottom and a Parse button; a wider main column with scrollable parsed text blocks.
- Run the HTTP request on a background thread (or a Tokio runtime owned by the GUI) and send the result back over a channel. akar-winit schedules work through wake deadlines, so while a request is loading, poll the channel on a short wake deadline. Never block rendering.
- Show idle, loading, success, and error states; prevent overlapping submissions for this first version.
- Render the original text in source order: span blocks with label badges, statuses, and optional assignee handles, and unlabelled text between them. Show answer targets simply where present. Each submission replaces the displayed result; do not accumulate chat messages.
- Run workspace checks and demonstrate textarea → API → Scribe → labelled blocks with a configured small model. Document setup and observed quality; do not introduce a separate CLI product for this evaluation.

## Local dependency sources

Check the relevant sources again when implementing each task; do not assume online examples match these checkouts.

Dependency Git URLs: Rig at `https://github.com/0xPlaygrounds/rig.git` and akar at `https://github.com/brainless/akar.git`. Other dependencies come from crates.io; record their resolved versions in `docs/development.md`. The local paths below are only for inspecting source and examples.

- Rig: `~/Projects/rig/examples/extractor/src/main.rs`, `crates/rig-agent/src/extractor.rs`, and the root `Cargo.toml`. The inspected extractor supports typed output through the `submit` tool, retries for errored/empty/unparseable runs (`crates/rig-agent/src/agent/typed.rs`), and `append_preamble` for Scribe-specific instructions.
- akar: `~/Projects/akar/examples/canvas-basic-rust/` for app setup; `examples/demo-rust/src/main.rs` for `akar_textarea`; `crates/akar-winit/` for input integration and wake deadlines (`src/activation.rs`). Inspect the workspace manifest and `rust-toolchain.toml` (nightly with `rustc-codegen-cranelift`) for compatible graphics dependencies and toolchain.
- Other dependencies: inspect checkouts under `~/Projects/` where available, otherwise installed crate sources and official documentation as needed.

## Open questions

- Which provider and small model should be the first live evaluation target? Keep configuration flexible, but implement and validate one provider first.
- What extraction accuracy and latency are good enough? Use the first fixture evaluation to establish a baseline before setting thresholds.
