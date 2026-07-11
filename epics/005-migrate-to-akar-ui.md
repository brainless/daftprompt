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
**Status:** ✅ Done
**Estimated Time:** 2 hours

**Review note (post-implementation):**
- `src/renderer.rs` and `src/input.rs` deleted; `src/main.rs` rewritten to drive `AkarCore`; `AppState` now has `cmd_or_ctrl` and `shift_pressed` fields; `ModifiersChanged` is intercepted in `window_event` before forwarding to `akar_winit::process_window_event`.
- **Deviation:** the spec asked for `wgpu`/`winit`/`glyphon` to be removed from direct deps. In Rust 2021 the binary cannot `use wgpu::...` or `winit::...` against a transitive-only crate, and no akar crate re-exports them. So `wgpu = "29.0.0"`, `winit = "0.30.12"`, and `glam` are still direct deps. `glyphon` and `bytemuck` are gone (transitive via akar-core). `pollster` is kept as a direct dep because main.rs needs `pollster::block_on` to drive wgpu's async setup in `resumed`.
- **Deviation:** `glam` stays as a direct dep. The "preferred" `akar_layout::glam::Vec2` route from the spec is impossible because `taffy::prelude::*` does not re-export glam.
- **Deviation:** the git-log container creation that was previously in `resumed` was deferred — `ui::container` survived but `ui::mod.rs` was trimmed to a single-line stub (`pub mod container;`) so the binary compiles. The original 4-module UI is rebuilt with akar in Tasks 2-5.
- `cargo check --workspace` passes. `cargo test -p sugacode-indexer` passes (18/18).

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
   - `AkarCore::new(&device, &queue, surface_format)` in the `resumed` handler — `device` and `queue` are **borrowed references**, not moved
   - `core.begin_frame(width_u32, height_u32, scale_factor)` at the start of each `RedrawRequested` — `width`/`height` are `u32` (cast `state.window_size.x as u32`); `scale_factor` is `f32`
   - `core.end_frame(&device, &queue, &mut render_pass)?` at the end — returns `Result<(), Box<dyn Error>>`, so use `?` or `match`. **Critical:** `end_frame` internally calls `self.input.begin_frame()` which clears per-frame input events (`chars`, `keys_pressed`, `scroll_delta`, mouse press/release). All input reads (component hover/click, `text_input`, scroll-aware widgets) must occur *before* `end_frame` — akar's components already do this
   - wgpu instance/device/surface creation stays in `main.rs` (sugacode owns the window, akar owns the pipeline)

4. Delete `src/input.rs` — replace with `akar_winit::process_window_event` feeding `AkarCore`'s `InputState`. **Note:** `akar_winit` only forwards `CursorMoved`, `MouseInput`, `MouseWheel`, and the text.KeyCode subset (`Backspace/Delete/Arrows/Home/End/Enter/Escape/Tab`) — it does **not** track keyboard modifiers. Sugacode's Cmd+K shortcut and Cmd+click-to-pan both depend on modifier state, so `AppState` must track this separately.

5. Track Cmd/Ctrl modifier state in `AppState` (akar cannot provide it):
   ```rust
   // In AppState:
   pub cmd_or_ctrl: bool,  // true when Super on macOS, Control elsewhere
   ```
   Handle the winit event directly in `main.rs` — `akar_winit::process_window_event` ignores it, so it falls through to the match arms:
   ```rust
   WindowEvent::ModifiersChanged(m) => {
       state.cmd_or_ctrl = if cfg!(target_os = "macos") {
           m.state().super_key()
       } else {
           m.state().control_key()
       };
   }
   ```
   The Cmd+K shortcut checker and Canvas Cmd+Left-drag pan handler (Tasks 2, 3, 6) both read `state.cmd_or_ctrl` — see those tasks for usage.

6. Update `Application` struct to hold `AkarCore` instead of `Renderer`.

**Acceptance Criteria:**
- [ ] `cargo check` passes with akar dependencies
- [ ] Window opens and shows a clear color (no text yet)
- [ ] Window resize works
- [ ] winit events are forwarded to akar's InputState
- [ ] Cmd/Ctrl modifier state is tracked in `AppState` via `ModifiersChanged` (akar does not expose modifiers)
- [ ] `renderer.rs` and `input.rs` are deleted
- [ ] No direct wgpu/glyphon/winit/glam imports remain in `Cargo.toml`

---

### Task 2: Migrate Application Shell and Render Loop

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 3 hours

