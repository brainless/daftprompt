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
| akar (`akar-core`, `akar-layout`, `akar-components`, `akar-winit` 0.1.0) | <https://github.com/brainless/akar.git> | `969079c2e3d14d1843c5e7ba2dccfb4c93c06459` | 2026-10-28 |

Rig is declared with `default-features = false` plus the `agent` feature: the parser uses the typed `submit`-tool extractor (`rig::extractor`). The optional `live-eval` feature in core adds `reqwest` and `rustls` for the evaluation binary, and `daftprompt-api` enables the same transport in its own dependency so it can build the provider client from environment credentials. Core's own manifest and the GUI never enable a provider transport. Scribe takes any Rig completion model, so the provider client and its transport are chosen where the model is built (the API or the opt-in live evaluation).

Crates.io dependencies are declared once in the workspace `Cargo.toml`. Resolved versions from `Cargo.lock` (2026-10-31):

- `serde` 1.0.229 (derive), `serde_json` 1.0.154 — used by `daftprompt-types` and by `daftprompt-core` for fixtures and reports; `schemars` 1.2.3 — used by `daftprompt-types`.
- `thiserror` 2.0.21 — shared library errors.
- `sha2` 0.10.9 — SHA-256 payload hashes.
- `axum` 0.8.9, `tokio` 1.54.1 — HTTP service. Core uses only Tokio's timer for per-attempt timeouts. The API binary enables Tokio's `net` and `signal` features for the listener and Ctrl+C shutdown, and `http-body-util` 0.1.5 to distinguish an oversized request body; `tower` 0.5.3 (`util`) is a test-only dependency for driving the router.
- `rig` 0.43.0 with `reqwest` and `rustls` — enabled only by `daftprompt-api`, which builds the provider client from environment credentials. Core keeps Rig's `agent` feature only, so the shared library stays transport-free.
- `dotenvy` 0.15.7 — loads a local `.env` at startup of `daftprompt-api` and, as an optional dependency of the `live-eval` feature, of `evaluate-scribe`. Core's normal build does not include it.
- `ureq` 3.5.0 — GUI HTTP client.
- `wgpu` 29.0.4, `winit` 0.30.13, `pollster` 0.4.0 — GUI host integration. These are the versions akar builds against; akar does not re-export its surface or event-loop types, so the app declares them directly. They stay out of core, types, and the API.
- `glyphon` 0.11.0 — same text shaping version as akar's pipeline. The GUI uses it only to give the scrollable block text an explicit viewport clip; app chrome still uses akar components.
- `arboard` 3.7.0 (default features off: text only, no image stack) — GUI system clipboard for textarea copy and paste. Also pulls `clipboard-win` 5.4.3 and `error-code` 3.4.0 on Windows, and `objc2-core-graphics`/`objc2-io-surface` 0.3.2 on macOS. Used only by `daftprompt-gui`.

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
`crates/daftprompt-core/prompts/scribe-v1.md`, included at compile time, and recorded on
every run as `scribe-v1`.

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
- Tests use the public `ExtractionModel` seam with a stub. No LLM credentials are needed
  for `cargo test --workspace`; live evaluation is opt-in (see below).

## Scribe evaluation

The fixture set and scoring live in `daftprompt_core::eval`; the data is in
`crates/daftprompt-core/fixtures/scribe/`. There is one JSON file per text, and the
fixture set covers chat, prose, pasted code, a pasted document, email quotations, JSON
text, and text with no actionable content. It includes locally answered and unanswered
questions plus explicit and incidental `@` mentions. Each file records:

- the complete text and optional source metadata;
- expected spans as half-open byte ranges plus a verbatim `quote` and their `question`
  labels (with an optional `assignee`);
- expected `answer` links between spans by local span id.

`load_fixtures` validates the format before scoring: quotes must equal their byte range,
ranges must be trimmed and on `char` boundaries, spans must not overlap, handle shapes
must be valid, and answer links must target expected questions. A malformed fixture
fails the ordinary test suite.

Scores are reported for spans, labels, answer links, and assignees, each under two span
matching modes:

- **exact** — byte ranges are equal after trimming surrounding whitespace;
- **lenient** — overlap divided by union is at least `0.5`.

A label is correct when its kind matches on a matched span, an answer link when both
ends match, and an assignee when the handle is identical on a matched question. Matching
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
| `DAFTPROMPT_EVAL_PROVIDER` | `openai` | `openai` (`OPENAI_BASE_URL` selects a compatible endpoint) or `neuralwatt` |
| `DAFTPROMPT_EVAL_MODEL` | `gpt-4o-mini` (`qwen3.6-35b-fast` for `neuralwatt`) | Model identifier recorded on every run |
| `DAFTPROMPT_EVAL_PROMPT_VERSION` | compiled version | Optional guard; must equal the compiled prompt version |
| `DAFTPROMPT_EVAL_MAX_ATTEMPTS` | `3` | Total attempt budget |
| `DAFTPROMPT_EVAL_TIMEOUT_SECS` | `60` | Per-attempt timeout |
| `OPENAI_API_KEY` | required for live `openai` runs | Read by `OpenAI::from_env()` |
| `NEURALWATT_API_KEY` | required for live `neuralwatt` runs | Bearer key for Neuralwatt |
| `NEURALWATT_BASE_URL` | `https://api.neuralwatt.com/v1` | Optional override |

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
endpoint. The full environment table, a curl example, the result shape, and the error
mapping are in [api.md](api.md). The API maps explicit `ParseError` variants to
400, 413, 422, 502, 504, or 500 and never returns a partly valid graph. Tests
drive the router with a stub `Parser`, so they need no credentials.

## Native GUI

`daftprompt-gui` is the two-column akar app. The left column holds a multiline
textarea anchored at the bottom and a Parse button; the right column renders the
original text in source order as scrollable blocks with label badges, statuses,
assignees, and answer targets. Each submission replaces the previous result.

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
