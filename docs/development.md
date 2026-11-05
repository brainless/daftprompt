# Development

## Toolchain

akar requires a nightly toolchain with the `rustc-codegen-cranelift` component (see `~/Projects/akar/rust-toolchain.toml`). The workspace pins one in `rust-toolchain.toml`:

| field | value |
| --- | --- |
| channel | `nightly-2026-10-25` |
| actual rustc | `1.101.0-nightly (12433c307 2026-10-24)` |
| components | `rustc-codegen-cranelift`, `rustfmt`, `clippy` |

The date was chosen on 2026-10-31, after checking that the components exist for that nightly and that the nightly builds Rig, akar, and this workspace. Rig 0.43.0 declares `rust-version = "1.95.0"`; the nightly is far above that floor.

Set up and verify from a fresh checkout:

```sh
rustup toolchain install nightly-2026-10-25 \
  --component rustc-codegen-cranelift \
  --component rustfmt \
  --component clippy
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
```

The pinned toolchain provides the Cranelift backend but this workspace does not enable it globally. akar's examples opt in through a local `.cargo/config.toml`:

```toml
[unstable]
codegen-backend = true

[profile.dev]
codegen-backend = "cranelift"
```

That config is intentionally not part of this workspace: LLVM is the default build backend, which keeps Rig and the test binaries on their best-tested path. The component is required so akar's nightly-only setup stays reproducible.

Verification on 2026-10-31 with `nightly-2026-10-25`: `cargo check --workspace` builds `daftprompt-types`, `daftprompt-core` with Rig 0.43.0, `daftprompt-api` with Axum 0.8, Tokio 1, and Rig's `reqwest` and `rustls` provider transport, and `daftprompt-gui` with the four akar crates, wgpu 29, winit 0.30, pollster 0.4, and `ureq` 3. `cargo test --workspace` passes without LLM credentials, including the GUI's view, client, and layout tests. `cargo clippy --workspace --all-targets -- -D warnings` is clean.

## Dependencies

Dependencies not published on crates.io (Rig, akar) are referenced by their upstream Git URL and pinned to an explicit revision. Everything else comes from crates.io, declared with versions in the workspace `Cargo.toml` and pinned exactly by the committed `Cargo.lock`; the resolved versions are recorded below. Rig and akar themselves depend on crates.io `serde`, `schemars`, and `wgpu`, so Git copies of those would be distinct crates and break the build. No external dependency uses a local path or a `[patch]`; local checkouts are reference-only. Relative paths appear only between this workspace's own crates.

| dependency | source | pinned revision | date |
| --- | --- | --- | --- |
| Rig (`rig` 0.43.0 facade, `rig-agent`) | <https://github.com/0xPlaygrounds/rig.git> | `7dfd8a4229a9d89af159489687cb14b404b2871d` | 2026-10-06 |
| akar (`akar-core`, `akar-layout`, `akar-components`, `akar-winit` 0.1.0) | <https://github.com/brainless/akar.git> | `1d61c6f10d952bd3cdb395f25cf22b7246e921d0` | 2026-11-04 |

Rig is declared with `default-features = false` plus the `agent` feature: the parser uses the typed `submit`-tool extractor (`rig::extractor`). The optional `live-eval` feature in core adds `reqwest` and `rustls` for the evaluation binary, and `daftprompt-api` enables the same transport in its own dependency so it can build the provider client from environment credentials. Core's own manifest and the GUI never enable a provider transport. Scribe takes any Rig completion model, so the provider client and its transport are chosen where the model is built (the API or the opt-in live evaluation).

Crates.io dependencies are declared once in the workspace `Cargo.toml`. Resolved versions from `Cargo.lock` (2026-10-31):

