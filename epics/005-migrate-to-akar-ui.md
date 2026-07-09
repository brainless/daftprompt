# Epic 005: Migrate UI to akar

## Introduction

This epic migrates sugacode's rendering layer from its hand-rolled wgpu + glyphon text-only pipeline to **akar**, a GPU-accelerated UI component library built on the same wgpu + glyphon + taffy stack. akar was directly inspired by sugacode's renderer and UI architecture (see akar's DEVELOP.md reference table), making this a natural consolidation: sugacode sheds ~1,100 lines of rendering code and gains proper quad rendering, flexbox layout, 30+ styled components, and a screenshot utility.

### Work Context

**Problem:** sugacode's current UI is rendered entirely through glyphon text buffers — rectangles are approximated by filling buffers with spaces and setting their color. There are no custom shaders, no geometry rendering, no proper rect/border/shadow support. This limits visual fidelity, performance, and development velocity. Every new UI element requires manual pixel math and text-buffer hacks.

**Solution:** Replace `src/renderer.rs`, `src/ui/canvas.rs`, `src/ui/drawer.rs`, `src/ui/search.rs`, and the rendering paths in `src/ui/mod.rs` with calls to akar's immediate-mode component API. Preserve all business logic: git log reading (`git_log.rs`), application state (`state.rs`), container data models (`container.rs`), CLI args, and the `sugacode-indexer` crate.

**Technology Stack (after migration):**
- **akar** (`~/Projects/akar`) — GPU UI component library (path dependency)
  - `akar-core` — wgpu pipelines, draw list, input state
  - `akar-layout` — taffy flexbox wrapper
  - `akar-components` — 30+ UI components
  - `akar-winit` — winit event bridge
- **wgpu** 29 — GPU rendering (shared with akar)
- **glyphon** 0.11 — text rendering (shared with akar)
- **winit** 0.30 — windowing (shared with akar)
- **glam** — math (shared with akar)
- **gix** — git operations (unchanged)
- **sugacode-indexer** — SQLite FTS5 + vector search (unchanged)

**Design Principles:**
- akar owns the renderer; sugacode owns the application state
- Immediate mode: no retained widget tree, no diffing
- Preserve the Canvas > Container > Card hierarchy using akar's canvas + scroll_area + container components
- Preserve all CLI modes (`--repo`, `--index`, `--search`, `--index-code`, `--search-code`)
- Leverage akar's screenshot utility for visual regression testing

### Component Mapping

| sugacode (current) | akar (target) | Notes |
|---|---|---|
| `renderer.rs` (wgpu setup, render pass, glyphon pipeline) | `AkarCore::new(device, queue, surface_format)` | akar owns the GPU pipeline |
| `ui/canvas.rs` (grid, zoom indicator) | `canvas_begin/end` + `CanvasPainter::push_quad` | akar's canvas has cursor-anchored zoom |
| `ui/drawer.rs` (folder icons, expand/collapse) | `drawer_begin/end` + `container` + `label` | akar drawer is behavior-only (scrim + panel) |
| `ui/search.rs` (search box rendering) | `text_input` + `modal` | akar has proper text input with cursor |
| `ui/mod.rs` (container/card rendering) | `container` + `scroll_area` + `list_clip` + `label` | akar has proper quad rendering |
| `input.rs` (winit event handling) | `akar_winit::process_window_event` | Direct replacement |
| `state.rs` (application state) | **Preserve** | Business logic, no changes needed |
| `git_log.rs` (git commit reading) | **Preserve** | Business logic, no changes needed |
| `sugacode-indexer/` | **Preserve** | Independent crate, no changes needed |

---

## Tasks

### Task 1: Add akar Dependencies and Remove Old Renderer

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 2 hours

**Description:** Replace sugacode's direct wgpu/glyphon/winit/glam dependencies with akar crates. Remove `renderer.rs` and wire up akar's `AkarCore` as the rendering backend.

**Details:**

1. Update `Cargo.toml`:
   ```toml
   [dependencies]
   # Remove these:
   # wgpu = "29.0.0"
   # winit = "0.30.12"
   # glyphon = { path = "../glyphon" }
   # glam = { path = "../glam-rs" }
   # pollster = "0.4"
   # bytemuck = "1.19"

   # Add these:
   akar-core = { path = "../akar/crates/akar-core" }
   akar-layout = { path = "../akar/crates/akar-layout" }
   akar-components = { path = "../akar/crates/akar-components" }
   akar-winit = { path = "../akar/crates/akar-winit" }

   # Keep unchanged:
   gix = { path = "../gitoxide/gix", default-features = false, features = ["basic", "sha1"] }
   clap = { version = "4", features = ["derive"] }
   sugacode-indexer = { path = "crates/sugacode-indexer" }
   anyhow = "1"
   serde_json = "1"
   log = "0.4"
   env_logger = "0.11"
   ```

2. Delete `src/renderer.rs` — its entire responsibility (wgpu instance, device, surface, glyphon pipeline, render pass) is now owned by `AkarCore`.

3. Rewrite `src/main.rs` event loop to use `AkarCore`:
   - `AkarCore::new(device, queue, surface_format)` in the `resumed` handler
   - `core.begin_frame(width, height, scale_factor)` at the start of each `RedrawRequested`
   - `core.end_frame(device, queue, &mut render_pass)` at the end
   - wgpu instance/device/surface creation stays in `main.rs` (sugacode owns the window, akar owns the pipeline)

4. Delete `src/input.rs` — replace with `akar_winit::process_window_event` feeding `AkarCore`'s `InputState`.

5. Update `Application` struct to hold `AkarCore` instead of `Renderer`.

**Acceptance Criteria:**
- [ ] `cargo check` passes with akar dependencies
- [ ] Window opens and shows a clear color (no text yet)
- [ ] Window resize works
- [ ] winit events are forwarded to akar's InputState
- [ ] `renderer.rs` and `input.rs` are deleted
- [ ] No direct wgpu/glyphon/winit/glam imports remain in `Cargo.toml`

---

### Task 2: Migrate Canvas — Grid and Zoom/Pan

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 3 hours

**Description:** Replace sugacode's custom infinite canvas (coordinate transforms, grid rendering, zoom indicator) with akar's `canvas_begin/end` + `CanvasPainter`.

**Details:**

1. Replace `AppState` canvas fields with akar's `CanvasState`:
   ```rust
   // Remove from AppState:
   // pub zoom: f32,
   // pub pan_offset: Vec2,
   // pub is_panning: bool,
   // pub last_mouse_pos: Option<Vec2>,

   // Add to AppState:
   pub canvas_state: akar_components::canvas::CanvasState,
   ```
   Note: sugacode's `screen_to_canvas` / `canvas_to_screen` methods are replaced by `CanvasResponse.world_to_screen` / `CanvasResponse.screen_to_world` returned from `canvas_begin`.

2. Replace `Canvas` struct in `ui/canvas.rs` with akar canvas usage in the render loop:
   ```rust
   let (resp, painter) = canvas_begin(&mut core, layout, canvas_node, &mut state.canvas_state, &CanvasConfig {
       pan_button: PanButton::Middle,
       zoom_sensitivity: 0.005,
       zoom_min: 0.1,
       zoom_max: 5.0,
   });

   // Grid rendering using painter.push_quad() in world space
   render_grid(&painter, resp.visible_world_rect, resp.world_to_screen);

   // Zoom indicator using label() in screen space (after canvas_end)
   canvas_end(&mut core, painter);
   ```

3. Migrate grid rendering from text-based labels to `CanvasPainter::push_quad`:
   - Grid lines: thin quads at regular world-space intervals
   - Coordinate labels: deferred (akar does not yet have `CanvasPainter::push_text`; grid lines alone are sufficient)
   - Hide grid when zoomed out past readability threshold

4. Migrate zoom indicator to akar's `label` component positioned at bottom-right corner.

5. Remove `AppState::screen_to_canvas` and `AppState::canvas_to_screen` — use `CanvasResponse` transforms instead.

**Acceptance Criteria:**
- [ ] Canvas renders with grid lines in world space
- [ ] Mouse wheel zooms centered on cursor (cursor-anchored)
- [ ] Middle mouse button pans the canvas
- [ ] Cmd+Left click pans the canvas (sugacode's existing convention)
- [ ] Zoom limits enforced (0.1 to 5.0)
- [ ] Zoom indicator shows current percentage at bottom-right
- [ ] `ui/canvas.rs` is deleted; canvas logic lives in the render loop

---

### Task 3: Migrate Drawer

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 2 hours

**Description:** Replace sugacode's custom drawer (emoji icons, expand/collapse animation, text-buffer backgrounds) with akar's `drawer_begin/end` + `container` + `label` + `button` components.

**Details:**

1. Replace `Drawer` struct with a simpler state:
   ```rust
   // Keep from Drawer:
   pub is_open: bool,
   pub selected_folder: Option<usize>,
   pub hover_index: Option<usize>,
   // Remove: width, expanded_width, animation_progress, get_rect, get_current_width
   // akar's drawer_begin handles panel sizing; animation is caller-managed (same as current)
   ```

2. Replace `Drawer::render()` with akar components:
   ```rust
   // Animated width (preserve existing frame-rate-dependent approach)
   let panel_width = lerp(60.0, 250.0, animation_progress);

   let resp = drawer_begin(&mut core, viewport_rect, DrawerEdge::Left, panel_width, &theme);
   if resp.close_requested {
       state.drawer_open = false;
   }

   // Render folder list inside drawer panel
   // Use layout to position folder items
   for (i, folder) in state.folders.iter().enumerate() {
       let icon_node = layout.new_leaf(Style { .. });
       // Position icon using layout
       let btn = button(&mut core, &layout, icon_node, &folder.icon_emoji, ButtonVariant::Ghost, &theme);
       if btn.clicked {
           state.selected_folder = Some(i);
       }
       if btn.hovered {
           state.hover_index = Some(i);
       }
       // Show name + doc count when expanded
       if animation_progress > 0.5 {
           let label_node = layout.new_leaf(Style { .. });
           label(&mut core, &layout, label_node, &format!("{} ({} docs)", folder.name, folder.document_count), theme.base_content, &theme);
       }
   }

   drawer_end(&mut core);
   ```

3. Migrate folder icon rendering from emoji-in-text-buffer to akar's `label` component (emoji text renders naturally through glyphon).

4. Keep the expand/collapse animation logic but use delta-time instead of frame-rate-dependent increments.

**Acceptance Criteria:**
- [ ] Drawer appears on left edge with scrim overlay
- [ ] Folder icons render with hover effects
- [ ] Click to select a folder works
- [ ] Expand/collapse animation is smooth
- [ ] Folder name and doc count shown when expanded
- [ ] Scrim click or Escape closes the drawer
- [ ] `ui/drawer.rs` is deleted; drawer logic lives in the render loop

---

### Task 4: Migrate Container and Card Rendering

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 4 hours

**Description:** Replace sugacode's custom container backgrounds, borders, scroll indicators, and card rendering (text-buffer rectangles) with akar's `container` + `scroll_area` + `list_clip` + `label` components.

**Details:**

1. Preserve `Container` data model (`ui/container.rs`) — it is business logic:
   - `ContainerType` enum (DocumentGrid, GitLogColumn, SearchResults, CodeSearchResults)
   - `CardData`, `DocumentData` structs
   - `new_git_log`, `new_search_results`, `new_code_search_results` constructors
   - Card height calculation logic
   - `visible_cards()` iterator (replaced by akar's `list_clip` but logic is similar)

2. Replace container rendering in `ui/mod.rs`:
   ```rust
   // For each container on the canvas:
   for container in &mut state.containers {
       // Position container in world space using canvas painter
       let container_screen_rect = resp.world_to_screen(container.position, container.size);

       // Container background + border
       container_component(&mut core, container_screen_rect, &BoxStyle::panel(&theme));

       // Scroll area for container content
       let scroll_resp = scroll_area_begin(&mut core, container_screen_rect, &mut container.scroll_offset, container.content_height);

       // Virtualized card rendering
       let visible = list_clip(container.cards.len(), card_height, container.scroll_offset, container_screen_rect[3]);
       for i in visible {
           let card = &container.cards[i];
           let card_y = scroll_resp.content_y + card.position.y;
           let card_rect = [container_screen_rect[0] + 8.0, card_y, container_screen_rect[2] - 16.0, card.size.y];

           // Card background
           container_component(&mut core, card_rect, &BoxStyle::card(&theme));

           // Card content (git commit or document)
           match container.container_type {
               ContainerType::GitLogColumn => render_git_card_akar(&mut core, &layout, card, card_rect, &theme),
               _ => render_doc_card_akar(&mut core, &layout, card, card_rect, &theme),
           }
       }

       scroll_area_end(&mut core);
   }
   ```

3. Migrate git commit card rendering (`render_git_card_content`):
   - Replace text-buffer rectangles with akar `label` components
   - Hash: cyan label at 12px
   - Author + date: gray labels at 11px
   - Separator: akar `separator` component
   - Message: white label at 13px
   - Hover state: use `container_component` with different `BoxStyle`

4. Migrate document card rendering (`render_doc_card_content`):
   - Icon + title: `label` with emoji prefix
   - Content preview: `label` with truncated text
   - Metadata footer: `label` at smaller font size
   - Selection/hover: different `BoxStyle` fills

5. Migrate card hover detection — use `core.input.is_hovering(card_rect)` instead of manual AABB checks.

6. Delete `ui/mod.rs` rendering functions: `render_container`, `render_card`, `render_git_card_content`, `render_doc_card_content`, `create_text_buffer`, `truncate_content`, `get_file_icon`, `is_visible`. Keep `UIManager::new`, `UIManager::update`, and card hover state logic.

**Acceptance Criteria:**
- [ ] Git log container renders commit cards with proper backgrounds and borders
- [ ] Cards show hash (cyan), author, date, separator, message
- [ ] Container scrolling works with mouse wheel when cursor is over container
- [ ] Scroll indicator renders for containers with overflow content
- [ ] Viewport culling via `list_clip` — only visible cards are rendered
- [ ] Card hover state changes background color
- [ ] Document cards render with icon, title, preview, metadata
- [ ] Search result containers render correctly
- [ ] Canvas > Container > Card hierarchy preserved

---

### Task 5: Migrate Search Box to akar TextInput

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 3 hours

**Description:** Replace sugacode's custom search box (text-buffer rendering, no actual text input) with akar's `text_input` component, which provides real keyboard text capture, cursor blinking, and focus management.

**Details:**

1. Replace `SearchBox` struct with akar-compatible state:
   ```rust
   // Keep:
   pub position: Vec2,  // auto-centered at bottom
   pub size: Vec2,      // 500x50

   // Replace is_focused/cursor_visible/cursor_timer with:
   pub value: String,
   pub cursor_pos: usize,
   pub cursor_visible: bool,  // blink timer managed externally
   pub cursor_timer: f32,

   // Add akar layout node:
   pub search_node: NodeId,
   ```

2. Replace `SearchBox::render()` with akar's `text_input`:
   ```rust
   // Position search box at bottom center using layout
   let search_rect = [
       (window_size.x - 500.0) / 2.0,
       window_size.y - 70.0,
       500.0,
       50.0,
   ];
   layout.set_style(search_node, Style {
       position: Position::Absolute,
       left: Val::Px(search_rect[0]),
       top: Val::Px(search_rect[1]),
       size: Size { width: Val::Px(500.0), height: Val::Px(50.0) },
       ..default()
   });

   let resp = text_input(
       &mut core, &layout, search_node,
       &mut state.search_query, &mut state.cursor_pos,
       "Search documents... (Cmd+K)",
       state.cursor_visible,
       &theme,
   );

   if resp.submitted {
       execute_search(state);
   }
   ```

3. Wire up keyboard shortcuts:
   - `Cmd+K`: Activate commit search, focus the text input
   - `Cmd+Shift+K`: Activate code search, focus the text input
   - `Escape`: Deactivate search, clear results
   - These shortcuts remain in the event loop (before `text_input` is called), not inside the component

4. Fix the existing bug: sugacode's search box never captures typed characters. akar's `text_input` handles `core.input.chars` and `core.input.keys_pressed` natively, so typing works out of the box.

5. Migrate search execution logic (indexer calls, result container creation) — this is business logic in `SearchBox::update()` and is preserved as-is.

6. Add search mode indicator:
   - Blue top border for commit search: use `container_component` with accent border
   - Green top border for code search: use `container_component` with success border
   - Or use akar's `badge` component to show "Commits" / "Code" mode label

**Acceptance Criteria:**
- [ ] Search box renders at bottom center with proper styling
- [ ] Cmd+K opens commit search with focus
- [ ] Cmd+Shift+K opens code search with focus
- [ ] Typing characters actually updates the search query (fixes existing bug)
- [ ] Cursor blinks in the text input
- [ ] Escape dismisses search and clears results
- [ ] Real-time search execution on query change
- [ ] Search result container appears on canvas with results
- [ ] Visual indicator distinguishes commit vs code search mode

---

### Task 6: Migrate Application Shell and Render Loop

**Priority:** High
**Status:** ⬜ Not Started
**Estimated Time:** 3 hours

**Description:** Restructure `main.rs` to use akar's frame lifecycle and layout system. Replace the custom `UIManager` orchestration with a flat immediate-mode render function.

**Details:**

1. Restructure the `RedrawRequested` handler:
   ```rust
   fn handle_redraw(&mut self) {
       let core = self.core.as_mut().unwrap();
       let state = &mut self.state;

       core.begin_frame(
           state.window_size.x,
           state.window_size.y,
           state.scale_factor,
       );

       // Build layout tree for this frame
       let mut layout = Layout::new();

       // 1. Canvas (full window)
       let canvas_node = layout.new_leaf(full_screen_style());
       layout.compute(canvas_node, (state.window_size.x, state.window_size.y), None);

       // 2. Canvas rendering (grid, containers, cards)
       render_canvas(&mut core, &mut layout, canvas_node, state);

       // 3. Drawer (overlays on left)
       if state.drawer_open {
           render_drawer(&mut core, &mut layout, state);
       }

       // 4. Search box (overlays at bottom)
       if state.search_active || state.code_search_active {
           render_search(&mut core, &mut layout, state);
       }

       // End frame — flush draw list to GPU
       let mut surface = self.surface.get_current_texture()?;
       let view = surface.texture.create_view(&..);
       let mut encoder = self.device.create_command_encoder(&..);
       {
           let mut pass = encoder.begin_render_pass(&..);
           core.end_frame(&self.device, &self.queue, &mut pass);
       }
       self.queue.submit(encoder.finish());
       surface.present();
   }
   ```

2. Replace `UIManager` with direct render functions:
   - `render_canvas(core, layout, canvas_node, state)` — canvas + grid + containers + cards
   - `render_drawer(core, layout, state)` — drawer + folder list
   - `render_search(core, layout, state)` — search input + results
   - Each function is self-contained and immediate-mode

3. Preserve `UIManager::update` logic (card hover state updates) by moving it into `render_canvas` — after `canvas_end`, iterate containers and update `card.is_hovered` using `core.input.is_hovering(screen_rect)`.

4. Wire up `akar_winit::process_window_event` in the event handler:
   ```rust
   fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
       let core = self.core.as_mut().unwrap();
       akar_winit::process_window_event(&mut core.input, &event);

       match event {
           WindowEvent::RedrawRequested => self.handle_redraw(),
           WindowEvent::Resized(size) => { /* resize logic */ },
           WindowEvent::CloseRequested => { /* exit */ },
           _ => {}
       }
       self.window.request_redraw();
   }
   ```

5. Handle keyboard shortcuts (Cmd+K, Escape) by checking `core.input.keys_pressed` and `core.input.chars` at the top of the frame, before component functions run.

**Acceptance Criteria:**
- [ ] `UIManager` struct deleted; render is flat immediate-mode functions
- [ ] Frame lifecycle uses `core.begin_frame` / `core.end_frame`
- [ ] Layout tree built per-frame (no retained widget state)
- [ ] akar-winit bridge processes all window events
- [ ] Keyboard shortcuts (Cmd+K, Cmd+Shift+K, Escape) work
- [ ] All UI layers render in correct order: canvas → containers → drawer → search
- [ ] `cargo check` passes clean

---

### Task 7: Preserve CLI Modes and Indexer Integration

**Priority:** Medium
**Status:** ⬜ Not Started
**Estimated Time:** 1 hour

**Description:** Ensure all existing CLI modes continue to work after the migration. The indexer crate and git_log module are untouched, but their integration points (argument parsing, indexer initialization, search execution) must be verified.

**Details:**

1. Verify these CLI modes still work:
   - `cargo run` — launches GUI with default repo (current directory)
   - `cargo run -- --repo ~/some-repo` — opens specific repo
   - `cargo run -- --repo . --index` — indexes commits into search DB
   - `cargo run -- --repo . --search "fix crash"` — CLI hybrid search (no GUI)
   - `cargo run -- --repo . --index-code` — indexes Rust source code
   - `cargo run -- --repo . --search-code "render pipeline"` — CLI code search
   - `RUST_LOG=debug cargo run` — debug logging

2. Verify indexer integration:
   - Indexer initializes on startup (same as current)
   - `SearchBox::update` calls `indexer.search_hybrid` and `indexer.search_code_hybrid`
   - Results populate containers correctly

3. No changes expected to `sugacode-indexer` crate — it is independent.

**Acceptance Criteria:**
- [ ] All CLI modes listed above produce correct output
- [ ] `--index` and `--search` work without launching GUI
- [ ] GUI mode shows indexed search results correctly
- [ ] `sugacode-indexer` crate compiles without changes

---

### Task 8: Visual Polish and Parity

**Priority:** Medium
**Status:** ⬜ Not Started
**Estimated Time:** 3 hours

**Description:** Bring the migrated UI to visual parity with (or improvement over) the original. akar's proper quad rendering with borders, shadows, and corner radii should make the UI look better than the text-buffer approximation.

**Details:**

1. **Card styling:**
   - Use `BoxStyle::card(theme)` for document cards (border + shadow)
   - Use `BoxStyle::panel(theme)` for git commit cards (subtle border)
   - Selection: accent border or tinted fill via `theme.primary` with low alpha
   - Hover: `BoxStyle::surface(theme)` or lighter fill

2. **Container styling:**
   - Use `BoxStyle::surface(theme)` for container backgrounds
   - Border: `theme.base_300` at 1px
   - Corner radii: `theme.radius_box` (12px)

3. **Drawer styling:**
   - akar's drawer provides scrim + panel with corner radii and shadow
   - Folder items: `button` with `Ghost` variant
   - Active folder: `button` with `Solid` variant or accent background

4. **Search box styling:**
   - akar's `text_input` provides themed background, border, cursor
   - Mode indicator: colored top border or `badge` component

5. **Theme:**
   - Use `AKAR_THEME_DARK` as the default (matches current dark theme)
   - Map sugacode's hardcoded colors to theme tokens:
     - `rgba(40,40,40,200)` → `theme.base_200`
     - `rgba(0,122,255,100)` → `theme.primary` with alpha
     - `rgb(100,200,255)` (cyan hash) → `theme.info`

6. **Screenshot verification:**
   ```bash
   cargo run --release -- --screenshot /tmp/sugacode-akar.png --exit
   ```
   Use akar's screenshot utility (Epic 013) to capture and verify the migrated UI.

**Acceptance Criteria:**
- [ ] Cards have proper rounded corners, borders, and shadows
- [ ] Container backgrounds are themed consistently
- [ ] Drawer has scrim + panel with proper styling
- [ ] Search box has themed input field with cursor
- [ ] Selection and hover states are visually distinct
- [ ] Screenshot captures match expected layout
- [ ] UI looks better than the text-buffer original

---

### Task 9: Update Documentation

**Priority:** Low
**Status:** ⬜ Not Started
**Estimated Time:** 1 hour

**Description:** Update DEVELOP.md and README.md to reflect the akar migration.

**Details:**

1. Update `DEVELOP.md`:
   - Update dependency table: remove direct wgpu/glyphon/winit/glam, add akar crates
   - Update project structure: remove `renderer.rs`, `input.rs`; update `ui/` contents
   - Update architecture notes: describe akar integration pattern
   - Add akar to "Other relevant dependencies cloned locally"

2. Update `README.md`:
   - Update technology stack section
   - Update project structure
   - Update controls (should be mostly the same)
   - Note that the UI is now rendered with akar

3. Update `epics/001-text-explorer-ui-prototype.md` status if needed.

**Acceptance Criteria:**
- [ ] DEVELOP.md reflects akar-based architecture
- [ ] README.md updated with new stack and structure
- [ ] All references to removed files are cleaned up

---

## Implementation Order

1. **Task 1:** Add akar dependencies, remove old renderer (2 hours)
2. **Task 2:** Migrate canvas (grid, zoom, pan) (3 hours)
3. **Task 3:** Migrate drawer (2 hours)
4. **Task 4:** Migrate container and card rendering (4 hours)
5. **Task 5:** Migrate search box to text_input (3 hours)
6. **Task 6:** Restructure application shell and render loop (3 hours)
7. **Task 7:** Verify CLI modes and indexer (1 hour)
8. **Task 8:** Visual polish and parity (3 hours)
9. **Task 9:** Update documentation (1 hour)

**Total Estimated Time:** ~22 hours

---

## File Changes Summary

| File | Action | Description |
|---|---|---|
| `Cargo.toml` | **Modify** | Replace wgpu/glyphon/winit/glam with akar crates |
| `src/main.rs` | **Modify** | Use AkarCore, akar-winit, flat render functions |
| `src/renderer.rs` | **Delete** | Replaced by AkarCore |
| `src/input.rs` | **Delete** | Replaced by akar_winit::process_window_event |
| `src/state.rs` | **Modify** | Replace canvas fields with CanvasState |
| `src/git_log.rs` | **Preserve** | No changes |
| `src/ui/mod.rs` | **Rewrite** | UIManager → flat render functions using akar components |
| `src/ui/canvas.rs` | **Delete** | Replaced by akar's canvas_begin/end |
| `src/ui/drawer.rs` | **Delete** | Replaced by akar's drawer_begin/end |
| `src/ui/container.rs` | **Modify** | Keep data model; remove rendering code |
| `src/ui/search.rs` | **Delete** | Replaced by akar's text_input |
| `crates/sugacode-indexer/` | **Preserve** | No changes |

---

## Success Criteria

The migration is complete when:
1. `cargo run` launches the GUI with all UI elements rendered via akar
2. Infinite canvas with zoom (0.1x-5x) and pan (middle mouse / Cmd+click) works
3. Left drawer with folder icons, hover, selection, expand/collapse works
4. Git log container renders commit cards with scrolling
5. Search box accepts typed input (fixes existing bug) and executes hybrid search
6. All CLI modes (`--repo`, `--index`, `--search`, `--index-code`, `--search-code`) work
7. UI has proper quad rendering: borders, shadows, corner radii (visual improvement)
8. `cargo check --workspace` passes clean
9. No direct wgpu/glyphon/winit imports in sugacode's Cargo.toml
10. Screenshot utility works: `cargo run --release -- --screenshot /tmp/sugacode.png --exit`

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| akar's canvas does not support `push_text` in world space | Grid coordinate labels cannot be rendered | Use `push_quad` for grid lines only; coordinate labels deferred to akar Epic 015+ |
| akar is pre-alpha; API may change | Breaking changes during migration | Pin to specific commit; path dependency allows local fixes |
| Layout tree rebuilt per-frame may be slower than retained | Performance regression with many cards | Use `list_clip` to minimize draw calls; profile early |
| akar's `text_input` may not support Cmd+K shortcut pre-focus | Search activation flow differs | Wire shortcuts before component call; set `focused_id` manually |
| Drawer animation timing differs (frame-rate vs delta-time) | Animation feels different | Use delta-time-based animation; may need `Instant` tracking |
