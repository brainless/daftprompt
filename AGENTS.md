# AGENTS.md

Guide for AI coding agents (and humans) working on daftprompt. Read this before touching the code.

## Project Overview

daftprompt is a Rust app for exploring text repositories (git repos, document folders) via an infinite-canvas, card-based UI. It uses **akar** (a GPU UI component library on wgpu + glyphon + taffy) for rendering, combined with a git commit reader (gitoxide) and a hybrid search indexer (SQLite FTS5 + sqlite-vec embeddings).

**Status:** early stage. APIs unstable, architecture evolving. Expect breaking changes.

Primary references — read these:
- `DEVELOP.md` — build/run commands, local path dependencies, project structure, architecture notes
- `README.md` — user-facing feature list and controls
- `epics/` — per-feature epic specs (001 UI prototype → 005 akar migration). Each epic has tasks, acceptance criteria, and file-change summaries.

## Build, Run, Test

```bash
cargo check --workspace        # type-check everything (run after changes)
cargo run                      # launch the GUI (default repo = current dir)
cargo test --workspace         # all tests
RUST_LOG=debug cargo run       # debug logging

# CLI modes (no GUI):
cargo run -- --repo . --index                       # index all sources (git log, code, documents)
cargo run -- --repo . --reindex                     # drop + rebuild all indexes
cargo run -- --repo . --search "fix crash"          # CLI unified hybrid search (all sources)
cargo run -- --repo . --index-code                  # index Rust source (tree-sitter)
cargo run -- --repo . --reindex-code
cargo run -- --repo . --search-code "render pipeline"
cargo run -- --repo . --index-documents             # index documents (Markdown, plain text)
cargo run -- --repo . --reindex-documents
cargo run -- --repo . --search-documents "setup guide"
cargo run -- --repo . --index-git-log               # index git log only
cargo run -- --repo . --reindex-git-log
cargo run -- --repo . --search-git-log "fix crash"
cargo run -- -- --no-index                          # GUI but skip startup auto-index
```

There is no separate linter/formatter script — `cargo check --workspace` is the canonical correctness gate. Run it after every change.

## Local Path Dependencies

Several dependency crates have their full source cloned under `~/Projects/` and are referenced via path dependencies. **Agents should read this local source when debugging internals, understanding behavior, or checking APIs** rather than guessing or fetching from crates.io. Active path deps (in `Cargo.toml`): `akar-core`, `akar-layout`, `akar-components`, `akar-winit` (`~/Projects/akar/crates/akar-*`), `glam` (`~/Projects/glam-rs`), `gix` (`~/Projects/gitoxide/gix`). Additional sources available locally (not currently path deps but useful for reference): `wgpu`, `sqlite-vec`, `model2vec-rs`, `tree-sitter`, `xilem`, and others — see `DEVELOP.md` for the full local-clone table and the `[patch.crates-io]` pattern for switching between local and published versions.

## Architecture (current)

```
Canvas > Container > Card      ← cards cannot exist directly on the canvas
Per-repo SQLite DB             ← ~/Library/Caches/daftprompt/{repo_slug}.db (macOS)
Hybrid search                  ← FTS5 (keyword) + sqlite-vec (vector KNN) via Reciprocal Rank Fusion
Graceful degradation           ← embedding model load failure → FTS5-only; non-git folder → substring search
Unified search                 ← Cmd+K searches all three sources (git log, code, documents) simultaneously
```

### Workspace layout

```
src/                    main binary (CLI + GUI)
  main.rs               entry, CLI args (clap), event loop, indexer init
  state.rs              AppState (canvas, drawer, containers, search, indexer handle)
  git_log.rs            git commit reader (gitoxide) — read_log, read_log_all_branches
  ui/
    mod.rs              module tree (container, render)
    adapter.rs          stable card key hashing (commit, code, document results)
    container.rs        Container abstraction + CardData (business logic, keep across migrations)
    render.rs           immediate-mode render functions (canvas, drawer, search, containers)
crates/daftprompt-indexer/  standalone indexing + search crate
  src/
    lib.rs              Indexer public API (index_commits, index_code, index_documents,
                        search_hybrid, search_code_hybrid, search_document_hybrid, search_all_hybrid)
    db.rs               SQLite schema, FTS5, vec0/vec_code/vec_documents, queries
    embed.rs            model2vec-rs wrapper
    code.rs             tree-sitter symbol extraction (Rust)
    documents.rs        document discovery, chunking, incremental indexing
    schema.sql          items, items_fts (external-content), vec_items, vec_code, vec_documents,
                        code_files, document_files, triggers
```

### DB schema invariants (do not break)

- `items` is the single source of truth for text; `items_fts` is an external-content FTS5 index kept in sync by AFTER INSERT/UPDATE/DELETE triggers — never write to `items_fts` directly.
- `vec_items` holds commit vectors; `vec_code` holds code vectors; `vec_documents` holds document vectors. Partitioned per `source_type` for exact KNN isolation (no over-fetch-and-filter). See Epic 004 "Search Isolation" and Epic 007.
- Per-repo DB files: one repo = one `.db` file, filename = slug of repo path. KNN is naturally scoped, no `repo_id` column.
- Increparency: `code_files` and `document_files` track `mtime` + `content_hash` (xxh3, **not** `DefaultHasher` — no stability guarantee across Rust versions). `touch` without content change must not re-index.

When implementing, keep **rejected alternatives as comments in code** (Epic 004 Design Decisions #1–8) — they are deliberate tuning knobs, not dead code.

## Conventions

- **Comments:** the codebase deliberately retains commented-out rejected-alternatives and notes about future tuning. When adding to modules that have these (e.g. `db.rs`, `code.rs`), follow the pattern. Do not strip them. Otherwise follow the standard "no unnecessary comments" rule.
- **Epic specs are the source of truth** for feature shape. If a task's acceptance criteria are not met, the task is not done. Mark task status in the epic file when completing a task.
- **Failures degrade, never panic:** model download failure → FTS5-only search; non-git folder → substring search; parse error in a `.rs` file → index what parseable, log, continue (one transaction per file so a bad file doesn't roll back the batch).
- **Per-file transactions** for `index_code()`: a single bad file must not roll back the whole run.
- **Stable hashing:** use `xxhash-rust` xxh3 for `content_hash`, never `std::collections::hash_map::DefaultHasher`.
- **Tree-sitter queries** are compiled once at construction and reused for every file (per-file compilation is a startup-error bug).
- **Trait default methods** (with bodies) are indexed individually as `TraitMethod`; signature-only methods are folded into the parent `Trait` item. Identifiers carry the full canonical namespace (`file_path::module_path::TypeName::method`).

- **UI is rendered by akar** (post-Epic 005): daftprompt owns application state + the winit window; akar owns the wgpu pipeline, draw list, input state, layout, and components. `src/ui/render.rs` is the immediate-mode render layer; the per-frame `Layout::new()` rebuilds the taffy tree every frame.
- **Screenshot mode** (post-Task 8): `cargo run --release -- --screenshot <path> --exit` waits 5 s for the UI to settle, captures one frame via akar's `core.take_screenshot`, PNG-encodes the result, and exits. Useful for visual regression testing.

## Epics

Per-feature epic specs live in `epics/`. Each epic has tasks, acceptance criteria, and file-change summaries. Check the epic file for current status before starting work.