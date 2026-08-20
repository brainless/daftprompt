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
cargo run --release -- --screenshot /tmp/daftprompt.png --exit  # capture one frame to PNG and quit (visual regression)
RUST_LOG=debug cargo run       # run with debug logging
cargo test --workspace         # run all tests
```

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

## Epic 014: ACP Prompt Enrichment

### Crate Boundaries

| Crate | Purpose | Dependencies |
|-------|---------|-------------|
| `daftprompt-acp` | ACP SDK boundary, process lifecycle, typed events | `agent-client-protocol`, tokio |
| `daftprompt-prompt-builder` | Deterministic selection, budgets, prompt formatting | `daftprompt-indexer` |
| `daftprompt-storage` | Durable conversation and trace storage | `rusqlite`, `serde_json` |
| `daftprompt` (main) | Coordinator, UI, config, lifecycle | all above + akar |

### Key Invariants

- **Mandatory enrichment**: every ordinary user prompt goes through retrieval +
  formatting before reaching `session/prompt`. There is no bypass.
- **Isolation**: `daftprompt-storage` has its own DB under
  `~/Library/Caches/daftprompt/conversations/`. `--reindex` does not erase
  conversations.
- **ACP boundary**: only `daftprompt-acp` depends on the ACP SDK. All other
  crates use its typed public API.
- **No secrets in traces**: event payloads pass through `redact_secrets()`.

### Testing

```bash
cargo test --workspace                    # all tests (207+)
cargo test -p daftprompt-acp              # ACP client + fixtures (18 tests)
cargo test -p daftprompt-prompt-builder   # golden prompt tests (11 tests)
cargo test -p daftprompt-storage          # durable store tests (30 tests)
cargo test --test coordinator             # end-to-end coordinator tests (12 tests)
```

The `acp-fake-adapter` binary (built automatically by tests) replays captured
fixtures without network access or Codex credentials.

### Live Manual Testing

```bash
# Start with an installed codex-acp against an indexed repo
cargo run -- --repo ~/your-project

# Or from a local codex-acp source clone
cargo run -- --repo ~/your-project --adapter npm --adapter-args "run,start,--prefix,~/Projects/codex-acp"
```

Press Tab to open the conversation panel. See
`epics/research/014-acp-prompt-enrichment-evaluation-worksheet.md` for the
manual test protocol.
