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
// Task 4 (drawer):
//   - `render_drawer` builds a rootless taffy sub-tree (panel + folder rows
//     + icon/text labels) and renders it inside the scissor that
//     `drawer_begin` pushes. The panel width animates 60px→250px via
//     `state.drawer_animation`, which `main.rs` advances each frame from
//     a delta-time. See the `render_drawer` doc comment for details.
//
// Task 6 (search) still stubs the search box.

use akar_components::{
    akar_button as button, akar_container as container, akar_label as label,
    canvas_begin, canvas_end, BoxStyle, ButtonVariant, CanvasConfig, CanvasPainter,
    CanvasResponse, DrawerEdge,
};
use akar_components::{drawer_begin, drawer_end};
use akar_core::AkarCore;
use akar_layout::{auto, length, Layout, NodeId, Position, Rect, Size, Style, WorldRect};

use crate::state::{self, AppState, IconType};

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

/// Renders the left navigation drawer (scrim + folder list).
///
/// The drawer is a fixed-width strip on the left edge. Its width animates
/// from 60px (collapsed, icons only) to 250px (expanded, icon + name + doc
/// count) via `state.drawer_animation` ∈ [0.0, 1.0], which is advanced
/// each frame in `main.rs::handle_redraw` from a delta-time.
///
/// Layout strategy:
///   * `panel_node` is a *rootless* taffy node (not added under canvas_node).
///     Its size is `length(panel_width) × length(window_height)` and it
///     carries no inset, so its computed `location` is (0, 0). This matches
///     the panel rect that `drawer_begin` scissors to, so any descendant
///     whose inset/size places it inside the panel will render unclipped.
///   * `drawer_begin` is called with the full viewport rect — it draws the
///     scrim + panel quad itself and pushes a scissor for the panel rect.
///     The folder rows below render inside that scissor, so they are
///     automatically clipped to the panel even if they overflow.
///   * For each folder we build a `row_node` (absolute, `top: 20 + i*66`)
///     plus an `icon_node` and (when expanded) a `text_node` for the
///     name/count. The `button` covers the row for hover/click; the
///     selected row gets a tinted `container` underneath.
///   * After all nodes are added we recompute just the panel sub-tree via
///     `layout.compute(panel_node, (panel_width, window_height), …)`. The
///     rest of the canvas tree (computed earlier in `handle_redraw` for
///     `canvas_node`) is unaffected.
///
/// Behaviour:
///   * Scrim click (set by `drawer_begin`'s `close_requested`) toggles
///     `state.drawer_open` to false.
///   * Clicking a folder row sets `state.selected_folder = Some(i)`.
///   * Hovering a row sets `state.hover_index = Some(i)`.
pub fn render_drawer(core: &mut AkarCore, layout: &mut Layout, state: &mut AppState) {
    let theme = match state.system_theme {
        state::SystemTheme::Dark => akar_components::AKAR_THEME_DARK,
        state::SystemTheme::Light => akar_components::AKAR_THEME_LIGHT,
    };

    // Animated panel width. Collapsed = 60px (icons only), expanded = 250px
    // (icons + name + doc count). The collapsed bar is wide enough to fit a
    // 40px icon with 10px padding on each side.
    const PANEL_COLLAPSED: f32 = 60.0;
    const PANEL_EXPANDED: f32 = 250.0;
    let panel_width = PANEL_COLLAPSED + (PANEL_EXPANDED - PANEL_COLLAPSED) * state.drawer_animation;
    let viewport_rect = [0.0, 0.0, state.window_size.x, state.window_size.y];

    // Build the panel sub-tree. `panel_node` is rootless — `layout.rect`
    // returns (0, 0, panel_width, window_height), which lines up with the
    // panel rect the scissor will be set to.
    let panel_node = layout.new_leaf(Style {
        position: Position::Absolute,
        size: Size {
            width: length(panel_width),
            height: length(state.window_size.y),
        },
        // display::Block (default) — absolute children are out-of-flow and
        // positioned by the absolute layout pass.
        ..Default::default()
    });

    // Per-folder row layout. The rows are absolute children of the panel,
    // stacked vertically with a 10px gap. Each row is 56px tall, 20px
    // top-padding before the first row. We also keep the icon (and
    // optional text) node IDs alongside the row ID so the second pass can
    // draw labels without needing `get_child_at` (which taffy 0.11 does
    // not expose).
    struct RowNodes {
        row: NodeId,
        icon: NodeId,
        text: Option<NodeId>,
    }
    const ROW_HEIGHT: f32 = 56.0;
    const ROW_GAP: f32 = 10.0;
    const ROW_TOP: f32 = 20.0;
    let show_labels = state.drawer_animation > 0.5;
    let mut rows: Vec<RowNodes> = Vec::with_capacity(state.folders.len());

    for i in 0..state.folders.len() {
        let row_top = ROW_TOP + i as f32 * (ROW_HEIGHT + ROW_GAP);
        let row = layout.new_leaf(Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(0.0),
                top: length(row_top),
                right: auto(),
                bottom: auto(),
            },
            size: Size {
                width: length(panel_width),
                height: length(ROW_HEIGHT),
            },
            ..Default::default()
        });
        layout.add_child(panel_node, row);

        // Icon node — always visible. A 40x56 box with 10px left padding.
        let icon = layout.new_leaf(Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(10.0),
                top: length(0.0),
                right: auto(),
                bottom: auto(),
            },
            size: Size {
                width: length(40.0),
                height: length(ROW_HEIGHT),
            },
            ..Default::default()
        });
        layout.add_child(row, icon);

        // Text node — only when the panel is wide enough. Sized to fit
        // the row width minus icon + padding.
        let text = if show_labels {
            let text_left = 10.0 + 40.0 + 8.0;
            let text_width = (panel_width - text_left - 10.0).max(20.0);
            let text = layout.new_leaf(Style {
                position: Position::Absolute,
                inset: Rect {
                    left: length(text_left),
                    top: length(0.0),
                    right: auto(),
                    bottom: auto(),
                },
                size: Size {
                    width: length(text_width),
                    height: length(ROW_HEIGHT),
                },
                ..Default::default()
            });
            layout.add_child(row, text);
            Some(text)
        } else {
            None
        };

        rows.push(RowNodes { row, icon, text });
    }

    // Recompute just the panel sub-tree. The canvas tree (computed earlier
    // in `handle_redraw` for `canvas_node`) is unaffected because
    // `panel_node` is not a descendant of `canvas_node`.
    layout.compute(
        panel_node,
        (Some(panel_width), Some(state.window_size.y)),
        |_, _, _, _, _| Size::ZERO,
    );

    // Start the drawer. `drawer_begin` draws the scrim + panel quad and
    // pushes a scissor for the panel rect; we render the folder list
    // inside the scissor below.
    let drawer_resp = drawer_begin(core, viewport_rect, DrawerEdge::Left, panel_width, &theme);
    if drawer_resp.close_requested {
        state.drawer_open = false;
    }

    // Track which row the mouse is over this frame. Reset to None first;
    // any row that reports `hovered = true` will overwrite it.
    state.hover_index = None;

    // Render each folder row. Order of pushes matters: the selected-row
    // container goes first (behind), then the button (which is transparent
    // when not hovered, so the container shows through for selected rows
    // that aren't being hovered), then the labels.
    for i in 0..state.folders.len() {
        let row = rows[i].row;
        let is_selected = state.selected_folder == Some(i);

        if is_selected {
            // Tinted background under the row. The `container` early-returns
            // on `style.fill == 0` so this is the cheapest way to draw a
            // solid quad; here we use a low-alpha primary tint.
            let mut bg_style = BoxStyle::panel(&theme);
            bg_style.fill = theme.primary;
            container(core, layout, row, &bg_style);
        }

        // Ghost button covering the whole row. Pass " " as the label so
        // the button's own text is invisible — we draw the icon and name
        // as separate `label` calls on top of the button's hover quad.
        let btn = button(core, &*layout, row, " ", ButtonVariant::Ghost, &theme);
        if btn.clicked {
            state.selected_folder = Some(i);
        }
        if btn.hovered {
            state.hover_index = Some(i);
        }
    }

    // Draw the icon + (when expanded) the name + count as labels. Labels
    // are text-only quads; they sit on top of the row's container/button
    // quads (we push them last).
    for (i, folder) in state.folders.iter().enumerate() {
        let entry = &rows[i];
        let icon_text = icon_emoji(folder.icon);
        label(
            core,
            &*layout,
            entry.icon,
            icon_text,
            theme.base_content,
            &theme,
        );

        if let Some(text) = entry.text {
            let name_text = format!("{}\n{} docs", folder.name, folder.document_count);
            label(
                core,
                &*layout,
                text,
                &name_text,
                theme.base_content,
                &theme,
            );
        }
    }

    drawer_end(core);
}

fn icon_emoji(icon: IconType) -> &'static str {
    match icon {
        IconType::Folder => "📁",
        IconType::GitRepo => "🔗",
        IconType::Document => "📄",
        IconType::Code => "📜",
        IconType::Markdown => "📝",
        IconType::Search => "🔍",
        IconType::Settings => "⚙",
    }
}

/// Renders the search box (commit search via Cmd+K, code search via Cmd+Shift+K).
/// Stub for Task 2; Task 6 wires this in using `text_input` + `modal`.
pub fn render_search(_core: &mut AkarCore, _layout: &mut Layout, _state: &mut AppState) {
    // TODO(Task 6): text_input + results overlay.
}
