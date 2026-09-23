# DEVELOP.md — Development Guide

## Project Status

This project is in **early stage of development**. Many features are incomplete, APIs are unstable, and architecture is evolving rapidly. Expect frequent breaking changes.

## Local Dependencies

Several dependencies are cloned locally under `~/Projects/` and referenced via path dependencies in `Cargo.toml`. Coding agents and contributors should refer to these local sources when debugging, reading docs, or understanding internals.

### Active path dependencies (used in Cargo.toml)

| Crate | Local path | Used by |
|---|---|---|
| **akar-core** | `~/Projects/akar/crates/akar-core` | `daftprompt` (wgpu pipeline, draw list, input state) |
| **akar-layout** | `~/Projects/akar/crates/akar-layout` | `daftprompt` (taffy flexbox layout) |
| **akar-components** | `~/Projects/akar/crates/akar-components` | `daftprompt` (buttons, inputs, drawer, canvas, etc.) |
| **akar-winit** | `~/Projects/akar/crates/akar-winit` | `daftprompt` (winit event routing) |
| **glam** | `~/Projects/glam-rs` | `daftprompt` (math/transforms) |
| **gix** (gitoxide) | `~/Projects/gitoxide/gix` | `daftprompt` (git repository access) |

### Other relevant dependencies cloned locally

These are not currently path dependencies but are available locally for reference, and may become path dependencies in future epics:

| Crate | Local path | Relevance |
|---|---|---|
| **akar** | `~/Projects/akar` | GPU UI component library that owns the rendering pipeline post-Epic 005 |
| **glyphon** | `~/Projects/glyphon` | Text shaping/atlas — transitive via `akar-core`; no longer a direct dep |
| **wgpu** | `~/Projects/wgpu` | GPU rendering pipeline (v29.0.0 from crates.io, source available locally) |
| **taffy** | (via `akar-layout`) | CSS Flexbox layout engine used by akar |
| **sqlite-vec** | `~/Projects/sqlite-vec` | Vector similarity search extension (v0.0.1-alpha.33, used in `daftprompt-indexer`) |
| **model2vec-rs** | `~/Projects/model2vec-rs` | Static text embeddings (v0.2.1, used in `daftprompt-indexer`) |
| **tree-sitter** | `~/Projects/tree-sitter` | Source code parsing with tree-sitter (Epic 004 — done) |
| **tree-sitter-typescript** | `~/Projects/tree-sitter-typescript` | TypeScript/TSX grammars (v0.23 from crates.io in `daftprompt-indexer`; local clone available for reference) |
| **xilem** | `~/Projects/xilem` | Rust-native UI framework (under evaluation) |
| **sqlx** | `~/Projects/sqlx` | Async SQL toolkit (potential future use) |
| **diesel** | `~/Projects/diesel` | ORM (potential future use) |
| **refinery** | `~/Projects/refinery` | Database migrations (potential future use) |
| **ort** | `~/Projects/ort` | ONNX Runtime bindings (potential future use for ML inference) |
| **casbin-rs** | `~/Projects/casbin-rs` | Authorization framework (potential future use) |
| **loco** | `~/Projects/loco` | Rust web framework (potential future use) |
| **tauri** | `~/Projects/tauri` | Desktop app framework (potential future use) |

### Switching between local and crates.io dependencies

To use a local checkout instead of the published version, add or uncomment a `[patch.crates-io]` section in the root `Cargo.toml`:

```toml
# Example: use local wgpu for debugging
# [patch.crates-io]
# wgpu = { path = "../wgpu/wgpu" }
# sqlite-vec = { path = "../sqlite-vec" }
# model2vec-rs = { path = "../model2vec-rs" }
```

## Build & Run

```bash
cargo check --workspace        # type-check everything
cargo run                      # launch the GUI
cargo run -- --repo ~/some-repo  # open a specific git repo
cargo run -- --repo . --index  # index all sources (git log, code, documents)
cargo run -- --repo . --search "fix crash"  # CLI unified hybrid search (all sources)
cargo run -- --repo . --index-code  # index Rust, TypeScript, TSX, JavaScript, and JSX source code
cargo run -- --repo . --search-code "render pipeline"  # CLI code search only
cargo run -- --repo . --index-documents  # index documents (Markdown, plain text) only
cargo run -- --repo . --search-documents "setup guide"  # CLI document search only
cargo run -- --repo . --index-git-log  # index git log only
cargo run -- --repo . --search-git-log "fix crash"  # CLI git-log search only
cargo run --bin graph -- build --repo . --source DEVELOP.md --source README.md --source AGENTS.md --source 'epics/*.md'
cargo run --bin graph -- search --repo . "Can we search Python code?" --json
cargo run --release -- --screenshot /tmp/daftprompt.png --exit  # capture one frame to PNG and quit (visual regression)
RUST_LOG=debug cargo run       # run with debug logging
cargo test --workspace         # run all tests
```

### Document provenance graph CLI

