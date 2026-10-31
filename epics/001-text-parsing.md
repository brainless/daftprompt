# Epic 001: Text parsing with Scribe

## Outcome

Paste text into a native akar app, submit it through an API, and inspect the original text as blocks with labels and assignees. This experiment tests whether a small LLM can turn ordinary text into useful graph annotations. **Scribe** is the proposed name of the first agent.

Use a Rust workspace with `daftprompt-core` (shared library), `daftprompt-api` (HTTP service), and `daftprompt-gui` (akar app). All parsing, prompts, validation, and graph construction belong in the shared library so a future CLI can reuse them. Organize agents into separate modules with separate system prompts; implement only Scribe now.

References: [PRD](../docs/PRD.md) and [data model](../docs/data-model.md).

## Scope and decisions

- Accept text regardless of origin: chat, file contents, clipboard, or email. This epic accepts submitted text and optional source metadata; source connectors, file watching, and email fetching are later work.
- No chat history, database, or persistence. Each submission produces an independent, in-memory graph fragment and replaces the previous result.
- **Output contract:** use a typed extraction schema with Rig and `serde`/`schemars`. The local Rig extractor uses a schema-defined `submit` output tool. Use that single result submission, rather than separate tools that mutate nodes or labels. No filesystem, search, or other action tools are needed. Native JSON-schema output can be evaluated later if the selected provider requires it.
- The model proposes source spans, labels, explicit assignee handles, and relationships between identified spans. Rust validates these and creates Payload, Node, NodeContext, Label, and relevant run metadata. Rust owns IDs, hashes, timestamps, and reference resolution; the model does not generate database records directly.
- Preserve the complete original text. Unlabelled content is valid. Derive displayed blocks from source ranges, never from model paraphrases. Treat submitted instructions as text to classify, not commands to execute.
- Start with `question` and locally identifiable `answer` relationships. Extract only explicit assignment intent involving `@username` or `@team-name`; a mention alone is not an assignment. Do not infer `stale`, `conflict`, or `supersedes` from isolated text. Clarification branch creation and linking to previous submissions are outside this epic.

## Tasks

### 1. Establish workspace and contracts

- Create the three crates with the dependency direction GUI → HTTP API → shared library. GUI may reuse shared serializable types but must invoke parsing through HTTP.
- Define request, extraction, graph-result, and error types. Represent optional source kind/reference separately from text; pasted file contents remain a captured payload, not a claim to a verified file on disk.
- Align in-memory graph types with the data model. Document the minimal model clarification needed for arbitrary source text: payloads can capture non-chat text, and a standalone import can be a tree root. Preserve source authorship separately from the parsing agent's annotation provenance; do not invent an author.
- Define exact span coordinates, including Unicode and exclusive ends, and convert validated spans to the model's 1-based line/column ranges. Handle repeated text without ambiguous anchoring.
- Inspect dependency manifests and toolchains before choosing versions; record dependency revisions and reproducible build commands. Keep GUI dependencies out of core.

### 2. Implement Scribe in the shared library

- Give Scribe its own prompt file and Rig configuration. Keep the public async parsing interface independent of HTTP and GUI details; make model/provider settings configurable and credentials external to source code.
- Prompt for meaningful spans, actionable questions, supported answer links, and explicit assignees. Include examples of unlabelled text, rhetorical questions, and quoted questions that are already answered. Allow an empty annotation list.
- Validate schema, source bounds, exact text correspondence, label shapes/statuses, assignees, and relationship targets before constructing the graph. Preserve text once in a payload and reference its ranges from child nodes.
- Bound retries and input size; return explicit errors for invalid output, oversized input, timeouts, and provider failures. Never silently truncate input or return a partly invalid graph.

### 3. Expose the parser through the API

- Add a stateless `POST /parse` endpoint using the shared request/result types. Use a small async HTTP stack, such as Axum with Tokio; inspect available dependency sources/examples before implementation.
- Configure provider credentials in the API process and bind locally by default. Validate requests and map errors to clear HTTP responses. Return the validated graph and enough information for the GUI to render source blocks, labels, and assignees.
- Document how to start the API and make one sample request. No conversation/session endpoints or storage.

### 4. Build the akar GUI

- Implement two columns: a left column with a multiline textarea anchored at the bottom and a Parse button; a wider main column with scrollable parsed text blocks.
- Call the API asynchronously without blocking rendering. Show idle, loading, success, and error states; prevent overlapping submissions for this first version.
- Render original text blocks in source order with label badges and optional assignee handles. Keep unlabelled text visible and show answer targets simply where present. Each submission replaces the displayed result; do not accumulate chat messages.

### 5. Validate the experiment

- Add deterministic tests for graph construction and rejection of invalid spans/references, including Unicode, repeated text, multiline input, and explicit versus incidental mentions. Test API success/failure with a stub parser; ordinary tests must not require an LLM or credentials.
- Create a small, manually annotated fixture set spanning chat, prose, pasted code/documents, email quotations, JSON text, and text with no actionable content. Include locally answered and unanswered questions.
- Provide an opt-in live evaluation using the same core parser. Record model/prompt version, span and label precision/recall, answer-link and assignee correctness, invalid-output rate, and latency. Report failures as well as successes; valid JSON alone is not parsing quality.
- Run workspace checks and demonstrate textarea → API → Scribe → labelled blocks with a configured small model. Document setup and observed quality; do not introduce a separate CLI product for this evaluation.

## Local dependency sources

Check the relevant sources again when implementing each task; do not assume online examples match these checkouts.

- Rig: `~/Projects/rig/examples/extractor/src/main.rs`, `crates/rig-agent/src/extractor.rs`, and the root `Cargo.toml`. The inspected extractor supports typed output, bounded retries, and `append_preamble` for Scribe-specific instructions.
- akar: `~/Projects/akar/examples/canvas-basic-rust/` for app setup; `examples/demo-rust/src/main.rs` for `akar_textarea`; `crates/akar-winit/` for input integration. Inspect the workspace manifest and `rust-toolchain.toml` for compatible graphics dependencies/toolchain.
- Other dependencies: inspect checkouts under `~/Projects/` where available, otherwise installed crate sources and official documentation as needed.

## Open questions

- Which provider and small model should be the first live evaluation target? Keep configuration flexible, but implement and validate one provider first.
- What extraction accuracy and latency are good enough? Use the first fixture evaluation to establish a baseline before setting thresholds.
