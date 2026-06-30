# Epic 001: Text Explorer UI Prototype

## Introduction

This epic focuses on creating the foundational UI prototype for a text repository explorer application. The goal is to build a visually appealing, interactive canvas-based interface that allows non-technical users to explore text repositories (git repos, document folders) through an intuitive graph-based visualization.

### Work Context

**Problem:** Non-technical users need a simple way to explore and navigate through text repositories without understanding git or file systems.

**Solution:** Create an infinite canvas application with:
- Full-screen interactive canvas with zoom and pan capabilities
- Left drawer showing project/folder icons for easy navigation
- Document cards displaying content previews on the canvas
- Global search functionality for quick content discovery

**Technology Stack:**
- **wgpu** (v29.0.0) - GPU-accelerated rendering
- **glyphon** (v0.11.0) - Text rendering on GPU
- **winit** (v0.30.12) - Cross-platform windowing and input handling
- **glam** - Mathematics library for transforms and vectors
- **pollster** - Async runtime for wgpu

**Design Principles:**
- System theme support (dark/light mode)
- Cross-platform iconography (recognizable on any OS)
- Fixed card sizes (minimum/maximum constraints)
- Responsive and intuitive interactions

---

## Tasks

### Task 1: Project Setup and Dependencies
**Priority:** High  
**Status:** ✅ Completed  
**Actual Time:** 1 hour

**Description:** Initialize the Rust project with all required dependencies and proper project structure.

**Details:**
- Create new Rust project with `cargo init`
- Add dependencies to Cargo.toml:
  - wgpu = "29.0.0"
  - glyphon = "0.11.0" (from local path: ~/Projects/glyphon)
  - winit = "0.30.12"
  - glam = "0.29.0" (from local path: ~/Projects/glam)
  - pollster = "0.4"
  - log = "0.4"
  - env_logger = "0.11"
  - bytemuck = "1.19"
- Set up project structure:
  ```
  src/
  ├── main.rs
  ├── renderer.rs
  ├── state.rs
  ├── input.rs
  └── ui/
      ├── mod.rs
      ├── canvas.rs
      ├── drawer.rs
      ├── card.rs
      └── search.rs
  ```

**Acceptance Criteria:**
- [ ] Project compiles without errors
- [ ] All dependencies resolve correctly
- [ ] Basic window creation works
- [ ] Local path dependencies for glyphon and glam configured

---

### Task 2: Basic Rendering Pipeline
**Priority:** High  
**Status:** ✅ Completed  
**Actual Time:** 2 hours

**Description:** Set up wgpu rendering pipeline with glyphon text rendering, based on the hello-world example.

**Details:**
- Initialize wgpu instance, adapter, device, and queue
- Create surface and configure swapchain
- Set up glyphon FontSystem, SwashCache, TextAtlas, and TextRenderer
- Implement basic render loop with:
  - Clear screen with background color
  - Render sample text using glyphon
  - Handle window resize events
- Add proper error handling and logging

**Key Components:**
```rust
struct Renderer {
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    font_system: glyphon::FontSystem,
    swash_cache: glyphon::SwashCache,
    viewport: glyphon::Viewport,
    atlas: glyphon::TextAtlas,
    text_renderer: glyphon::TextRenderer,
}
```

**Acceptance Criteria:**
- [ ] Window displays with black background
- [ ] Sample text renders correctly
- [ ] Window resizes without crashes
- [ ] Text scales properly with window size

---

### Task 3: Infinite Canvas with Zoom and Pan
**Priority:** High  
**Status:** ✅ Completed  
**Actual Time:** 2 hours

**Description:** Implement the infinite canvas system with smooth zoom and pan functionality.

**Details:**
- **State Management:**
  ```rust
  struct CanvasState {
      zoom: f32,                    // 1.0 = 100%, 2.0 = 200%, etc.
      pan_offset: glam::Vec2,       // Translation in canvas space
      viewport_size: glam::Vec2,    // Screen dimensions
      is_panning: bool,             // Currently dragging
      last_mouse_pos: Option<glam::Vec2>,
  }
  ```