- `serde` 1.0.229 (derive), `serde_json` 1.0.154 — used by `daftprompt-types` and by `daftprompt-core` for fixtures and reports; `schemars` 1.2.3 — used by `daftprompt-types`.
- `pulldown-cmark` 0.13.6 (default features disabled; Rust floor 1.71.1) — core Markdown structure via `Parser::new_ext` with tables and `into_offset_iter`; manifest, `Tag`/`Event`, and offset iterator source inspected before integration. Fetch reproducibly with `cargo fetch --locked`; no HTML renderer is enabled.
- `thiserror` 2.0.21 — shared library errors.
- `sha2` 0.10.9 — SHA-256 payload hashes.
- `axum` 0.8.9, `tokio` 1.54.1 — HTTP service. Core uses only Tokio's timer for per-attempt timeouts. The API binary enables Tokio's `net` and `signal` features for the listener and Ctrl+C shutdown, and `http-body-util` 0.1.5 to distinguish an oversized request body; `tower` 0.5.3 (`util`) is a test-only dependency for driving the router.
- `rig` 0.43.0 with `reqwest` and `rustls` — enabled only by `daftprompt-api`, which builds the provider client from environment credentials. Core keeps Rig's `agent` feature only, so the shared library stays transport-free.
- `dotenvy` 0.15.7 — loads a local `.env` at startup of `daftprompt-api` and, as an optional dependency of the `live-eval` feature, of `evaluate-scribe`. Core's normal build does not include it.
- `ignore` 0.4.34, `globset` 0.4.21 — core's read-only project tools (`daftprompt_core::project_tools`): `ignore::WalkBuilder` for `.gitignore`-aware, hidden-skipping, sorted traversal and `ignore::gitignore::Gitignore` for single-path checks (sources inspected before choosing); `globset` for the `glob` filters. `tempfile` 3.27.0 is a dev-dependency of core for filesystem tests.
- `ureq` 3.5.0 — GUI HTTP client.
- `wgpu` 29.0.4, `winit` 0.30.13, `pollster` 0.4.0 — GUI host integration. These are the versions akar builds against; akar does not re-export its surface or event-loop types, so the app declares them directly. They stay out of core, types, and the API.
- `glyphon` 0.11.0 — same text shaping version as akar's pipeline. The GUI uses it only to give the scrollable block text an explicit viewport clip; app chrome still uses akar components.
- `arboard` 3.7.0 (default features off: text only, no image stack) — GUI system clipboard for textarea copy and paste. Also pulls `clipboard-win` 5.4.3 and `error-code` 3.4.0 on Windows, and `objc2-core-graphics`/`objc2-io-surface` 0.3.2 on macOS. Used only by `daftprompt-gui`.

### Bundled GUI fonts

The GUI bundles static TTFs (SIL OFL 1.1) in `crates/daftprompt-gui/assets/fonts/` and loads them with `include_bytes!` and `TextPipeline::load_font_bytes`. License texts sit beside them (`Inter-OFL.txt`, `SourceSerif4-OFL.md`). Only the weights the GUI draws are included. They were downloaded from the upstream release archives and extracted without running anything from them.

| file | font and version | source | bytes | SHA-256 |
| --- | --- | --- | --- | --- |
| `Inter-Regular.ttf` | Inter 4.1 | <https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip> (`extras/ttf/`) | 411640 | `40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82` |
| `Inter-Medium.ttf` | Inter 4.1 | same archive | 417300 | `97ad806f526e41546d46365bb3a393145f75b7b1568913db74549ad8b8dba872` |
| `Inter-SemiBold.ttf` | Inter 4.1 | same archive | 419744 | `78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3` |
| `SourceSerif4-Regular.ttf` | Source Serif 4.005 | <https://github.com/adobe-fonts/source-serif/releases/download/4.005R/source-serif-4.005_Desktop.zip> (`TTF/`) | 261868 | `e5a4ee6a3d87bb9024796be390c6771e2a0eb1883dae25effaf57ca01668e24b` |

Archive SHA-256: `Inter-4.1.zip` `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`; `source-serif-4.005_Desktop.zip` `549fdb8f9a682bd06944298621404969f6de77c2e422ff3b8244a1dcd6a0c425`.

