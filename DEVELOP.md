# DEVELOP.md — Development Guide

## Project Status

This project is in **early stage of development**. Many features are incomplete, APIs are unstable, and architecture is evolving rapidly. Expect frequent breaking changes.

## Local Dependencies

Several dependencies are cloned locally under `~/Projects/` and referenced via path dependencies in `Cargo.toml`. Coding agents and contributors should refer to these local sources when debugging, reading docs, or understanding internals.

### Active path dependencies (used in Cargo.toml)

| Crate | Local path | Used by |
|---|---|---|
| **glyphon** | `~/Projects/glyphon` | `text-explorer` (GPU text rendering) |
| **glam** | `~/Projects/glam-rs` | `text-explorer` (math/transforms) |
| **gix** (gitoxide) | `~/Projects/gitoxide/gix` | `text-explorer` (git repository access) |

### Other relevant dependencies cloned locally

These are not currently path dependencies but are available locally for reference, and may become path dependencies in future epics:

| Crate | Local path | Relevance |
|---|---|---|
| **wgpu** | `~/Projects/wgpu` | GPU rendering pipeline (v29.0.0 from crates.io, source available locally) |
| **sqlite-vec** | `~/Projects/sqlite-vec` | Vector similarity search extension (v0.0.1-alpha.33, used in `sugacode-indexer`) |
| **model2vec-rs** | `~/Projects/model2vec-rs` | Static text embeddings (v0.2.1, used in `sugacode-indexer`) |
| **tree-sitter** | `~/Projects/tree-sitter` | Source code parsing with tree-sitter (Epic 004 — planned) |
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
cargo run -- --repo . --index  # index commits into search DB
cargo run -- --repo . --search "fix crash"  # CLI hybrid search (no GUI)
cargo run -- --repo . --index-code  # index Rust source code (Epic 004)
cargo run -- --repo . --search-code "render pipeline"  # CLI code search (Epic 004)
RUST_LOG=debug cargo run       # run with debug logging
cargo test --workspace         # run all tests
```

## Project Structure

```
sugacode/
├── Cargo.toml                    # workspace root + main binary
├── src/
│   ├── main.rs                   # entry point, CLI args, event loop
│   ├── renderer.rs               # wgpu rendering pipeline
│   ├── state.rs                  # application state
│   ├── input.rs                  # mouse/keyboard input handling
│   ├── git_log.rs                # git commit reader (gitoxide)
│   └── ui/
│       ├── mod.rs                # UI manager
│       ├── canvas.rs             # infinite canvas with grid
│       ├── drawer.rs             # left navigation drawer
│       ├── card.rs               # document/commit card renderer
│       ├── container.rs          # container abstraction (cards live in containers)
│       └── search.rs             # global search box (Cmd+K)
├── crates/
│   └── sugacode-indexer/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs            # Indexer public API
│           ├── db.rs             # SQLite schema, FTS5, vec0, queries
│           ├── embed.rs          # model2vec-rs wrapper
│           ├── code.rs           # Tree-sitter code parsing (Epic 004 — planned)
│           └── schema.sql        # SQL schema definition
└── epics/                        # feature epic specifications
```

## Architecture Notes

- **Canvas > Container > Card** hierarchy: Cards cannot exist directly on the canvas; they must be inside a Container.
- **Per-repo SQLite DB**: Each indexed repository gets its own DB file in the OS cache directory (`~/Library/Caches/sugacode/{repo_slug}.db` on macOS).
- **Hybrid search**: Combines FTS5 (keyword) and sqlite-vec (vector KNN) via Reciprocal Rank Fusion.
- **Graceful degradation**: If the embedding model fails to load, search falls back to FTS5-only. In non-git folders, Cmd+K falls back to substring matching.
- **Separate search modes**: Commit search (`Cmd+K`) and code search (`Cmd+Shift+K`) are independent APIs and UI modes (Epic 004).