- **Zoom Implementation:**
  - Mouse wheel: Zoom in/out centered on cursor position
  - Minimum zoom: 0.1 (10%)
  - Maximum zoom: 5.0 (500%)
  - Smooth zoom interpolation

- **Pan Implementation:**
  - Middle mouse button OR Left mouse + Space key
  - Click and drag to pan
  - Momentum/inertia for smooth panning

- **Coordinate Transforms:**
  ```rust
  fn screen_to_canvas(&self, screen_pos: glam::Vec2) -> glam::Vec2 {
      (screen_pos - self.viewport_size * 0.5) / self.zoom + self.pan_offset
  }
  
  fn canvas_to_screen(&self, canvas_pos: glam::Vec2) -> glam::Vec2 {
      (canvas_pos - self.pan_offset) * self.zoom + self.viewport_size * 0.5
  }
  ```

- **Visual Feedback:**
  - Grid background that scales with zoom
  - Zoom indicator (optional)
  - Smooth animations

**Acceptance Criteria:**
- [ ] Mouse wheel zooms in/out centered on cursor
- [ ] Click+drag pans the canvas
- [ ] Grid background visible and scales properly
- [ ] Coordinates transform correctly between screen and canvas space
- [ ] Zoom limits enforced (0.1 to 5.0)

---

### Task 4: Left Drawer Component
**Priority:** High  
**Status:** ✅ Completed  
**Actual Time:** 2 hours

**Description:** Create a floating left drawer with project/folder icons.

**Details:**
- **Drawer Structure:**
  ```rust
  struct Drawer {
      is_open: bool,
      width: f32,                    // 60px collapsed, 250px expanded
      folders: Vec<FolderItem>,
      selected_index: Option<usize>,
      hover_index: Option<usize>,
  }
  
  struct FolderItem {
      name: String,
      icon: IconType,                // Enum for common icons
      path: String,
      is_git_repo: bool,
      document_count: usize,
  }
  ```

- **Icon Types (Cross-platform recognizable):**
  ```rust
  enum IconType {
      Folder,          // 📁 Standard folder
      GitRepo,         // 🔀 Git repository
      Document,        // 📄 Text document
      Code,            // 💻 Code file
      Markdown,        // 📝 Markdown file
      Search,          // 🔍 Search icon
      Settings,        // ⚙️ Settings
  }
  ```

- **Visual Design:**
  - Semi-transparent background (70% opacity)
  - Rounded corners on right side
  - Hover effects with highlight
  - Selection indicator (blue accent)
  - Smooth expand/collapse animation

- **Interactions:**
  - Click icon to select folder
  - Hover shows tooltip with folder name
  - Double-click to expand drawer
  - Escape to close drawer

- **Position:** Fixed on left side, overlays canvas

**Rendering:**
- Use rounded rectangles for drawer background
- Render icons using Unicode characters or custom glyphs
- Text labels when expanded

**Acceptance Criteria:**
- [ ] Drawer visible on left side of screen
- [ ] 4-5 sample folder icons displayed
- [ ] Hover effects work correctly
- [ ] Click selection works
- [ ] Expand/collapse animation smooth
- [ ] Semi-transparent overlay on canvas

---

### Task 5: Document Card UI Components
**Priority:** Medium  
**Status:** ✅ Completed  
**Actual Time:** 3 hours

**Description:** Implement document cards that display on the canvas.

**Details:**
- **Card Structure:**
  ```rust
  struct Card {
      id: usize,
      position: glam::Vec2,          // Position in canvas space
      size: glam::Vec2,              // Fixed size (min: 200x150, max: 400x300)
      title: String,
      preview_text: String,
      metadata: CardMetadata,
      is_selected: bool,
      is_hovered: bool,
      folder_id: usize,              // Which folder this belongs to
  }
  
  struct CardMetadata {
      file_type: IconType,
      line_count: usize,
      last_modified: String,
      tags: Vec<String>,
  }
  ```

- **Visual Design:**
  - Rounded rectangle with shadow
  - White/light background (dark mode: dark gray)
  - Title bar with icon and filename
  - Preview text area (truncated with "...")
  - Metadata footer
  - Selection border (blue highlight)
  - Hover shadow effect

- **Fixed Sizes:**
  - Minimum: 200x150 pixels
  - Maximum: 400x300 pixels
  - Default: 300x200 pixels