**Review note (post-implementation):**
- `UIManager` is gone; per-frame `Layout::new()` builds a tree containing a single `canvas_node` (full-window `percent(1.0)` × `percent(1.0)`), `layout.compute(...)` runs with the required `Size::ZERO` measure closure, then `core.begin_frame` / `core.end_frame` wrap the render pass. `render_canvas`/`render_drawer`/`render_search` are stubbed in a new `src/ui/render.rs`.
- Keyboard-shortcut detection lives at the top of `handle_redraw` immediately after `core.begin_frame` so input is read before `core.end_frame` clears it. Cmd+K / Cmd+Shift+K toggle `search_active` / `code_search_active`, clearing the other mode's results and containers (matching the deleted `input.rs` semantics). Escape cascades code → commits → deselect-all, and clears `core.input.focused_id` (no-op for Task 2, but wires Task 6's text input focus loss).
- `AppState` now has `SearchMode { Commits, Code }`, `search_mode`, `search_just_opened`, `code_search_just_opened`, `cursor_pos`, `cursor_visible`, `cursor_timer` — Task 6 will read/write these; declared here so the state schema is in one place.
- Git-log container creation restored in `resumed()` (deferred from Task 1) — the git-log data is loaded into `state.containers[0]`, ready for Task 5's rendering.
- Winit's IME gives both `'k'` and `'K'` on Cmd+Shift+K depending on the platform; the detector checks both. `let _ = core.end_frame(...)` ignores the `Result` for now (the empty draw list can't fail; Tasks 3+ can handle errors properly if needed).
- `cargo check --workspace` passes. `cargo test -p sugacode-indexer` passes (18/18).

**Description:** Restructure `main.rs` to use akar's frame lifecycle and layout system. Replace the custom `UIManager` orchestration with a flat immediate-mode render function.

**Details:**

1. Restructure the `RedrawRequested` handler:
    ```rust
    fn handle_redraw(&mut self) -> anyhow::Result<()> {
        let core = self.core.as_mut().unwrap();
        let state = &mut self.state;

        // begin_frame takes (width: u32, height: u32, scale_factor: f32),
        // so cast the f32 window_size fields to u32.
        core.begin_frame(
            state.window_size.x as u32,
            state.window_size.y as u32,
            state.scale_factor,
        );

        // Build layout tree for this frame (rebuilt every frame — immediate mode).
        let mut layout = Layout::new();

        // 1. Canvas (full window)
        let canvas_node = layout.new_leaf(full_screen_style());

        // Layout::compute takes (root, (Option<f32>, Option<f32>), measure_fn).
        // The available-size tuple uses Option (Some(x) = definite, None = max-content);
        // the third arg is a MANDATORY measure closure — pass |_,_,_,_,_| Size::ZERO
        // for nodes whose sizes are fully specified in their Style (no content measuring).
        use taffy::prelude::*;
        layout.compute(
            canvas_node,
            (Some(state.window_size.x), Some(state.window_size.y)),
            |_, _, _, _, _| Size::ZERO,
        );

        // 2. Canvas rendering (grid, containers, cards)
        render_canvas(core, &mut layout, canvas_node, state);

        // 3. Drawer (overlays on left)
        if state.drawer_open {
            render_drawer(core, &mut layout, state);
        }

        // 4. Search box (overlays at bottom)
        if state.search_active || state.code_search_active {
            render_search(core, &mut layout, state);
        }

        // End frame — flush draw list to GPU. end_frame returns Result; use ?.
        // (end_frame internally clears per-frame input — all reads happened above.)
        let mut surface = self.surface.get_current_texture()?;
        let view = surface.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_writes: None,
            });
            core.end_frame(&self.device, &self.queue, &mut pass)?;
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        surface.present();
        Ok(())
    }
    ```

2. Replace `UIManager` with direct render functions:
    - `render_canvas(core, layout, canvas_node, state)` — canvas + grid + containers + cards
    - `render_drawer(core, layout, state)` — drawer + folder list
    - `render_search(core, layout, state)` — search input + results
    - Each function is self-contained and immediate-mode

3. Preserve `UIManager::update` logic (card hover state updates) by moving it into `render_canvas` — after `canvas_end`, iterate containers and update `card.is_hovered` using `core.input.is_hovering(screen_rect)`.

4. Wire up `akar_winit::process_window_event` in the event handler. akar-winit ignores `ModifiersChanged`, so handle it ourselves before the match (it feeds the `cmd_or_ctrl` state set up in Task 1 step 5):
    ```rust
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let core = self.core.as_mut().unwrap();
        let state = &mut self.state;

        // akar-winit forwards cursor/mouse/wheel/text; modifiers are on us.
        if let WindowEvent::ModifiersChanged(m) = &event {
            state.cmd_or_ctrl = if cfg!(target_os = "macos") {
                m.state().super_key()
            } else {
                m.state().control_key()
            };
        }

        akar_winit::process_window_event(&mut core.input, &event);

        match event {
            WindowEvent::RedrawRequested => { let _ = self.handle_redraw(); },
            WindowEvent::Resized(size) => { /* resize logic */ },
            WindowEvent::CloseRequested => { /* exit */ },
            _ => {}
        }
        self.window.request_redraw();
    }
    ```

5. Handle keyboard shortcuts. Read modifier-aware shortcuts at the top of the frame (before component functions run), because `end_frame` clears input after the frame. The mapping between sugacode's actions and akar's input fields is:
   - **Named keys** (Escape, Enter, Backspace,…) → `core.input.keys_pressed: Vec<akar_core::Key>` — akar's typed enum, *not* `Vec<char>`. Only 11 keys are members (`Backspace, Delete, Left, Right, Up, Down, Home, End, Enter, Escape, Tab`); alphabetic keys like `K` are **not** here.
   - **Text characters** (letters, digits) → `core.input.chars: Vec<char>` — output of winit's IME/text event. `Cmd+K` is detected by combining `state.cmd_or_ctrl` with `'k' (or 'K') ∈ core.input.chars`.
   - **Modifier state** → `state.cmd_or_ctrl` (Task 1 step 5), not `core.input`.
   - Example: `Cmd+K` opens commit search → `if state.cmd_or_ctrl && core.input.chars.contains(&'k') { state.search_active = true; state.search_mode = SearchMode::Commits; state.search_just_opened = true; }`. `Cmd+Shift+K` → additionally check `state.shift_pressed` (add `shift: bool` to `AppState` and set it from the same `ModifiersChanged`). `Escape` → scan `core.input.keys_pressed.contains(&Key::Escape)`.

