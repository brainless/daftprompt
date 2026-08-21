# daftprompt

## Overview

This is a text repository explorer built with Rust and akar (a GPU UI component library) on top of wgpu and glyphon. The application provides an infinite canvas interface for exploring text documents in a visual, card-based layout, with hybrid commit, code, and document search.

## Features Implemented

✅ **Infinite Canvas**
- Zoom in/out with mouse wheel (0.1x to 5x)
- Pan with middle mouse button or Cmd/Ctrl + left click
- Grid background with coordinate indicators
- Zoom indicator in bottom-right corner

✅ **Left Drawer**
- Folder icons with Unicode symbols
- Hover effects and selection states
- Expand/collapse animation
- Document count display

✅ **Document Cards**
- Git log cards (real data from the current repo) and search result cards
- File type icons
- Title, content preview, and metadata
- Hover and selection states
- Viewport culling for performance

✅ **Global Search Box**
- Fixed at bottom center of screen
- Cmd+K / Ctrl+K keyboard shortcut (unified search — all sources)
- Real-time filtering across git log, code, and documents (hybrid FTS5 + vector KNN via the indexer)
- Result count display
- Three result containers on canvas (git log, codebase, documents)
- Clear button

✅ **Code Search Across Five Languages**
- Git-tracked Rust (`.rs`), TypeScript (`.ts`), TSX (`.tsx`), JavaScript (`.js`), and JSX (`.jsx`) files share one code index
- The existing global search box searches all five languages through the same hybrid FTS5 + vector KNN index; no separate language mode is required
- Tracked generated/vendor/minified files are filtered out centrally (`node_modules/`, `vendor/`, `dist/`, `build/`, `.next/`, `coverage/`, and `*.min.js`), so production paths stay language-neutral

✅ **System Theme Support**
- Dark theme (default, via `AKAR_THEME_DARK`)
- Theme-aware colors for all components

## Running the Application

```bash
# From the daftprompt directory
cargo run
```

## Controls

- **Zoom**: Mouse wheel
- **Pan**: Middle mouse button OR Cmd/Ctrl + left mouse button
- **Select Card**: Left click on card
- **Select Folder**: Left click on folder icon in drawer
- **Open Unified Search**: Cmd+K (Mac) or Ctrl+K (Windows/Linux) — searches git log, code, and documents
- **Close Search**: Escape key
- **Deselect All**: Escape key (when search is closed)

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

## Dependencies

- **akar-core / akar-layout / akar-components / akar-winit** — path deps at `~/Projects/akar/crates/akar-*`. Owns the wgpu pipeline, taffy layout, components, and winit event routing.
- **wgpu** (v29.0.0) — GPU rendering (still a direct dep; akar builds on it)
- **winit** (v0.30.12) — Windowing and input handling (still a direct dep; `akar-winit` augments it)
- **glam** — Mathematics library for transforms
- **pollster** — Async runtime
- **gix** (gitoxide) — Git repository access (path dep)
- **daftprompt-indexer** — in-workspace indexer crate (FTS5 + sqlite-vec + model2vec-rs)
- **tree-sitter / tree-sitter-rust / tree-sitter-typescript / tree-sitter-javascript** — tree-sitter parsing pipelines for Rust (`.rs`), TypeScript (`.ts`), TSX (`.tsx`), and JavaScript/JSX (`.js`/`.jsx`); tree-sitter-javascript shares one grammar across JS and JSX (Epics 004, 009, 010)
- **png** — PNG encoding for `--screenshot` output
- **clap** — CLI argument parsing
- **anyhow / serde_json** — error handling and serialization

## Next Steps

The previous "Next Steps" list (real-data integration, document parsing, file watching) is now done by Epic 002 (git log column), Epic 003 (commit indexer), Epic 004 (Rust code indexer), Epic 007 (document indexer), Epic 009 (TypeScript and TSX), and Epic 010 (JavaScript and JSX). The remaining genuine follow-ups after Epic 010 are:

1. **Drag-and-drop card repositioning** — Cards are positioned in world space; drag-to-move is a follow-up epic.
2. **Graph visualization** — Relationship mapping between documents/commits on the canvas.
3. **Context menus** — Right-click menus on cards, container headers, and the canvas background.
4. **Plugin system** — Third-party extension points for new container types and search backends.
5. **Window/canvas state persistence** — Restore pan, zoom, and selected folder across sessions.

## Known Limitations

- Card dragging is not yet implemented (cards are positioned in world space; drag-to-move is a future epic).
- Per-repo indexer DBs are persisted in the OS cache directory (`~/Library/Caches/daftprompt/{slug}.db` on macOS), but window state and canvas pan/zoom are not.
- `akar-winit` does not expose Cmd/Ctrl modifier state; modifier tracking is done manually in `AppState` via a `WindowEvent::ModifiersChanged` arm.

## Development

```bash
# Check for compilation errors
cargo check --workspace

# Run with debug logging
RUST_LOG=debug cargo run

# Capture one frame to PNG (visual regression)
cargo run --release -- --screenshot /tmp/daftprompt.png --exit

# Run tests
cargo test --workspace
```

## ACP Prompt Enrichment

daftprompt can enrich user prompts with repository context before sending them
to a coding agent via the Agent Client Protocol (ACP).

### Starting a Conversation

1. Ensure `codex-acp` is installed or specify the adapter path with `--adapter`
2. Start daftprompt against an indexed repository:
   ```
   cargo run -- --repo ~/your-project
   ```
3. Press **Tab** to open the conversation panel
4. Type your request and press **Send** (or Cmd+Enter)
5. daftprompt retrieves relevant code, documents, and git history, builds an
   enriched prompt, and sends it to the agent

### What Gets Enriched

Every ordinary prompt is automatically enriched with:
- Relevant source code excerpts (ranked by hybrid search)
- Document chunks matching your query
- Git log entries for recent changes

The original prompt is preserved exactly. Retrieved context is labeled as
untrusted reference material and safely delimited.

### Inspecting Enrichment

- Click the **Inspector** button in the conversation header to see:
  - The exact original prompt
  - The enriched prompt sent to the agent
  - Which excerpts were included or excluded and why
  - Retrieval budget and status
- Use the separate **Copy** buttons to copy the complete original or enriched
  prompt; the inspector shows success or clipboard-error feedback.

### Permissions

When the agent needs to run a command or access files, a permission dialog
appears. You must explicitly choose an option — nothing is auto-approved.

### Configuration

```bash
cargo run -- --adapter codex-acp                    # use installed binary
cargo run -- --adapter npm --adapter-args "run,start,--prefix,/path/to/codex-acp"  # from source
cargo run -- --request-timeout 120                   # 2-minute timeout
cargo run -- --shutdown-grace 15                     # 15s graceful shutdown
```

### Trace Location

Conversation records are stored in `~/Library/Caches/daftprompt/conversations/`
and survive index rebuilding (`--reindex`).

## License

This project is a prototype and is not yet licensed for distribution.
