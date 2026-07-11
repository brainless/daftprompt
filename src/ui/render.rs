// Per-frame render functions wired into the loop in `main.rs::handle_redraw`.
//
// Task 2 introduced the signatures; Tasks 3/4/6 fill the bodies.
//
// Task 3 (canvas):
//   - `canvas_begin`/`canvas_end` wrap the grid + (later) container layer.
//   - `cmd_panning` runs in `handle_redraw` *before* `render_canvas` so the
//     frame's `world_to_screen` transform from `canvas_begin` reflects the
//     drag (see `main.rs` for the block + rationale).
//   - The zoom indicator is a small `label` node in screen space, computed
//     and rendered after `canvas_end` (the canvas scissor is popped, so the
//     text is not clipped to the canvas).
//
// Tasks 4 (drawer) and 6 (search) still stub the remaining functions.

use akar_components::{
    canvas_begin, canvas_end, akar_label as label, CanvasConfig, CanvasPainter, CanvasResponse,
};
use akar_core::AkarCore;
use akar_layout::{Layout, NodeId, WorldRect};

use crate::state::{self, AppState};

/// Renders the infinite canvas: grid (world space) + zoom indicator (screen
/// space).
///
/// `canvas_node` is the root layout node covering the full window — its rect
/// defines the screen-space viewport the world is drawn into.
/// `indicator_node` is a pre-built absolute-positioned child of
/// `canvas_node` (created in `main.rs::handle_redraw`); it is used here
/// just to draw the zoom percentage label after `canvas_end`.
///
/// Order of operations within this function:
///   1. `canvas_begin` — handles middle/right pan and cursor-anchored wheel
///      zoom, pushes a scissor for the canvas rect, and returns a
///      `CanvasResponse` (transforms, visible rect) and a `CanvasPainter`
///      (quad buffer in world space).
///   2. `render_grid` — pushes thin quads for the world-space grid via the
///      painter.
///   3. `canvas_end` — flushes the painter's quads to `core.draw_list` and
///      pops the scissor.
///   4. Zoom indicator — `label` at `indicator_node`'s rect. `label`
///      consumes `&mut AkarCore` so it can't be called while the painter is
///      alive; it must run after `canvas_end`.
///
/// Container/card rendering is Task 5 and will run between (2) and (3).
pub fn render_canvas(
    core: &mut AkarCore,
    layout: &mut Layout,
    canvas_node: NodeId,
    indicator_node: NodeId,
    state: &mut AppState,
) {
    let theme = match state.system_theme {
        state::SystemTheme::Dark => akar_components::AKAR_THEME_DARK,
        state::SystemTheme::Light => akar_components::AKAR_THEME_LIGHT,
    };

    // canvas_begin handles middle/right pan and cursor-anchored wheel zoom,
    // and pushes a scissor for the canvas rect. Cmd+Left-drag is handled in
    // main.rs *before* this call so the pan shows up in the frame's
    // world_to_screen transform.
    let config = CanvasConfig::default();
    let (resp, mut painter) = canvas_begin(
        core,
        &*layout,
        canvas_node,
        &mut state.canvas_state,
        &config,
    );

    render_grid(&mut painter, &resp, state);

    // Container/card rendering lives here in Task 5.

    canvas_end(core, painter);

    // Zoom indicator. Screen-space, not clipped to the canvas (the canvas
    // scissor was popped by `canvas_end`). The node was added in
    // `handle_redraw` as an absolute-positioned child of `canvas_node` with
    // a fixed size of 80x20; `layout.rect(indicator_node)` returns its
    // screen-space rect.
    let zoom_text = format!("{}%", (state.canvas_state.zoom * 100.0) as i32);
    label(
        core,
        &*layout,
        indicator_node,
        &zoom_text,
        theme.base_content,
        &theme,
    );
}

/// World-space grid. Picks a spacing in world units that keeps screen-space
/// line density roughly constant (one major line every ~50px) by snapping to
/// a power of 10. Hidden when the canvas is zoomed out past a readability
/// threshold.
///
/// Grid line thickness is `1.0 / zoom` in world space, so the line is ~1px
/// wide on screen regardless of zoom level (caller may see aliased 1px lines
/// at high zoom — acceptable for a grid).
fn render_grid(painter: &mut CanvasPainter, resp: &CanvasResponse, state: &AppState) {
    let zoom = state.canvas_state.zoom;
    if zoom < 0.15 {
        return;
    }

    let target_screen_px = 50.0;
    let world_spacing = (target_screen_px / zoom).max(1.0);
    // Round to a power of 10 so spacings are 1, 10, 100, 1000, ... — keeps
    // the grid visually consistent at all zoom levels.
    let exponent = world_spacing.log10().floor() as i32;
    let spacing = 10f32.powi(exponent);

    let vis = resp.visible_world_rect;
    // base_300 from the dark theme (0x27272aff) at 50% alpha.
    // Color is 0xRRGGBBAA: R=0x27, G=0x27, B=0x2A, A=0x80.
    let line_color = if state.system_theme == state::SystemTheme::Dark {
        0x27272a80
    } else {
        // base_300 from the light theme (0xe4e4e7ff) at 50% alpha.
        0xe4e4e780
    };

    // Vertical lines (constant x, full visible y range).
    let line_thickness_world = 1.0 / zoom;
    if vis.max.y > vis.min.y {
        let start_x = (vis.min.x / spacing).floor() * spacing;
        let end_x = (vis.max.x / spacing).ceil() * spacing;
        let mut x = start_x;
        while x <= end_x {
            painter.push_quad(
                WorldRect::from_xywh(x, vis.min.y, line_thickness_world, vis.max.y - vis.min.y),
                line_color,
                0x00000000,
                0.0,
                [0.0; 4],
                0.0,
            );
            x += spacing;
        }
    }

    // Horizontal lines (constant y, full visible x range).
    if vis.max.x > vis.min.x {
        let start_y = (vis.min.y / spacing).floor() * spacing;
        let end_y = (vis.max.y / spacing).ceil() * spacing;
        let mut y = start_y;
        while y <= end_y {
            painter.push_quad(
                WorldRect::from_xywh(vis.min.x, y, vis.max.x - vis.min.x, line_thickness_world),
                line_color,
                0x00000000,
                0.0,
                [0.0; 4],
                0.0,
            );
            y += spacing;
        }
    }
}

/// Renders the left navigation drawer (scrim + folder list). Stub for Task 2;
/// Task 4 wires this in using `drawer_begin` + `container` + `label` + `button`.
pub fn render_drawer(_core: &mut AkarCore, _layout: &mut Layout, _state: &mut AppState) {
    // TODO(Task 4): drawer panel + folder list.
}

/// Renders the search box (commit search via Cmd+K, code search via Cmd+Shift+K).
/// Stub for Task 2; Task 6 wires this in using `text_input` + `modal`.
pub fn render_search(_core: &mut AkarCore, _layout: &mut Layout, _state: &mut AppState) {
    // TODO(Task 6): text_input + results overlay.
}
