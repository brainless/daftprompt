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
│           ├── code.rs           # tree-sitter code parsing
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
- **tree-sitter / tree-sitter-rust** — Rust source parsing (in the indexer crate, Epic 004)
- **png** — PNG encoding for `--screenshot` output
- **clap** — CLI argument parsing
- **anyhow / serde_json** — error handling and serialization

## Next Steps

The previous "Next Steps" list (real-data integration, document parsing, file watching) is now done by Epic 002 (git log column), Epic 003 (commit indexer), and Epic 004 (code indexer). The remaining genuine follow-ups after Epic 005 are:

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

## License

This project is a prototype and is not yet licensed for distribution.
