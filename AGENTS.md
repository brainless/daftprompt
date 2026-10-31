# Project guidance

## Planning references

Read [docs/PRD.md](docs/PRD.md), [docs/data-model.md](docs/data-model.md), and the relevant task in [epics/](epics/) before implementing changes. Keep implementation and these documents consistent. The project is currently at the first parsing experiment; planned capabilities are not all implemented.

## Tech stack and architecture

- Use Rust and a Cargo workspace.
- `daftprompt-core` is the shared library for all core features: agents, prompts, text parsing, validation, and graph construction. Keep its public interfaces independent of HTTP and GUI details so a future CLI can reuse them.
- `daftprompt-api` uses the shared library and exposes its features through HTTP. The initial endpoint is `POST /parse`. Axum with Tokio is a suggested API stack, not yet a finalized dependency choice.
- `daftprompt-gui` is a native app built with **akar**. It calls the API for parsing; do not duplicate core logic in the GUI. Shared serializable types may be reused. Keep network work off the rendering thread and GUI dependencies out of core.
- Use **Rig** for LLM agents. Give each agent a separate module and system prompt. Start with the parsing agent, provisionally named **Scribe**; add other agents as their features are implemented.
- Use typed extraction contracts with `serde` and `schemars`. For the first parser, use Rig's schema-defined output submission tool to return one complete extraction result. Rust validates it and constructs graph records; the LLM proposes annotations rather than directly mutating the graph.
- **SQLite will store the graph in the future**, following the data model. Do not introduce a separate graph database. The first epic has no storage or chat history: graph fragments and run metadata remain in memory, and each submission is independent.
- A CLI is a future client of the shared library, outside the first epic.

## Text and graph conventions

- Treat text from chat, files, clipboard, and email as eligible input. Source integrations are separate from parsing; the first epic accepts submitted text with optional source metadata.
- Preserve original text and provenance. Payloads hold immutable text; Nodes express relationships; NodeContexts reference payloads or files and optional ranges. Do not copy or paraphrase source text into nodes.
- Validate source spans, Unicode boundaries, label shapes, and relationship targets. Rust owns IDs, hashes, timestamps, and reference resolution. Unlabelled text is valid.
- Follow the documented label vocabulary. Do not invent conflicts, supersession, or staleness from isolated text without the required context or dependency evidence.
- Assignees are optional opaque `@username` or `@team-name` strings on labels. No people directory, team membership management, or membership verification. Only extract an assignee when assignment intent is explicit.

## Dependency and verification workflow

- Inspect local dependency sources before implementing integrations: Rig at `~/Projects/rig`, akar at `~/Projects/akar`, and other available dependencies under `~/Projects/`. Consult installed crate sources or official documentation when needed.
- Check actual manifests, examples, and toolchain requirements instead of assuming an API from memory. Useful starting points are Rig's `examples/extractor/` and `crates/rig-agent/src/extractor.rs`, and akar's `examples/canvas-basic-rust/` and textarea usage in `examples/demo-rust/`.
- Record dependency versions/revisions and reproducible setup commands. Keep provider/model settings configurable and credentials outside source code.
- Once the workspace exists, run formatting, relevant tests, and workspace checks for code changes. Ordinary tests must work without LLM credentials; use stubs for provider interactions and keep live model evaluations opt-in.
- Evaluate extraction quality as well as schema validity: missed/false questions, source-span accuracy, answer links, assignees, invalid outputs, and latency.