Local checkouts used for source and example reference only:

- Rig: `~/Projects/rig` (extractor example, `crates/rig-agent/src/extractor.rs`, `crates/rig-agent/src/agent/typed.rs`).
- akar: `~/Projects/akar` (`examples/canvas-basic-rust`, `examples/demo-rust`, `crates/akar-winit`).

First build needs network access to fetch both Git repositories; afterwards they are cached under `~/.cargo/git`. `Cargo.lock` is committed so repeat builds resolve identically.

## Workspace layout

| crate | depends on | purpose |
| --- | --- | --- |
| `daftprompt-types` | `serde`, `schemars` | Serializable request, extraction, graph-result, and error contracts. No HTTP, GUI, or core dependency. |
| `daftprompt-core` | `daftprompt-types`, Rig, `serde`, `sha2`, Tokio (timer) | Shared library: text primitives, validation, graph construction, agents, and evaluation fixtures/scoring. |
| `daftprompt-api` | `daftprompt-core`, `daftprompt-types`, Axum, Tokio, Rig (provider transport) | Stateless HTTP service: `POST /parse`, implemented in task 4 and documented in [api.md](api.md). |
| `daftprompt-gui` | `daftprompt-types`, akar, `ureq`, `wgpu`/`winit`/`pollster`, `glyphon` | Native akar app: two-column parse view. Calls the API over HTTP; never links core or api. |

The GUI is implemented in [gui.md](gui.md). It talks to the API only, keeps the
HTTP call on a worker thread, and renders the API's payload and span ranges
without duplicating parsing or graph construction. The API serves the shared
Scribe parser over HTTP.

## Scribe

Scribe lives in `daftprompt-core::scribe`. Its system prompt is versioned in
`crates/daftprompt-core/prompts/scribe-v4.md` plus exactly one compile-time format
section (`scribe-format-plain-v1.md` or `scribe-format-markdown-v1.md`). Each run
records `scribe-v4+plain-v1` or `scribe-v4+markdown-v1`. Rust detects the format
when undeclared and builds source-offset blocks; Scribe sees inline unnumbered
markers and may select whole blocks or smaller segment ranges.

- `Scribe::parse(ParseRequest)` is the transport-independent entry point. The API can
  also hold an `Arc<dyn scribe::Parser>`.
- `ScribeConfig` sets the provider and model identifiers recorded on the run, the input
  limits (default 32 KiB and 1,000 segments), the total attempt budget (default 3), and
  the per-attempt timeout (default 60 seconds).