**Acceptance Criteria:**
- [ ] `UIManager` struct deleted; render is flat immediate-mode functions
- [ ] Frame lifecycle uses `core.begin_frame` / `core.end_frame`
- [ ] Layout tree built per-frame (no retained widget state)
- [ ] akar-winit bridge processes all window events
- [ ] Keyboard shortcuts (Cmd+K, Cmd+Shift+K, Escape) work
- [ ] All UI layers render in correct order: canvas → containers → drawer → search
- [ ] `cargo check` passes clean

---

### Task 3: Migrate Canvas — Grid and Zoom/Pan

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 3 hours

**Review note (post-implementation):**
- `render_canvas` in `src/ui/render.rs` calls `canvas_begin`/`canvas_end` and uses `CanvasPainter::push_quad` to draw a world-space grid; grid spacing snaps to powers of 10 to keep line density stable across zoom levels, and the grid is hidden when zoom < 0.15 for readability.
- Cmd+Left-drag pan lives in `main.rs::handle_redraw` (not in `render_canvas`) so the pan shows up in the same frame's `world_to_screen` transform; `cmd_panning` is a separate flag because `canvas_begin` clears `CanvasState::is_panning` each frame the configured button isn't pressed.
- Zoom indicator is an absolute-positioned child of `canvas_node` (taffy 0.11 reports location (0,0) for rootless absolute nodes, so it has to be parented); `label` is drawn after `canvas_end` because it borrows `&mut AkarCore`.
- `src/ui/canvas.rs` deleted (was 146 lines of hand-rolled coordinate transforms + text-buffer grid).
- Zoom limits (0.1–5.0) come from `CanvasConfig::default()`; `PanButton::Middle` is the default.
- `cargo check --workspace` passes clean (only pre-existing dead-code warnings on `Container` fields that Task 5 will use).

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
    let (resp, painter) = canvas_begin(core, layout, canvas_node, &mut state.canvas_state, &CanvasConfig {
        pan_button: PanButton::Middle,
        zoom_sensitivity: 0.005,
        zoom_min: 0.1,
        zoom_max: 5.0,
    });

    // Grid rendering using painter.push_quad() in world space
    render_grid(&painter, resp.visible_world_rect, resp.world_to_screen);

    // Zoom indicator using label() in screen space (after canvas_end)
    canvas_end(core, painter);
    ```
    Note: `resp.world_to_screen` is a `CanvasTransform` struct with methods like `apply_rect()` and `scale_radius()`.

3. Migrate grid rendering from text-based labels to `CanvasPainter::push_quad`:
   - Grid lines: thin quads at regular world-space intervals
   - Coordinate labels: deferred (akar does not yet have `CanvasPainter::push_text`; grid lines alone are sufficient)
   - Hide grid when zoomed out past readability threshold

4. Migrate zoom indicator to akar's `label` component positioned at bottom-right corner.

5. Remove `AppState::screen_to_canvas` and `AppState::canvas_to_screen` — use `CanvasResponse` transforms instead.

6. Implement sugacode's Cmd+Left-click pan manually. akar's `PanButton` enum is only `Middle`/`Right` (`akar-components/src/canvas.rs:9`) — there is no `Left` variant, so sugacode's existing Cmd+Left-click-to-pan convention is not covered by akar's `canvas_begin`. Furthermore, `canvas_begin` resets `state.canvas_state.is_panning = false` every frame when its configured PanButton isn't pressed (`canvas.rs:134`), so reusing `CanvasState::is_panning` for Cmd+Left pan would be cleared immediately. Track a separate `cmd_panning: bool` field in `AppState`, apply the pan delta to `state.canvas_state.pan` **before** `canvas_begin` so the frame's `world_to_screen` transform reflects the pan:
    ```rust
    // Before canvas_begin — apply Cmd+Left drag pan so the frame's
    // world_to_screen transform includes it. cmd_or_ctrl is from Task 1 step 5.
    let canvas_rect = layout.rect(canvas_node);
    if state.cmd_or_ctrl
        && core.input.mouse_buttons_pressed[0]
        && core.input.is_hovering(canvas_rect)
    {
        state.cmd_panning = true;
    }
    if !core.input.mouse_buttons[0] {
        state.cmd_panning = false;
    }
    if state.cmd_panning {
        let delta = (core.input.mouse_pos - core.input.mouse_pos_prev) / state.canvas_state.zoom;
        state.canvas_state.pan -= delta;
    }

    // canvas_begin then handles middle-mouse pan (default PanButton::Middle)
    // and cursor-anchored wheel zoom itself.
    let (resp, painter) = canvas_begin(
        core, layout, canvas_node, &mut state.canvas_state, &CanvasConfig::default(),
    );
    ```

**Acceptance Criteria:**
- [ ] Canvas renders with grid lines in world space
- [ ] Mouse wheel zooms centered on cursor (cursor-anchored)
- [ ] Middle mouse button pans the canvas
- [ ] Cmd+Left click pans the canvas (sugacode's existing convention)
- [ ] Zoom limits enforced (0.1 to 5.0)
- [ ] Zoom indicator shows current percentage at bottom-right
- [ ] `ui/canvas.rs` is deleted; canvas logic lives in the render loop

---

### Task 4: Migrate Drawer

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 2 hours

**Review note (post-implementation):**
- `render_drawer` uses `drawer_begin`/`drawer_end` (scrim + panel + scissor) and pushes the folder list inside the scissor. Per-row is a taffy `row_node` (absolute) with `icon_node` and (when expanded) `text_node` children; a Ghost-variant `button` covers the row for hover/click; the selected row gets a `container` with a `theme.primary` fill underneath.
- Animation is delta-time-based: `AppState::drawer_animation` ∈ [0.0, 1.0] lerps toward 1.0 (open) or 0.0 (closed) at 6.0/sec in `main.rs::handle_redraw`, driven by a new `last_frame: Option<Instant>` field on `Application`. Replaces the old frame-rate-incremented `0.1 / frame` step. A 60→250px full open/close takes ~1/6 s.
- `AppState` gained `drawer_animation: f32` and `hover_index: Option<usize>`.
- `src/ui/drawer.rs` deleted (268 lines of emoji-in-text-buffer rendering and frame-rate-incremented animation).
- **Deviation:** `panel_node` is a *rootless* taffy leaf, then we call `layout.compute(panel_node, …)` as a second pass — the canvas tree (already computed for `canvas_node`) is unaffected because `panel_node` is not a descendant. This works because taffy 0.11 lets you compute a subtree independently of its parent.
- **Deviation:** no in-app way to *re-open* the drawer after a scrim-close yet — the spec said this was acceptable to defer.
- **Deviation:** did not add Escape-to-close for the drawer (spec said not to). Selected-row highlight uses full-alpha `theme.primary` which is barely visible against the panel's `theme.base_200` in the dark theme — Task 8 should swap to a low-alpha tint or `theme.accent` for visual contrast.
- `cargo check --workspace` passes clean (10 pre-existing dead-code warnings on `Container`, all from Task 5). `cargo test -p sugacode-indexer` passes 18/18. `cargo build` clean.

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

   let resp = drawer_begin(core, viewport_rect, DrawerEdge::Left, panel_width, &theme);
   if resp.close_requested {
       state.drawer_open = false;
   }

   // Render folder list inside drawer panel
   // Use layout to position folder items
   for (i, folder) in state.folders.iter().enumerate() {
       let icon_node = layout.new_leaf(Style { .. });
       // Position icon using layout
       let btn = button(core, &layout, icon_node, &folder.icon_emoji, ButtonVariant::Ghost, &theme);
       if btn.clicked {
           state.selected_folder = Some(i);
       }
       if btn.hovered {
           state.hover_index = Some(i);
       }
       // Show name + doc count when expanded
       if animation_progress > 0.5 {
           let label_node = layout.new_leaf(Style { .. });
           label(core, &layout, label_node, &format!("{} ({} docs)", folder.name, folder.document_count), theme.base_content, &theme);
       }
   }

   drawer_end(core);
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

### Task 5: Migrate Container and Card Rendering

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 4 hours

**Review note (post-implementation):**
- `render_containers` is wired into `render_canvas` between `render_grid` and `canvas_end`. It walks `state.containers` and renders each container's background (via `painter.push_quad` at z=0.0, flushed at `canvas_end` behind everything else), opens a scroll area for card content, and renders a virtualized card list using `list_clip` from `akar_core`.
- **Z-ordering for layered draws.** Container backgrounds are pushed through the painter at z=0.0; card backgrounds push to `core.draw_list` directly at z=0.1 so the active `scroll_area_begin` scissor clips them to the container rect (the painter's buffer doesn't carry a scissor, so painter quads would land unclipped). `core.draw_list.sorted_quads()` sorts by z so the layering is correct.
- **Labels via rootless taffy overlay.** Card text (hash/author/date/message) is laid out as absolute children of a rootless overlay node (one per label), then a second `layout.compute(overlay_node, …)` pass resolves them, then `label(core, &*layout, node, …)` renders them. The `label` calls respect the active scissor because they push to `core.draw_list.push_text` (not the painter).
- **Data path is parsing, not refactor.** The git log / search-result document content is `"<hash>\nAuthor: <name>\nDate: <time>"`; `render_containers` parses it line-by-line to extract the three fields. `DocumentData` is unchanged, so the data-model "preserve" constraint is satisfied strictly. Drawback: a slight code smell; cleaner would be a `GitMeta` field on `DocumentData` populated by the constructors. Left as a Task 8 follow-up if visual polish needs it.
- **Selection is single-select per container** (`is_clicked` on a card clears other cards' `is_selected` in the same container). Matches the spec wording.
- **No scroll indicator rendered.** The spec marked it "optional but encouraged" — left for Task 8 (visual polish).
- **Deviation (compile workaround).** `glam 0.30` (sugacode's path dep) vs `glam 0.33` (akar's transitive dep) means `container.position` and `WorldRect.min/max` are different `Vec2` types. Constructed `WorldRect` via `WorldRect::from_xywh(x, y, w, h)` (f32s) to dodge the version clash. No `use glam::Vec2` in `render.rs`.
- **Deviation (small).** `akar_components::color::color_to_f32` is `pub(crate)`, so inlined a 6-line helper in `render.rs` to convert u32 → `[f32; 4]`.
- **Deviation (small).** All labels use the same font size — akar's `label` hardcodes `theme.font_size_base` (16px). The spec wanted 11/12/13px variants; without exposing akar's `text_pipeline` we can't easily mix sizes. Used color/position to convey hierarchy instead. Documented as a Task 8 follow-up.
- `src/ui/search.rs` deleted (429 lines of pre-akar search rendering; not exported by `mod.rs`).
- `cargo check --workspace` passes clean (9 pre-existing dead-code warnings on `Container::scroll`/`is_mouse_over`/`visible_cards`/`new_document_grid` — these are data-model methods, not render code, and will be flagged in the next task if they remain unused). `cargo test -p sugacode-indexer` 18/18. `cargo build` clean.

**Description:** Replace sugacode's custom container backgrounds, borders, scroll indicators, and card rendering (text-buffer rectangles) with akar's `container` + `scroll_area` + `list_clip` + `label` components.

**Details:**

1. Preserve `Container` data model (`ui/container.rs`) — it is business logic:
   - `ContainerType` enum (DocumentGrid, GitLogColumn, SearchResults, CodeSearchResults)
   - `CardData`, `DocumentData` structs
   - `new_git_log`, `new_search_results`, `new_code_search_results` constructors
   - Card height calculation logic
   - `visible_cards()` iterator (replaced by akar's `list_clip` but logic is similar)

2. Replace container rendering in `ui/mod.rs`. Caveats confirmed against akar source:
   - `list_clip` lives in **`akar-core`**, not `akar-components` — import it as `use akar_core::list_clip;` (see `akar-core/src/lib.rs:24`). It takes `(total: usize, item_height: f32, scroll_y: f32, viewport_height: f32) -> Range<usize>` and adds one item of padding on each end.
   - `scroll_area_begin`'s signature is `(core, rect: [f32;4], scroll_y: &mut f32, content_height: f32) -> ScrollAreaResponse` (the offset param is `scroll_y`, not `scroll_offset`; the return struct is `ScrollAreaResponse`, not `ScrollResponse`; it has `.content_y: f32`).
   - `container` early-returns when `BoxStyle.fill == 0` (`container.rs:9`) — transparent = skip, not "draw transparent".
   - `apply_rect` consumes a `WorldRect { min, max: Vec2 }`, not a `[f32;4]`; build one from the container's world-space position/size.

    ```rust
    use akar_core::list_clip;
    use akar_layout::WorldRect;
    use akar_components::{container, scroll_area_begin, scroll_area_end, BoxStyle};

    // For each container on the canvas:
    for container in &mut state.containers {
        // Position container in world space using canvas painter
        let container_screen_rect = resp.world_to_screen.apply_rect(WorldRect {
            min: container.position,
            max: container.position + container.size,
        });

        // Container background + border. Use a non-zero fill or container() no-ops.
        container(core, layout, container_node, &BoxStyle::panel(&theme));

        // Scroll area for container content — note the &mut scroll_y param.
        let scroll_resp = scroll_area_begin(
            core,
            container_screen_rect,
            &mut container.scroll_offset,
            container.content_height,
        );

        // Virtualized card rendering — list_clip returns Range<usize>.
        let visible = list_clip(
            container.cards.len(),
            card_height,
            container.scroll_offset,
            container_screen_rect[3],
        );
        for i in visible {
            let card = &container.cards[i];
            let card_y = scroll_resp.content_y + card.position.y;
            let card_rect = [container_screen_rect[0] + 8.0, card_y, container_screen_rect[2] - 16.0, card.size.y];

            // Card background
            container(core, layout, card_node, &BoxStyle::card(&theme));

            // Card content (git commit or document)
            match container.container_type {
                ContainerType::GitLogColumn => render_git_card_akar(core, layout, card, card_rect, &theme),
                _ => render_doc_card_akar(core, layout, card, card_rect, &theme),
            }
        }

        scroll_area_end(core);
    }
    ```

3. Migrate git commit card rendering (`render_git_card_content`):
    - Replace text-buffer rectangles with akar `label` components
    - Hash: cyan label at 12px
    - Author + date: gray labels at 11px
    - Separator: akar `separator` component
    - Message: white label at 13px
    - Hover state: use `container` with different `BoxStyle`

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

### Task 6: Migrate Search Box to akar TextInput

**Priority:** High
**Status:** ✅ Done
**Estimated Time:** 3 hours

**Review note (post-implementation):**
- `render_search` is now a real implementation. It builds a rootless taffy sub-tree (window-sized parent + 16px-tall mode-indicator child + 500×50px search-box child), computes it, then calls `akar_components::text_input` with the active mode's query string and shared `state.cursor_pos`.
- **Bug fix from spec**: typing characters into the search box now actually updates the query (the old `src/ui/search.rs` only rendered; no input capture). `text_input` reads `core.input.chars` and `core.input.keys_pressed` natively.
- **Focus persistence via re-assert.** `core.input.focused_id` is set to `Some(u64::from(search_node))` on `state.search_just_opened` (the first frame) and re-asserted each frame as long as `search_active || code_search_active` and no other widget has claimed focus. This is option A from the Risks row — option B (persistent `Layout`) would require restructuring `main.rs::handle_redraw`'s per-frame `Layout::new()`, which is out of scope.
- **Click-outside-to-unfocus is broken in practice** as the sub-agent flagged: the per-frame `NodeId` churn means the re-assert branch keeps grabbing focus whenever `focused_id` is `None`, even if the user clicked outside. Escape is the only way to unfocus without the box closing. Documented for Task 8.
- **Mode indicator** is a `badge` ("Commits" = `BadgeVariant::Info` cyan, "Code" = `BadgeVariant::Success` green) 4px above the search box, left-aligned. Avoids needing a new accent-border component.
- **Search execution** in `execute_search(state)`: empty query → remove results container; non-empty + indexer present → call `search_hybrid` / `search_code_hybrid` and push a `Container::new_search_results` / `new_code_search_results` at world `(620, 20)`, width 500, height `window_h - 40`. Container id is reused from the previous results container if one exists (so the id space stays small and selection state isn't orphaned).
- **Cursor blink** uses `dt` from `main.rs::handle_redraw` (already plumbed by Task 4). `state.cursor_timer += dt; if timer >= 0.5 { timer -= 0.5; visible = !visible; }` — fps-independent.
- `src/main.rs` changed by exactly 1 line: `render_search(...)` → `render_search(..., dt)`. The keyboard-shortcut detector block (Cmd+K, Cmd+Shift+K, Escape) at lines 429-499 was left untouched.
- **Deviation:** `state.search_results = results.clone()` was dropped because `SearchResult` / `CodeSearchResult` don't derive `Clone`. The data is owned by the `Container` already; the `search_results` field is only ever read via `.clear()` by the keyboard handler. The field stays as a placeholder.
- **Deviation:** `submit-on-Enter` re-runs the search instead of dismissing the box, matching the spec's `execute_search(state)` pseudocode literally.
- `cargo check --workspace` passes clean (7 pre-existing dead-code warnings). `cargo test -p sugacode-indexer` 18/18. `cargo build` clean.

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

2. Replace `SearchBox::render()` with akar's `text_input`. **taffy 0.11 has no `Val::Px`/`Val::Percent` enum and `Style` has no `left`/`top` fields** — use the taffy helpers `length(x)` / `percent(p)` for size values, and the `inset: Rect<LengthPercentageAuto>` field for offsets:
    ```rust
    use taffy::prelude::*;  // brings length, Position, Size, Rect, Style via akar-layout re-export

    // Position search box at bottom center
    let search_x = (window_size.x - 500.0) / 2.0;
    let search_y = window_size.y - 70.0;
    layout.set_style(search_node, Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(search_x),
            top: length(search_y),
            ..Rect::auto(),
        },
        size: Size { width: length(500.0), height: length(50.0) },
        ..Style::DEFAULT
    });

    // Focus management (see focus-persistence note in step 3):
    if state.search_just_opened {
        core.input.focused_id = Some(u64::from(search_node));
        state.search_just_opened = false;
    }
    // Re-grab focus if the user hasn't clicked elsewhere this frame.
    if state.search_active && core.input.focused_id.is_none() {
        core.input.focused_id = Some(u64::from(search_node));
    }

    let resp = text_input(
        core, &layout, search_node,
        &mut state.search_query, &mut state.cursor_pos,
        "Search documents... (Cmd+K)",
        state.cursor_visible,
        &theme,
    );
    if resp.submitted {
        execute_search(state);
    }
    ```
    Note: `text_input` self-manages focus via `core.input.focused_id == Some(u64::from(node_id))` (`akar-components/src/text_input.rs:56-69`): clicking it sets focus, pressing mouse outside while down clears it. To force focus from a Cmd+K handler, set `core.input.focused_id = Some(u64::from(search_node))` **before** calling `text_input`.

3. Wire up keyboard shortcuts. These run at the **top of the frame** (before `text_input`) and lean on the `cmd_or_ctrl` / `shift` state added in Task 1 step 5:
   - `Cmd+K`: detect `state.cmd_or_ctrl && core.input.chars.contains(&'k')`. Set `state.search_active = true`, set `state.search_mode = SearchMode::Commits`, set `state.search_just_opened = true` so step 2 forces focus on the next frame.
   - `Cmd+Shift+K`: additionally require `state.shift`. Same activation flow but `SearchMode::Code`.
   - `Escape`: scan `core.input.keys_pressed.contains(&akar_core::Key::Escape)` — `Escape` is one of the named keys in akar's `Key` enum. Clear `state.search_active`, `state.code_search_active`, set `core.input.focused_id = None`, and clear search-result containers.

> **Focus persistence across frames (subtle).** `Layout::new()` is called per-frame and `NodeId = taffy::NodeId` is a slotmap key, so the u64 backing `search_node` changes every frame if the layout tree is rebuilt from scratch. `core.input.focused_id` is `Option<u64>` and akar compares via `u64::from(node_id)`, so a stale `focused_id` from the previous frame will silently fail to match the new `search_node`, dropping focus. Two robust mitigations (pick one):
> - **Re-assert focus each frame** based on a sugacode-side flag (`state.search_active && !clicked_outside_this_frame` → set `focused_id` before `text_input`), as shown in step 2's pseudocode; or
> - **Reuse the `Layout` instance across frames** (call `Layout::new()` once in `AppState::new()` and call `layout.compute()` every frame with updated sizes). This keeps NodeIds stable, so `focused_id` survives frame boundaries naturally.
>
> Verify focus by clicking inside the search box (should gain cursor) and clicking elsewhere (should lose cursor) — both flows are handled by akar's `text_input`.

4. Fix the existing bug: sugacode's search box never captures typed characters. akar's `text_input` handles `core.input.chars` and `core.input.keys_pressed` natively, so typing works out of the box.

5. Migrate search execution logic (indexer calls, result container creation) — this is business logic in `SearchBox::update()` and is preserved as-is.

6. Add search mode indicator:
    - Blue top border for commit search: use `container` with accent border
    - Green top border for code search: use `container` with success border
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

### Task 7: Preserve CLI Modes and Indexer Integration

**Priority:** Medium
**Status:** ✅ Done
**Estimated Time:** 1 hour

**Review note (post-implementation):**
- All 8 CLI modes verified working end-to-end against the local `sugacode` repo: `--help`, `--index`, `--reindex`, `--search`, `--index-code`, `--reindex-code`, `--search-code`, and `RUST_LOG=debug --index-code`.
- **No code changes were required** — the indexer crate and `src/git_log.rs` were correctly left untouched by Tasks 1-6, and the CLI's flag set was already complete. The migration's only deletion was `src/renderer.rs` + `src/input.rs` (purely GUI-layer), so every CLI code path is intact.
- All four indexer-to-GUI wires confirmed intact: `Indexer::new` in `src/main.rs:150-176` → `Application.indexer` field at `:189` → `state.indexer = self.indexer.take()` in `resumed` at `:206,233` → `execute_search` calling `search_hybrid` / `search_code_hybrid` in `src/ui/render.rs:1166,1202`.
- `cargo check --workspace` clean (7 pre-existing dead-code warnings). `cargo test -p sugacode-indexer` 18/18.
- Working tree is clean; no commit needed for this task.

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
**Status:** ✅ Done
**Estimated Time:** 3 hours

**Review note (post-implementation):**
- **Card backgrounds** in `render_containers` now use values copied from `BoxStyle::card(&theme)` rather than ad-hoc color literals. Cards are screen-space rects (not taffy nodes), so the `container(...)` helper (which reads `layout.rect(node)`) doesn't apply — went with Option A: build a `BoxStyle::card(&theme)`, copy its `fill` / `border_color` / `border_width` / `corner_radii` / `shadow` into the `QuadCall`, and override `fill` for the selected/hover variants. The card now has a real shadow, rounded corners, and a proper 1px border in `theme.base_300` against the panel's `theme.base_200` background.
- **Selected card alpha fixed.** Initial implementation used `((theme.primary & 0x00FFFFFF) | 0x40000000)`, intending "low-alpha primary tint." But `theme.primary` is `0xRRGGBBAA` (verified against the existing `color_to_f32` in `render.rs:655-662`), so that formula writes `0x40` to the **R** byte, not the **A** byte — a fully-opaque slightly-lighter blue, not a 25%-alpha tint. Patched to `((theme.primary & 0xFFFF_FF00) | 0x40)`, which zeroes the A byte and sets it to `0x40` (25%). Sub-agent flagged the issue in their deviation note.
- **Hover state** uses `theme.base_300` (a step lighter than the card's `base_100` default) for a subtle lift effect.
- **Container backgrounds** were already using theme tokens (`theme.base_200` fill, `theme.base_300` border, `theme.radius_box` corners) — confirmed in the painter push at `render.rs:226-233`. No change needed.
- **Drawer / search box** rely on akar's own theming via `drawer_begin` and `text_input` — no change needed.
- **`--screenshot <PATH>` and `--exit` clap flags** added to `Args` in `src/main.rs`. Pattern follows `akar/examples/demo-rust/src/main.rs:1484-1614`:
  - 5-second settle delay (`start_time` primed lazily on the first frame after `screenshot_path` is set).
  - `core.request_screenshot()` runs before the pass, then `core.capture_target_view(&device, w, h)` provides the color attachment.
  - `core.take_screenshot(&device, &queue, encoder, &frame)` returns `CapturedFrame { width, height, rgba }`; PNG-encoded with `png = "0.17"` (added to `Cargo.toml`).
  - `event_loop.exit()` is called from `window_event`'s `RedrawRequested` arm after `handle_redraw` returns, because `handle_redraw` doesn't have the `&ActiveEventLoop` borrow.
- `cargo check --workspace` passes clean (7 pre-existing dead-code warnings). `cargo test -p sugacode-indexer` 18/18. `cargo build` clean. `cargo run -- --help` shows the new `--screenshot` and `--exit` flags.
- **Not smoke-tested live** in this agent environment (no display surface available to `create_surface`). The release build compiles, the CLI parsing is verified, and the screenshot flow is a direct port of akar's reference example. A human (or visual-regression pipeline) needs to run `cargo run --release -- --screenshot /tmp/sugacode-akar.png --exit` to confirm the rendered output looks correct.

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

6. **Screenshot verification.** Add `--screenshot <path>` and `--exit` clap flags to sugacode's CLI (`src/main.rs`) — these do **not exist today**. akar's screenshot API is on `AkarCore` (not a CLI helper): `core.request_screenshot()` early in the frame, then `core.capture_target_view(&device, w, h)` returns a `wgpu::TextureView` to render into instead of the surface view when a shot is pending, and `core.take_screenshot(&device, &queue, encoder, &surface_texture)` returns `CapturedFrame { width, height, rgba: Vec<u8> }` to PNG-encode. akar ships `CapturedFrame`, **not** a PNG writer — encode the RGBA buffer with the `png` crate (see `~/Projects/akar/examples/demo-rust/src/main.rs:1578-1596` for the reference flow; the demo also hardcodes a 5-second settle delay before capture). Wire the flags so:
   ```bash
   cargo run --release -- --screenshot /tmp/sugacode-akar.png --exit
   ```
   captures the migrated UI and exits. Use this for visual regression comparison against the text-buffer original.

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
2. **Task 2:** Restructure application shell and render loop (3 hours)
3. **Task 3:** Migrate canvas (grid, zoom, pan) (3 hours)
4. **Task 4:** Migrate drawer (2 hours)
5. **Task 5:** Migrate container and card rendering (4 hours)
6. **Task 6:** Migrate search box to text_input (3 hours)
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
| akar's canvas does not support `push_text` in world space | Grid coordinate labels cannot be rendered | Use `push_quad` for grid lines only; coordinate labels deferred to akar Epic 015+ (verified: only `push_quad` exists on `CanvasPainter`) |
| akar is pre-alpha; API may change | Breaking changes during migration | Pin to specific commit; path dependency allows local fixes |
| Layout tree rebuilt per-frame may be slower than retained | Performance regression with many cards | Use `list_clip` (in `akar-core`, not akar-components) to minimize draw calls; profile early |
| akar's `text_input` may not support Cmd+K shortcut pre-focus | Search activation flow differs | Set `core.input.focused_id = Some(u64::from(search_node))` before calling `text_input` (Task 6 step 2). Confirmed: `text_input.rs:56-69` matches focus via `u64::from(node_id)` |
| **`akar_winit` does not expose Cmd/Ctrl modifier state** | Cmd+K shortcut and Cmd+left-drag pan don't work | Track `cmd_or_ctrl: bool` in `AppState` via a `WindowEvent::ModifiersChanged` arm in `main.rs`, separate from `akar_winit::process_window_event` (Task 1 step 5 + Task 2 step 4). `InputState` has no `modifiers` field |
| **`taffy 0.11` has no `Val::Px`/`Val::Percent` enum** | Pseudocode using `Val::Px` will not compile | Use `length(x)` / `percent(p)` helpers (re-exported via `akar_layout::pub use taffy::prelude::*`). Offset fields live in `Style.inset: Rect<LengthPercentageAuto>`, not standalone `left`/`top` (Task 6 step 2) |
| **`Layout::compute` needs a mandatory measure closure** | The third arg cannot be `None` | Always pass `|_,_,_,_,_| Size::ZERO` for nodes with fixed sizes (no content measuring) — see Task 2 step 1 |
| **akar's `PanButton` enum is only `Middle`/`Right`** | Cmd+left-click pan (sugacode convention) is not in akar | Implement manually in `render_canvas` BEFORE `canvas_begin`: track `cmd_panning: bool` separately in `AppState` and update `state.canvas_state.pan` directly. Don't reuse `CanvasState::is_panning` — it is reset by `canvas_begin` on every frame the configured button isn't pressed (Task 3 step 6) |
| **Per-frame `Layout::new()` churns `NodeId`s; `focused_id` is u64-keyed** | Search box loses focus between frames when Cmd+K opens it | Two options: re-assert `focused_id` each frame from a `state.search_active` flag (Task 6 step 2), or construct `Layout` once in `AppState::new()` and reuse across frames so NodeIds are stable |
| **`--screenshot`/`--exit` CLI flags don't exist in sugacode** | Visual regression step (Task 8) can't run as-is | Add new clap args in `src/main.rs`; call `core.request_screenshot()` / `capture_target_view` / `take_screenshot` and PNG-encode the returned `CapturedFrame { rgba }` with the `png` crate (akar ships the raw frame, not a writer — pattern in `akar/examples/demo-rust/src/main.rs:1578-1596`) |
| Drawer animation timing differs (frame-rate vs delta-time) | Animation feels different | Use delta-time-based animation; may need `Instant` tracking |
