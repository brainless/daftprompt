# DEVELOP.md — Development Guide

## Project Status

This project is in **early stage of development**. Many features are incomplete, APIs are unstable, and architecture is evolving rapidly. Expect frequent breaking changes.

## Local Dependencies

Several dependencies are cloned locally under `~/Projects/` and referenced via path dependencies in `Cargo.toml`. Coding agents and contributors should refer to these local sources when debugging, reading docs, or understanding internals.

### Active path dependencies (used in Cargo.toml)

| Crate | Local path | Used by |
|---|---|---|
| **akar-core** | `~/Projects/akar/crates/akar-core` | `text-explorer` (wgpu pipeline, draw list, input state) |
| **akar-layout** | `~/Projects/akar/crates/akar-layout` | `text-explorer` (taffy flexbox layout) |
| **akar-components** | `~/Projects/akar/crates/akar-components` | `text-explorer` (buttons, inputs, drawer, canvas, etc.) |
| **akar-winit** | `~/Projects/akar/crates/akar-winit` | `text-explorer` (winit event routing) |
| **glam** | `~/Projects/glam-rs` | `text-explorer` (math/transforms) |
| **gix** (gitoxide) | `~/Projects/gitoxide/gix` | `text-explorer` (git repository access) |

### Other relevant dependencies cloned locally

These are not currently path dependencies but are available locally for reference, and may become path dependencies in future epics:

| Crate | Local path | Relevance |
|---|---|---|
| **akar** | `~/Projects/akar` | GPU UI component library that owns the rendering pipeline post-Epic 005 |
| **glyphon** | `~/Projects/glyphon` | Text shaping/atlas — transitive via `akar-core`; no longer a direct dep |
| **wgpu** | `~/Projects/wgpu` | GPU rendering pipeline (v29.0.0 from crates.io, source available locally) |
| **taffy** | (via `akar-layout`) | CSS Flexbox layout engine used by akar |
| **sqlite-vec** | `~/Projects/sqlite-vec` | Vector similarity search extension (v0.0.1-alpha.33, used in `sugacode-indexer`) |
| **model2vec-rs** | `~/Projects/model2vec-rs` | Static text embeddings (v0.2.1, used in `sugacode-indexer`) |
| **tree-sitter** | `~/Projects/tree-sitter` | Source code parsing with tree-sitter (Epic 004 — done) |
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
cargo run -- --repo . --index-code  # index Rust source code only
cargo run -- --repo . --search-code "render pipeline"  # CLI code search only
cargo run -- --repo . --index-documents  # index documents (Markdown, plain text) only
cargo run -- --repo . --search-documents "setup guide"  # CLI document search only
cargo run -- --repo . --index-git-log  # index git log only
cargo run -- --repo . --search-git-log "fix crash"  # CLI git-log search only
cargo run --release -- --screenshot /tmp/sugacode.png --exit  # capture one frame to PNG and quit (visual regression)
RUST_LOG=debug cargo run       # run with debug logging
cargo test --workspace         # run all tests
```

## Project Structure

```
sugacode/
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
│   └── sugacode-indexer/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs            # Indexer public API (commits, code, documents, unified search)
│           ├── db.rs             # SQLite schema, FTS5, vec0, queries
│           ├── embed.rs          # model2vec-rs wrapper
│           ├── code.rs           # tree-sitter code parsing
│           ├── documents.rs      # document discovery, chunking, incremental indexing
│           └── schema.sql        # SQL schema definition
└── epics/                        # feature epic specifications
```

## Architecture Notes

- **Canvas > Container > Card** hierarchy: Cards cannot exist directly on the canvas; they must be inside a Container.
- **Per-repo SQLite DB**: Each indexed repository gets its own DB file in the OS cache directory (`~/Library/Caches/sugacode/{repo_slug}.db` on macOS).
- **Hybrid search**: Combines FTS5 (keyword) and sqlite-vec (vector KNN) via Reciprocal Rank Fusion.
- **Graceful degradation**: If the embedding model fails to load, search falls back to FTS5-only. In non-git folders, Cmd+K falls back to substring matching.
- **Unified search**: Cmd+K searches all three sources (git log, code, documents) simultaneously and displays results in three separate containers. Source-specific CLI flags (`--search-code`, `--search-documents`, `--search-git-log`) are also available.
- **UI is rendered by akar** (post-Epic 005): sugacode owns application state + the winit window; akar owns the wgpu pipeline, draw list, input state, layout, and components. `src/ui/render.rs` is the immediate-mode render layer; the per-frame `Layout::new()` rebuilds the taffy tree every frame.
- **Screenshot mode** (post-Task 8): `cargo run --release -- --screenshot <path> --exit` waits 5 s for the UI to settle, captures one frame via akar's `core.take_screenshot`, PNG-encodes the result, and exits. Useful for visual regression testing.