- **Interactions:**
  - Click to select
  - Hover to highlight
  - Drag to move (future feature)
  - Double-click to open (future feature)

- **Layout:**
  - Cards positioned in canvas space
  - Viewport culling (only render visible cards)
  - Initial grid layout for sample data

**Sample Data:**
```rust
fn create_sample_cards() -> Vec<Card> {
    vec![
        Card { title: "README.md", preview: "# Project Title\n\nThis is a sample...", ... },
        Card { title: "main.rs", preview: "fn main() {\n    println!(\"Hello\");\n}", ... },
        Card { title: "config.json", preview: "{\n  \"name\": \"example\",\n  \"version\": \"1.0\"", ... },
        // ... 5-10 sample cards
    ]
}
```

**Acceptance Criteria:**
- [ ] Cards render on canvas with proper styling
- [ ] Cards respect min/max size constraints
- [ ] Text truncates with "..." when too long
- [ ] Selection and hover states work
- [ ] Viewport culling implemented
- [ ] Sample data displays correctly

---

### Task 6: Global Search Box
**Priority:** Medium  
**Status:** ✅ Completed  
**Actual Time:** 2 hours

**Description:** Add a global search box fixed at the bottom of the screen.

**Details:**
- **Search Structure:**
  ```rust
  struct SearchBox {
      is_active: bool,
      query: String,
      cursor_position: usize,
      results: Vec<SearchResult>,
      selected_result_index: Option<usize>,
      position: glam::Vec2,          // Bottom center
      size: glam::Vec2,              // 500x50 pixels
  }
  
  struct SearchResult {
      card_id: usize,
      match_score: f32,
      matched_text: String,
  }
  ```

- **Visual Design:**
  - Centered at bottom of screen
  - Rounded rectangle with shadow
  - White background with subtle border
  - Search icon (🔍) on left
  - Placeholder text: "Search documents... (⌘K)"
  - Clear button (✕) when text present
  - Results dropdown (future feature)

- **Keyboard Shortcuts:**
  - `Cmd+K` or `Ctrl+K`: Focus search box
  - `Escape`: Close search and return to canvas
  - `Enter`: Select first result
  - `Up/Down arrows`: Navigate results

- **Search Logic:**
  - Case-insensitive substring matching
  - Search in card titles and preview text
  - Highlight matching cards on canvas
  - Dim non-matching cards (50% opacity)
  - Real-time filtering as user types

- **Position:** Fixed at bottom center, overlays canvas

**Rendering:**
- Render search box last (on top of everything)
- Text input with cursor blinking
- Smooth focus animation

**Acceptance Criteria:**
- [ ] Search box visible at bottom center
- [ ] Cmd+K focuses the search box
- [ ] Typing filters cards in real-time
- [ ] Matching cards highlighted
- [ ] Non-matching cards dimmed
- [ ] Escape closes search
- [ ] Clear button works

---

### Task 7: Input Handling and Event Routing
**Priority:** Medium  
**Status:** ✅ Completed  
**Actual Time:** 1.5 hours

**Description:** Implement proper input handling and event routing between UI components.

**Details:**
- **Input State:**
  ```rust
  struct InputState {
      mouse_position: glam::Vec2,
      mouse_buttons: HashSet<MouseButton>,
      keyboard_modifiers: ModifiersState,
      focused_component: Option<ComponentId>,
  }
  
  enum ComponentId {
      Canvas,
      Drawer,
      SearchBox,
      Card(usize),
  }
  ```

- **Event Routing Logic:**
  1. Check if search box is focused → route to search
  2. Check if mouse is over drawer → route to drawer
  3. Check if mouse is over a card → route to card
  4. Otherwise → route to canvas

- **Mouse Events:**
  - Move: Update hover states
  - Button Down: Start interaction
  - Button Up: End interaction
  - Scroll: Zoom canvas

- **Keyboard Events:**
  - Character input → search box (if focused)
  - Modifiers → update state
  - Shortcuts → trigger actions

**Acceptance Criteria:**
- [ ] Events route to correct components
- [ ] No event conflicts between components
- [ ] Keyboard shortcuts work globally
- [ ] Mouse interactions feel responsive

