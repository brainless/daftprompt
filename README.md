# Text Explorer - UI Prototype

## Overview

This is a UI prototype for a text repository explorer application built with Rust, wgpu, and glyphon. The application provides an infinite canvas interface for exploring text documents in a visual, card-based layout.

## Features Implemented

✅ **Infinite Canvas**
- Zoom in/out with mouse wheel (0.1x to 5x)
- Pan with middle mouse button or Cmd/Ctrl + left click
- Grid background with coordinate indicators
- Zoom indicator in bottom-right corner

✅ **Left Drawer**
- 4 sample folder icons with Unicode symbols
- Hover effects and selection states
- Expand/collapse animation
- Document count display

✅ **Document Cards**
- 8 sample documents with previews
- File type icons
- Title, content preview, and metadata
- Hover and selection states
- Viewport culling for performance

✅ **Global Search Box**
- Fixed at bottom center of screen
- Cmd+K / Ctrl+K keyboard shortcut
- Real-time filtering (placeholder implementation)
- Result count display
- Clear button

✅ **System Theme Support**
- Dark theme (default)
- Theme-aware colors for all components

## Running the Application

```bash
# From the sugacode directory
cargo run
```

## Controls

- **Zoom**: Mouse wheel
- **Pan**: Middle mouse button OR Cmd/Ctrl + left mouse button
- **Select Card**: Left click on card
- **Select Folder**: Left click on folder icon in drawer
- **Open Search**: Cmd+K (Mac) or Ctrl+K (Windows/Linux)
- **Close Search**: Escape key
- **Deselect All**: Escape key (when search is closed)

## Project Structure

```
src/
├── main.rs              # Entry point and event loop
├── renderer.rs          # wgpu rendering pipeline
├── state.rs             # Application state management
├── input.rs             # Input handling (mouse, keyboard)
└── ui/
    ├── mod.rs           # UI manager
    ├── canvas.rs        # Infinite canvas with grid
    ├── drawer.rs        # Left drawer component
    ├── card.rs          # Document card renderer
    └── search.rs        # Global search box
```

## Dependencies

- **wgpu** (v29.0.0) - GPU rendering
- **glyphon** (v0.11.0) - Text rendering on GPU
- **winit** (v0.30.12) - Windowing and input handling
- **glam** - Mathematics library for transforms
- **pollster** - Async runtime

## Sample Data

The prototype includes 8 sample documents:
1. README.md - Project documentation
2. main.rs - Rust code example
3. config.json - JSON configuration
4. meeting-notes.md - Meeting notes
5. todo.txt - Task list
6. algorithms.rs - Binary search implementation
7. design.md - Design document
8. api-docs.txt - API documentation

## Next Steps

1. **Connect to Real Data**
   - Git repository integration
   - Document parsing and indexing
   - File system watching

2. **Enhanced Interactions**
   - Drag-and-drop card repositioning
   - Double-click to open documents
   - Context menus

3. **Graph Visualization**
   - Relationship mapping between documents
   - Visual connections on canvas
   - Clustering algorithms

4. **Performance Optimization**
   - Spatial indexing for large collections
   - Level-of-detail rendering
   - Background loading

5. **Additional Features**
   - Export/import functionality
   - Collaboration features
   - Plugin system

## Known Limitations

- Text rendering may have minor alignment issues
- Search is currently placeholder (no actual filtering)
- Card dragging not yet implemented
- No persistence between sessions

## Development

```bash
# Check for compilation errors
cargo check

# Run with debug logging
RUST_LOG=debug cargo run

# Run tests (when available)
cargo test
```

## License

This project is a prototype and is not yet licensed for distribution.