- `Scribe::from_rig_model` accepts any Rig completion model. The caller constructs the
  provider client and reads its credentials from the environment (for example, the
  provider client's `from_env` constructor); core never stores or reads credentials.
- Input limits are checked before the first model call. Invalid output (deserialization
  or validation failures) is retried from scratch without feedback until the budget runs
  out, then returned as an explicit `ParseError`. Provider errors and per-attempt
  timeouts fail fast with `ParseError::Provider` / `ParseError::Timeout`; they do not
  consume further attempts, so worst-case latency is one timeout.
- Sibling consistency follow-up (Epic 005): after an extraction validates, Rust groups the
  `item` and `section` children of each parent (top-level blocks share a virtual root) by
  kind. If a group has at least two blocks, one is exactly covered by an annotated span,
  and a sibling has no span overlapping it or its descendants, Scribe makes ONE extra
  model call with the same input plus corrective feedback (`ModelInput::feedback`, which
  `RigExtractor` appends to the user prompt) naming the labelled block, its kind, and the
  untouched sibling IDs. The call is outside the attempt budget, uses the per-call
  timeout, and is controlled by `ScribeConfig::consistency_followup` (default on; disable
  with `with_consistency_followup(false)`). The result is used only if it validates;
  errors, timeouts, and invalid output keep the original extraction, so the follow-up
  never fails a parse. Rust never adds labels itself. `Run` records
  `consistency_followup` (a call ran) and `consistency_used` (its result replaced the
  original); `attempts` does not count the follow-up. The live evaluation uses the default
  config, so it includes the follow-up and costs at most one extra call per fixture.
- Tests use the public `ExtractionModel` seam with a stub. No LLM credentials are needed
  for `cargo test --workspace`; live evaluation is opt-in (see below).

## Explainer

`daftprompt_core::explainer` is the clarification agent of [Epic 004](../epics/004-clarification-branch.md). Public surface: `Explainer<M: ExplainerModel>` (`new(config, model)`, `with_project(Arc<dyn ProjectFiles>)`, async `clarify(ClarifyRequest) -> Result<ClarifyResult, ParseError>`, `grounding()`), the object-safe `Clarifier` trait, `ExplainerConfig`, `ExplainerModel`/`ExplainerInput` (the model seam), and `RigExplainer`/`Explainer::from_rig_model`.

- **Configuration:** `ExplainerConfig::default()` is provider `neuralwatt`, model `glm-5.3-flash`, independent of Scribe. `ExplainerConfig::from_env()` applies `DAFTPROMPT_EXPLAINER_PROVIDER` and `DAFTPROMPT_EXPLAINER_MODEL` (blank values ignored); it reads no credentials. The caller builds the provider client (for Neuralwatt or Cerebras, the Chat Completions wire from OpenAI-compatible config with the selected provider's `*_API_KEY` and optional `*_BASE_URL`, as the API binary does for Scribe) and passes the model to `Explainer::from_rig_model`. Limits and defaults: question 2 KiB, 8 earlier turns, 16 KiB of earlier turns, 20 segments on each side of the subject, source limits as for parsing, 12 model calls per attempt, 20 citations, 3 attempts, 120 s per attempt covering the whole tool loop, plus `ProjectToolConfig` caps.
- **Rig usage (rev `7dfd8a42`):** Rig's `Extractor` is single-turn, so `RigExplainer` builds an `AgentBuilder` with `.output_schema::<ExplainerExtraction>()` and `.output_mode(OutputMode::Tool)`, the project tools registered through `rig_tools`, and runs `agent.prompt(..).max_turns(n).await`. `AgentRunner::output_tool` and the extractor's `submit` name are crate-private in `rig-agent`, so the synthetic output tool has Rig's default name `final_result` (the prompt says so); Explainer records it as the `submit` tool call. The run ends when the model calls it; `PromptResponse::output_tool_calls()` must be at least 1 and `output()` is the serialized arguments. `MaxTurns` and `UnknownToolCall` count as invalid output; other `PromptError`s are provider errors.
- **Tool-call records:** every project tool invocation is read from the per-attempt `ToolRecorder` (no hooks are needed), so the records cover exactly the attempt that succeeded; `submit` is appended. Outputs are stored as content-addressed payloads in `tool_payloads`.
- **Tests:** `tests/explainer.rs` uses a stub `ExplainerModel` that drives the attempt's `ProjectToolbox` over an `InMemoryProject` and then returns an extraction; it needs no credentials. `tests/explainer_live.rs` is an opt-in (`--features live-eval -- --ignored`) check of multi-turn tool calling with the output tool against the configured model. It has not been run in this environment.

## Scribe evaluation

The fixture set and scoring live in `daftprompt_core::eval`; the data is in
`crates/daftprompt-core/fixtures/scribe/`. There is one JSON file per text, and the
fixture set covers chat, prose, pasted code, a pasted document, email quotations, JSON
text, and text with no actionable content. It includes locally answered and unanswered
questions plus explicit and incidental `@` mentions. Each file records:

- the complete text, optional source metadata, declared `content_type`, and an
  independently annotated `expected_detection` (`plain` or `markdown`);
- expected spans as half-open byte ranges plus a verbatim `quote` and unary `question`,
  `recommendation`, and `task` labels (questions/tasks may carry an explicit `assignee`);
- expected `answer`, `recommendation_links`, and symmetric `alternative_links` between spans by local span id.

`load_fixtures` validates the format before scoring: quotes must equal their byte range,
ranges must be trimmed and on `char` boundaries, spans must not overlap, handle shapes
must be valid, answer and recommendation links must target expected questions, and alternatives must join recommendations. A malformed fixture
fails the ordinary test suite.

Scores are reported for spans, labels, answer links, unary recommendations, tasks, recommendation-question links, symmetric alternative pairs, and assignees, each under two span
matching modes:

- **exact** — byte ranges are equal after trimming surrounding whitespace;
- **lenient** — overlap divided by union is at least `0.5`.

A label is correct when its kind matches on a matched span, an answer link when both
ends match, and an assignee when both label kind and handle match on a matched question or task. Matching
is one-to-one and greedy, preferring exact pairs and then higher overlap. Reports also
list every missed, unexpected, approximate, or mismatched item, plus parse failures with
their explicit error kind and reason, attempts per parse, and latency.

### Running the live evaluation

The `evaluate-scribe` binary is built only with the `live-eval` feature and calls a model
only when `DAFTPROMPT_EVAL_LIVE` opts in. Credentials are read by the provider client
from the environment; core never stores them.

```sh
OPENAI_API_KEY=... DAFTPROMPT_EVAL_LIVE=1 \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --output docs/evaluations/scribe-<provider>-<model>.md
```

| variable | default | meaning |
| --- | --- | --- |
| `DAFTPROMPT_EVAL_LIVE` | unset | `1`, `true`, or `yes` enables model calls |
| `DAFTPROMPT_EVAL_PROVIDER` | `openai` | `openai` (`OPENAI_BASE_URL` selects a compatible endpoint), `neuralwatt`, `cerebras`, or `groq` |
| `DAFTPROMPT_EVAL_MODEL` | `gpt-4o-mini` (`qwen3.6-35b-fast` for `neuralwatt`, `gpt-oss-120b` for `cerebras`, `openai/gpt-oss-20b` for `groq`) | Model identifier recorded on every run |
| `DAFTPROMPT_EVAL_PROMPT_VERSION` | `scribe-v4` | `scribe-v4` or original `scribe-v3` prompt, rendering and schema for paired comparisons |
| `DAFTPROMPT_EVAL_MAX_ATTEMPTS` | `3` | Total attempt budget |
| `DAFTPROMPT_EVAL_TIMEOUT_SECS` | `60` | Per-attempt timeout |
| `OPENAI_API_KEY` | required for live `openai` runs | Read by `OpenAI::from_env()` |
| `NEURALWATT_API_KEY` | required for live `neuralwatt` runs | Bearer key for Neuralwatt |
| `NEURALWATT_BASE_URL` | `https://api.neuralwatt.com/v1` | Optional override |
| `CEREBRAS_API_KEY` | required for live `cerebras` runs | Bearer key for Cerebras |
| `CEREBRAS_BASE_URL` | `https://api.cerebras.ai/v1` | Optional override |
| `GROQ_API_KEY` | required for `groq` | Bearer key for Groq |
| `GROQ_BASE_URL` | `https://api.groq.com/openai/v1` | Optional override |

The binary loads a `.env` file from the working directory (or a parent) first, using
`dotenvy::dotenv()` semantics: a missing file is fine, variables already in the
environment win, and a malformed file stops the run with exit code `2`. Copy
[`.env.example`](../.env.example) to `.env` (git-ignored) for local keys.

Other flags are `--fixtures DIR` and `--limit N`. With no `DAFTPROMPT_EVAL_LIVE`, the
binary writes a baseline report with the fixture inventory, matching rules, and the
command to reproduce, but no scores. A live run exits `1` if any fixture failed to parse
and `2` if the run could not start (for example missing credentials).

The committed [baseline report](evaluations/scribe-baseline.md) records that the live run
was **not performed** on 2026-10-31 because no provider credentials were available; it
contains no model results. A live report should be saved under a provider/model name so
the baseline remains comparable.

Deterministic tests cover fixture validation, every scoring rule with fake outcomes, and
a stub-driven pass over every fixture: returning the annotated extraction through the
normal Scribe pipeline must score exactly, without network access.

## Explainer grounding evaluation

Cases live in `crates/daftprompt-core/fixtures/explainer/cases/` (one JSON file each) and the
small project the agent explores lives in `crates/daftprompt-core/fixtures/explainer/project/`
(no hidden or git-ignored files, so `FsProject` sees all of it). The cases cover a term defined
in a project file, a term defined only in the source text, a term defined nowhere ("not found",
no fabricated citations), an ambiguous term defined differently in two files, a definition in
a file whose comment tries to inject instructions (the case's `injection_marker` must never
appear in the output or in a tool call), and a follow-up turn. Each case holds the text, the
target byte range with a verbatim quote, the question, optional earlier turns, and the expected
files with inclusive line ranges, source quotes, `not_found`, and the marker. Rust computes the
payload hash and node IDs when the request is built. `load_clarify_fixtures` validates spans,
quotes, expected files and line ranges against the project, and the marker against the project
text; the deterministic tests in `crates/daftprompt-core/tests/explainer_eval.rs` cover this
and every metric with synthetic results, without credentials.

The `evaluate-explainer` binary is built only with `live-eval` and calls a model only when
`DAFTPROMPT_EVAL_LIVE=1` is set and the selected provider's API key is in the process environment. Unlike
`evaluate-scribe` it never reads a `.env` file.

```sh
NEURALWATT_API_KEY=... DAFTPROMPT_EVAL_LIVE=1 \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-explainer -- \
  --output docs/evaluations/explainer-neuralwatt-glm-5.3-flash.md
```

Provider and model come from `DAFTPROMPT_EXPLAINER_PROVIDER` / `DAFTPROMPT_EXPLAINER_MODEL`
(default `neuralwatt` / `glm-5.3-flash`; `openai`, `cerebras`, and `groq` are also supported), `DAFTPROMPT_EVAL_MAX_ATTEMPTS` and
`DAFTPROMPT_EVAL_TIMEOUT_SECS` override the attempt budget and per-attempt timeout, and
the selected provider's `*_BASE_URL` overrides the endpoint. Flags: `--fixtures DIR`, `--project DIR`,
`--output FILE`, `--limit N`. Exit codes: `0` success, `1` some case failed, `2` the run could
not start. The report gives invalid-output rate, attempts, project tool calls per run, latency,
file-citation recall and precision (exact and lenient line overlap), source-citation overlap,
not-found correctness (phrase heuristic), injection resistance, every explanation with its
citations, and a manual-review section on whether citations support the explanation. Without
credentials, [`evaluations/explainer-baseline.md`](evaluations/explainer-baseline.md) records the
evaluation as not run.

## HTTP API

`daftprompt-api` serves the stateless `POST /parse` endpoint with Axum and
Tokio. The process builds the provider client from the environment and binds to
`127.0.0.1:8080` by default:

```sh
OPENAI_API_KEY=... cargo run -p daftprompt-api
```

The API (and the evaluation binary) load `.env` first; see
[`.env.example`](../.env.example). `DAFTPROMPT_API_PROVIDER=neuralwatt` with
`NEURALWATT_API_KEY` selects Neuralwatt's OpenAI-compatible Chat Completions
endpoint. `DAFTPROMPT_API_PROVIDER=cerebras` with `CEREBRAS_API_KEY` selects
Cerebras Chat Completions; `gpt-oss-120b` is the model default for that provider.
Set `DAFTPROMPT_EXPLAINER_PROVIDER=cerebras` and
`DAFTPROMPT_EXPLAINER_MODEL=gpt-oss-120b` to use it for clarification as well.
The full environment table, a curl example, the result shape, and the error
mapping are in [api.md](api.md). The API maps explicit `ParseError` variants to
400, 413, 422, 502, 504, or 500 and never returns a partly valid graph. Tests
drive the router with a stub `Parser`, so they need no credentials.

## Native GUI

`daftprompt-gui` is the two-column akar app. The left column holds a multiline
textarea anchored at the bottom and a Parse button; the right column renders the
original text in source order as scrollable blocks with label badges, statuses,
assignees, API display IDs, and answer/recommendation/alternative links. Each submission replaces the previous result.

```sh
# Build the API binary, then run the app; it starts the API itself.
cargo build -p daftprompt-api
cargo run -p daftprompt-gui
```

The GUI reads `DAFTPROMPT_API_URL` (default `http://127.0.0.1:8080/parse`) and
`DAFTPROMPT_GUI_TIMEOUT_SECS` (default 300). With the default URL it also starts the built `daftprompt-api` binary as a child process (see [gui.md](gui.md)). Parsing runs on a background thread
and the result returns over a channel; while loading, the event loop polls on a
50 ms wake deadline and never blocks rendering. Setup, states, source-order
rendering, and the live-run status are documented in [gui.md](gui.md). A live
model run was not performed on 2026-10-31 because credentials were unavailable.

## Workspace checks

```sh
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The opt-in evaluation binary is not part of the default workspace build. Check it
explicitly when changing it:

```sh
cargo check -p daftprompt-core --features live-eval --bins
cargo clippy -p daftprompt-core --features live-eval --bins -- -D warnings
```

Ordinary tests must not need LLM credentials. Live model evaluations are opt-in and are documented with the parsing task.

## Epic 002 verification

The parser uses Scribe v3. Deterministic tests cover recommendation/task fields, target
kinds, self and duplicate links (including reversed alternatives), many-to-many links,
source-order contiguous display IDs, multiple labels sharing a number, standalone
recommendations, explicit alternative sets, and complementary plans. The fixture set
contains 19 texts; relationship labels do not increase the recommendation entity count.

The [Scribe v3 Neuralwatt report](evaluations/scribe-v3-neuralwatt-qwen3.6-35b-fast.md)
records the opt-in live run with `qwen3.6-35b-fast`. The 11 original fixtures retain all
question and answer expectations. `chat-local-answer` and `email-crlf-answer` additionally
expect a task on their explicit action-request answer spans; these are independent labels
justified by the expanded contract. Eight new fixtures exercise advice/decision boundaries,
standalone alternatives, complementary plans, requests and commitments, completed work,
incidental mentions, and many-to-many recommendations. Historical baseline reports have
no live v2 scores, so no quantitative v2/v3 model comparison is claimed.

## Structure-aware parsing and paired evaluation (Epic 005)

The suite has 23 fixtures: the original 19 plus the complete Epic 003 text, an
LLM-style Markdown plan with a question list, a nested-list plan, and a plain
multi-paragraph proposal. The Epic 003 golden annotations select each refinement
section and each Good to have bullet, leaving grouping headings unlabelled.
Ordinary tests check every expected detection, verify all four new fixtures'
golden spans match whole Rust blocks, and score stub extractions without model
credentials. Reports separate detection accuracy from extraction quality and
report all existing exact/lenient metrics, invalid-output rates, attempts, and
latency for each declared content type.

To compare the same fixtures and model, run these commands sequentially. The
binary loads credentials from `.env`; no keys belong in reports. The v3 mode
uses the original v3 prompt, segment-only input and original segment-only schema;
`ScribeConfig.legacy_v3` is an evaluation compatibility setting used with that
extractor. Production parsing defaults to v4.

```sh
DAFTPROMPT_EVAL_LIVE=1 DAFTPROMPT_EVAL_PROVIDER=neuralwatt \
DAFTPROMPT_EVAL_MODEL=qwen3.6-35b-fast DAFTPROMPT_EVAL_PROMPT_VERSION=scribe-v3 \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --output docs/evaluations/structure-scribe-v3-neuralwatt.md

DAFTPROMPT_EVAL_LIVE=1 DAFTPROMPT_EVAL_PROVIDER=neuralwatt \
DAFTPROMPT_EVAL_MODEL=qwen3.6-35b-fast DAFTPROMPT_EVAL_PROMPT_VERSION=scribe-v4 \
  cargo run -p daftprompt-core --features live-eval --bin evaluate-scribe -- \
  --output docs/evaluations/structure-scribe-v4-neuralwatt.md
```

Default settings for both are three attempts and a 60-second timeout per attempt.
Keep provider, model, fixture directory, and limits identical when comparing.

### Recorded structure-aware results (2026-11-04)

The [v3 baseline](evaluations/structure-scribe-v3-neuralwatt.md) and
[retained v4 result](evaluations/structure-scribe-v4-neuralwatt.md) use the same
23 fixtures, Neuralwatt `qwen3.6-35b-fast`, three-attempt budget and 60-second
per-attempt timeout. Detection is correct on all 23 fixtures in both reports.

| metric | v3 baseline | retained v4 |
| --- | --- | --- |
| Plain exact span precision / recall | 80.0% / 84.2% | 87.2% / 89.5% |
| Plain lenient span precision / recall | 95.0% / 100.0% | 97.4% / 100.0% |
| Plain exact answer-link precision / recall | 66.7% / 66.7% | 66.7% / 66.7% |
| Plain lenient answer-link precision / recall | 100.0% / 100.0% | 100.0% / 100.0% |
| Epic 003 exact recommendation units | invalid output after 3 attempts | 8 / 11 |
| Nested-list plan exact spans | 0 / 3 | 3 / 3 |
| Markdown plan + questions exact spans | 2 / 4 | 4 / 4 |

The quality acceptance is **partial**. The retained prompt preserves/improves
aggregate plain-text span metrics, resolves the section/list boundaries it selects,
and passes deterministic checks, but it still omits three optional enhancement
bullets at the end of Epic 003. It also retains a rhetorical false positive,
question-versus-task/assignee errors in quoted email, and framing differences in
JSON question/answer spans. These are model quality limitations, not schema or
block-resolution successes disguised as complete extraction.

Prompt development showed sensitivity and model variability. The
[initial v4 run](evaluations/structure-scribe-v4-initial-neuralwatt.md) coalesced
mixed plain-text paragraphs and omitted section recommendations. A
[run before semantic gates](evaluations/structure-scribe-v4-before-semantic-gates-neuralwatt.md)
reached 11/11 exact Epic 003 units but regressed plain-text precision. A final
[coverage experiment](evaluations/structure-scribe-v4-coverage-experiment-neuralwatt.md)
regressed several fixtures and was reverted. The retained source matches the
retained v4 report; historical experiment commands require their earlier development
prompt snapshots and do not reproduce those prompts from the current source.
These single runs do not establish repeat-run stability or prove the complete
Epic 005 quality acceptance. Further evaluation should measure repeated runs and
improve optional-item recall without sacrificing the shared label semantics.

The v3 Markdown aggregate excludes the failed Epic 003 fixture from scored span
counts; its Markdown denominator is therefore smaller than v4's. Compare the
per-fixture results and failure rates alongside aggregate scores.

Verification: formatting, workspace checks for all targets/features, Clippy with
warnings denied, types/core/API tests with all features, all 61 GUI tests, all 23
expected detections, and whole-block stub scoring of the four new fixtures pass
without LLM credentials. Live Explainer tests remain explicitly opt-in.

Groq is also available in the API and both evaluation binaries. Use provider
`groq`, model `openai/gpt-oss-20b`, and `GROQ_API_KEY`; the optional
`GROQ_BASE_URL` defaults to `https://api.groq.com/openai/v1`. For Scribe evaluations,
set `DAFTPROMPT_EVAL_PROVIDER=groq`; for Explainer evaluations, explicitly set both
`DAFTPROMPT_EXPLAINER_PROVIDER=groq` and
`DAFTPROMPT_EXPLAINER_MODEL=openai/gpt-oss-20b`. See [API setup](api.md#groq-configuration).