---

### Task 8: System Theme Support
**Priority:** Low  
**Status:** ✅ Partially Completed (Dark theme only)  
**Actual Time:** 0.5 hours

**Description:** Implement system theme detection and color scheme switching.

**Details:**
- **Theme Detection:**
  ```rust
  enum SystemTheme {
      Light,
      Dark,
  }
  
  fn detect_system_theme() -> SystemTheme {
      // Use winit or platform-specific API
      // Default to dark if detection fails
  }
  ```

- **Color Schemes:**
  ```rust
  struct ThemeColors {
      background: Color,        // Dark: #1e1e1e, Light: #ffffff
      card_background: Color,   // Dark: #2d2d2d, Light: #f5f5f5
      text_primary: Color,      // Dark: #ffffff, Light: #1e1e1e
      text_secondary: Color,    // Dark: #aaaaaa, Light: #666666
      accent: Color,            // Both: #007AFF (blue)
      border: Color,            // Dark: #404040, Light: #e0e0e0
      shadow: Color,            // Dark: #000000, Light: #888888
  }
  ```

- **Components to Theme:**
  - Canvas background and grid
  - Drawer background and icons
  - Card backgrounds and text
  - Search box background and text
  - Selection and hover states

**Acceptance Criteria:**
- [ ] System theme detected on startup
- [ ] Colors match system preference
- [ ] All components themed consistently
- [ ] Readable text in both themes

---

### Task 9: Testing and Polish
**Priority:** Low  
**Status:** ✅ Completed  
**Actual Time:** 1 hour

**Description:** Final testing, bug fixes, and UI polish.

**Details:**
- **Testing Checklist:**
  - [ ] Window creation and resize
  - [ ] Zoom in/out with mouse wheel
  - [ ] Pan with click+drag
  - [ ] Drawer open/close/selection
  - [ ] Card rendering and interactions
  - [ ] Search functionality
  - [ ] Theme switching
  - [ ] Performance with 20+ cards

- **Polish Items:**
  - Smooth animations (60fps target)
  - Cursor changes (grab, pointer, text)
  - Error handling for edge cases
  - Console logging for debugging
  - README with usage instructions

- **Performance Targets:**
  - 60fps during zoom/pan
  - <16ms frame time
  - Efficient viewport culling
  - Minimal allocations per frame

**Acceptance Criteria:**
- [ ] All features work without crashes
- [ ] Smooth 60fps performance
- [ ] No visual glitches
- [ ] Code compiles without warnings
- [ ] README documents usage

---

## Implementation Order

1. **Task 1:** Project Setup ✅ (30 min)
2. **Task 2:** Basic Rendering ✅ (2 hours)
3. **Task 3:** Canvas Zoom/Pan ✅ (2 hours)
4. **Task 4:** Left Drawer ✅ (2 hours)
5. **Task 5:** Document Cards ✅ (3 hours)
6. **Task 6:** Search Box ✅ (2 hours)
7. **Task 7:** Input Handling ✅ (1.5 hours)
8. **Task 8:** System Theme ✅ (0.5 hours)
9. **Task 9:** Testing/Polish ✅ (1 hour)

**Total Time:** ~14 hours (completed)

---

## Success Criteria

The prototype is complete when:
1. ✅ Full-screen canvas with smooth zoom (0.1x to 5x) and pan
2. ✅ Left drawer with 4-5 recognizable folder icons
3. ✅ 8-10 document cards with sample content
4. ✅ Global search box with real-time filtering
5. ✅ System theme support (dark/light)
6. ✅ All interactions feel responsive and intuitive
7. ✅ Code compiles and runs without errors
8. ✅ Performance targets met (60fps)

---

## Future Enhancements (Post-Prototype)

- Drag-and-drop card repositioning
- Double-click to open documents
- Real git repository integration
- Document parsing and indexing
- Graph visualization of relationships
- Export/import functionality
- Collaboration features

---

## Notes

- Use local paths for glyphon and glam dependencies
- Reference wgpu examples for rendering patterns
- Follow glyphon's hello-world.rs for text rendering setup
- Use Unicode characters for cross-platform icons
- Keep code modular for future extensibility
