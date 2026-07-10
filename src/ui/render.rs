// Task 2 stubs. These are wired into the render loop but do nothing yet.
// Task 3 fills in the canvas (grid + zoom/pan), Task 4 fills in the drawer,
// and Task 6 fills in the search box. The signatures must stay stable so
// callers in `main.rs` keep compiling across the migration.

use akar_core::AkarCore;
use akar_layout::{Layout, NodeId};

use crate::state::AppState;

/// Renders the infinite canvas, including the grid, container/card layer,
/// and any on-canvas overlays. For Task 2 this is a no-op that only touches
/// the layout tree (to exercise `Layout::compute`) — the actual visual layer
/// is added in Task 3.
///
/// `canvas_node` is the root layout node covering the full window.
/// Reads `layout.rect(canvas_node)` to confirm the layout tree was computed
/// (this is the only thing this stub needs to do for `cargo check` to pass
/// and the empty draw list to render cleanly).
pub fn render_canvas(
    _core: &mut AkarCore,
    layout: &mut Layout,
    canvas_node: NodeId,
    _state: &mut AppState,
) {
    // TODO(Task 3): replace with `canvas_begin` + grid + container iteration.
    let _rect = layout.rect(canvas_node);
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