`graph build` and `graph search` store and query provenance data in the same
per-repository SQLite database used by the indexer. They use the GitHub
`llm-sdk` Xiaomi client with its `MIMO_V2_5_PRO` model constant. Set
`XIAOMI_API_KEY` in the shell or in a Git-ignored `.env` file in the current
working directory; the graph CLI loads that file when present. Missing
configuration and model failures are errors; graph commands do not
silently return an empty graph or fall back to code search.
Extraction is limited to the graph's supported typed relations; a broad PRD
passage may contribute no claims. A rejected model extraction gets one
correction attempt before the build reports a validation error.

Every `--source` declares that repo-relative document authoritative for that
build. It may be a readable UTF-8 Markdown/plain-text file or quoted glob;
untracked files are allowed, matches are sorted/deduplicated, and the complete
selected set replaces the previously active set on a successful build. The
graph stores the exact working-tree snapshot and cites its one-based source
line ranges. This declaration does not verify authorship. Source code is never
an authority for graph claims.

```bash
export XIAOMI_API_KEY="your-key"

cargo run --bin graph -- build --repo . \
  --source DEVELOP.md --source README.md --source AGENTS.md \
  --source 'epics/*.md'
cargo run --bin graph -- search --repo . "Which languages can code search find?"
cargo run --bin graph -- search --repo . "Can we search Python code?" --json
cargo run --bin graph -- search --repo . "Can we search Lua code?" --json
cargo run --bin graph -- search --repo . "Does the indexer use Tree-sitter?"
```

Search prints a concise `yes`, `no`, or `unknown` answer with citations in
`repo-relative-path:start-end` form. `--json` includes `outcome`, `answer`,
`citations`, `claim_ids`, and `build_id`. `no` requires a current explicit
exclusion or a validated exhaustive set; insufficient and unresolved
conflicting evidence yields `unknown`.

Search records are written atomically as separate UTF-8 JSON files in
`<repo>/.daftprompt/graph-search/`; that directory must be Git-ignored in the
target repository. Each record contains schema version, UTC timestamp,
repository path, graph build ID, question, status, exact response, citations,
claim IDs, and an optional concise error. API keys, request headers, full
prompts, and source documents are excluded. The local records may contain
sensitive questions. Keep this folder private and remove records according to
your repository's data-retention needs.

## Project Structure

```
daftprompt/
├── Cargo.toml                    # workspace root + main binary
├── src/
│   ├── main.rs                   # entry point, CLI args, event loop, screenshot flow
│   ├── state.rs                  # application state (CanvasState, AppState)
│   ├── git_log.rs                # git commit reader (gitoxide)
│   └── ui/
│       ├── mod.rs                # module tree (container, render)
│       ├── adapter.rs            # stable card key hashing
│       ├── container.rs          # data model (Container, CardData, DocumentData, ContainerType)
│       └── render.rs             # immediate-mode render functions (canvas, drawer, search, containers)
├── crates/
│   └── daftprompt-indexer/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs            # Indexer public API (commits, code, documents, unified search)
│           ├── db.rs             # SQLite schema, FTS5, vec0, queries
│           ├── embed.rs          # model2vec-rs wrapper
│           ├── code.rs           # language registry/router + shared Rust/TypeScript/TSX/JavaScript/JSX query constants
│           ├── documents.rs      # document discovery, chunking, incremental indexing
│           └── schema.sql        # SQL schema definition
└── epics/                        # feature epic specifications
```

## Architecture Notes

- **Canvas > Container > Card** hierarchy: Cards cannot exist directly on the canvas; they must be inside a Container.
- **Per-repo SQLite DB**: Each indexed repository gets its own DB file in the OS cache directory (`~/Library/Caches/daftprompt/{repo_slug}.db` on macOS).
- **Hybrid search**: Combines FTS5 (keyword) and sqlite-vec (vector KNN) via Reciprocal Rank Fusion.
- **Graceful degradation**: If the embedding model fails to load, search falls back to FTS5-only. In non-git folders, Cmd+K falls back to substring matching.
- **Unified search**: Cmd+K searches all three sources (git log, code, documents) simultaneously and displays results in three separate containers. Source-specific CLI flags (`--search-code`, `--search-documents`, `--search-git-log`) are also available.
- **Code language routing**: `crates/daftprompt-indexer/src/code.rs` owns extension-based dispatch for Git-tracked `.rs`, `.ts`, `.tsx`, `.js`, and `.jsx` files and the compiled tree-sitter query registry. CLI/UI indexing and search stay language-neutral and call the shared indexer APIs. The shared code-file discovery path also enforces a centralized generated/vendor/minified exclusion (`is_excluded_generated_path`: `node_modules/`, `vendor/`, `dist/`, `build/`, `.next/`, `coverage/`, and `*.min.js`).
- **UI is rendered by akar** (post-Epic 005): daftprompt owns application state + the winit window; akar owns the wgpu pipeline, draw list, input state, layout, and components. `src/ui/render.rs` is the immediate-mode render layer; the per-frame `Layout::new()` rebuilds the taffy tree every frame.
- **Screenshot mode** (post-Task 8): `cargo run --release -- --screenshot <path> --exit` waits 5 s for the UI to settle, captures one frame via akar's `core.take_screenshot`, PNG-encodes the result, and exits. Useful for visual regression testing.
