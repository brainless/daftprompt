// Per-frame render functions wired into the loop in `main.rs::handle_redraw`.
//
// Task 2 introduced the signatures; Tasks 3/4/5/6 fill the bodies.
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
// Task 5 (containers/cards):
//   - `render_containers` runs between `render_grid` and `canvas_end`. It
//     walks `state.containers` and draws each one's background, scroll
//     area, and virtualized card list. Card backgrounds are pushed
//     directly to `core.draw_list` (so the `scroll_area_begin` scissor
//     clips them) at z=0.1; the container background goes through
//     `painter.push_quad` at z=0.0 (flushed at `canvas_end` behind the
//     cards). Card text is laid out in a rootless taffy overlay and
//     rendered with `label`. See the `render_containers` doc comment for
//     the full layering rationale.
//
// Task 6 (search):
//   - `render_search` builds a rootless taffy sub-tree containing a
//     mode-indicator `badge` (above the box) and the `text_input` (the
//     search box). Both are absolute-positioned children of a rootless
//     parent that covers the window. The same `Layout::new()`-per-frame
//     model that churns `NodeId`s for the canvas tree applies here too,
//     so `core.input.focused_id` is re-asserted from
//     `state.search_just_opened` before each `text_input` call (see the
//     epic Risks row on `focused_id` being u64-keyed).
//   - The mode indicator uses `badge` (cyan for Commits, green for Code)
//     — a 16px-tall pill above the search box left-aligned with it.
//   - Search execution (`resp.changed` or `resp.submitted`) re-runs
//     `indexer.search_hybrid` / `search_code_hybrid` and (re)creates a
//     results container; empty query removes it.

use akar_components::{
    akar_button as button, akar_container as container, akar_label as label,
    canvas_begin, canvas_end, BoxStyle, ButtonVariant, CanvasConfig, CanvasPainter,
    CanvasResponse, DrawerEdge,
};
use akar_components::{
    akar_badge as badge, akar_text_input as text_input, drawer_begin, drawer_end,
    scroll_area_begin, scroll_area_end, BadgeVariant,
};
use akar_core::{list_clip, AkarCore, QuadCall};
use akar_layout::{
    auto, length, CanvasTransform, Layout, NodeId, Position, Rect, Size, Style, WorldRect,
};
use glam::Vec2;

use crate::state::{self, AppState, IconType};
use crate::ui::container::{Container, ContainerType};

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

    // Container/card rendering (Task 5). World-space backgrounds flow
    // through `painter`; card backgrounds are pushed directly to
    // `core.draw_list` so the per-container `scroll_area_begin` scissor
    // clips them to the container rect. See the `render_containers` doc
    // comment for the z-ordering rationale.
    render_containers(core, &mut *layout, &mut painter, state, &resp.world_to_screen);

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

/// Renders all containers and their card lists between the grid and
/// `canvas_end`.
///
/// Each container is a world-space rectangle (`position`, `size`); its
/// screen-space rect comes from `world_to_screen.apply_rect(WorldRect { .. })`.
///
/// Per-container pipeline:
///   1. Push the container's background to the `CanvasPainter` (z=0.0).
///      The painter buffers quads and flushes them to `core.draw_list` on
///      `canvas_end` — that puts the background behind everything else
///      that already went into `core.draw_list` directly.
///   2. Open a scroll area (`scroll_area_begin`) which clamps
///      `container.scroll_offset` and pushes a scissor for the
///      container's screen rect onto `core.draw_list`.
///   3. Use `list_clip` (from `akar_core`) to pick a `Range<usize>` of
///      cards whose `card.position.y` falls inside the visible viewport,
///      plus one item of padding on each end (per the helper's contract
///      — see `akar-core/src/lib.rs:24`). The card heights are computed
///      per card by `new_git_log`/`new_search_results`/etc. but
///      `list_clip` takes a single uniform `item_height`; we pass the
///      first card's height (or 80px as a safe lower bound) and accept
///      the spec-acknowledged over-render at boundaries.
///   4. For each visible card:
///      * compute its screen-space rect,
///      * push the card background to `core.draw_list` directly (z=0.1)
///        so the active scroll-area scissor clips it, and so it sorts
///        after the painter's z=0.0 quads in `sorted_quads()`,
///      * update `card.is_hovered` from `core.input.is_hovering`,
///      * toggle `card.is_selected` from `core.input.is_clicked` (single-
///        select within a container),
///      * add a small taffy overlay (rootless absolute node, computed
///        as a second pass via `layout.compute(overlay_node, ..)`) with
///        one absolute child per text label (hash/author/date/message).
///   5. Close the scroll area (`scroll_area_end`) which pops the
///      container scissor.
///   6. After the loop, render all collected label nodes with `label`.
///      Labels go through `core.draw_list.push_text` (not the painter)
///      and are clipped by whichever scissor is active at the call site
///      — within a scroll area that means clipped to the container rect.
///
/// Card backgrounds are pushed to `core.draw_list` directly (not via the
/// painter) so the active scissor at the time of the call (the
/// container's scroll-area scissor) applies. The painter's buffer
/// doesn't carry a scissor; its quads only get pushed to
/// `core.draw_list` on `canvas_end` and would otherwise land unclipped
/// to the container.
fn render_containers(
    core: &mut AkarCore,
    layout: &mut Layout,
    painter: &mut CanvasPainter,
    state: &mut AppState,
    world_to_screen: &CanvasTransform,
) {
    let theme = match state.system_theme {
        state::SystemTheme::Dark => akar_components::AKAR_THEME_DARK,
        state::SystemTheme::Light => akar_components::AKAR_THEME_LIGHT,
    };

    // Rootless taffy overlay whose absolute children are the label
    // rectangles. Each label is a leaf with `inset.left`/`inset.top`
    // pointing at the desired screen-space position; we compute the
    // subtree once and then walk it with `label(...)`. Rootless means
    // `Layout::rect` traverses no parent chain and returns the
    // inset-based screen rect directly.
    let overlay_node = layout.new_leaf(Style {
        position: Position::Absolute,
        size: Size {
            width: length(state.window_size.x),
            height: length(state.window_size.y),
        },
        ..Default::default()
    });

    // Collected (node, text, color) tuples. Rendered after the subtree
    // is computed.
    let mut labels: Vec<(NodeId, String, u32)> = Vec::new();

    // Geometry constants.
    const TITLE_HEIGHT: f32 = 28.0;
    const HEADER_LINE_HEIGHT: f32 = 18.0;
    const SEPARATOR_HEIGHT: f32 = 8.0;
    const LABEL_GAP: f32 = 4.0;
    const CARD_BG_Z: f32 = 0.1;
    const SEPARATOR_Z: f32 = 0.15;

    for ci in 0..state.containers.len() {
        let container = &mut state.containers[ci];

        // Container screen rect. `container.position`/`size` are glam
        // 0.30 Vec2; `WorldRect` is constructed from f32s to dodge the
        // version-mismatch error (akar pulls in glam 0.33 transitively
        // — see the E0308 note from the `Vec2` import).
        let container_world_rect = WorldRect::from_xywh(
            container.position.x,
            container.position.y,
            container.size.x,
            container.size.y,
        );
        let container_screen_rect = world_to_screen.apply_rect(container_world_rect);
        let [cx, cy, cw, ch] = container_screen_rect;

        // Container background (panel-style). Pushed to the painter at
        // z=0.0 — it's flushed at `canvas_end` behind the cards.
        painter.push_quad(
            container_world_rect,
            theme.base_200,
            theme.base_300,
            1.0,
            [theme.radius_box; 4],
            0.0,
        );

        // Title bar text.
        let title_text = match container.container_type {
            ContainerType::GitLogColumn => "Git Log",
            ContainerType::SearchResults => "Search Results",
            ContainerType::CodeSearchResults => "Code Search",
            ContainerType::DocumentGrid => "Documents",
        };
        let title_node = layout.new_leaf(Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(cx + 12.0),
                top: length(cy + 6.0),
                right: auto(),
                bottom: auto(),
            },
            size: Size {
                width: length((cw - 24.0).max(20.0)),
                height: length(TITLE_HEIGHT),
            },
            ..Default::default()
        });
        layout.add_child(overlay_node, title_node);
        labels.push((title_node, title_text.to_string(), theme.base_content));

        // Scroll area. Pushes a scissor for the container rect and
        // clamps `container.scroll_offset` to `[0, content_height - ch]`.
        // Wheel events over the container advance the offset.
        let scroll_resp = scroll_area_begin(
            core,
            container_screen_rect,
            &mut container.scroll_offset,
            container.content_height,
        );

        // Virtualized visible range. `list_clip` returns
        // `start..end` with one-item padding on each end; cards outside
        // are skipped entirely.
        let card_height = container.cards.first().map(|c| c.size.y).unwrap_or(80.0);
        let visible = list_clip(
            container.cards.len(),
            card_height,
            container.scroll_offset,
            ch,
        );

        for i in visible {
            if i >= container.cards.len() {
                break;
            }

            // Read the card fields we need first so we can release the
            // mutable borrow before the (potential) selection-mutate
            // loop and the long match below.
            let card_pos = container.cards[i].position;
            let card_size = container.cards[i].size;
            let was_selected = container.cards[i].is_selected;
            let doc = container.documents[container.cards[i].document_id].clone();

            // Card screen rect. `card.position` is in world space
            // relative to the container; `scroll_resp.content_y` is the
            // container's screen y minus the scroll offset.
            let card_screen_x = cx + card_pos.x;
            let card_screen_y = scroll_resp.content_y + card_pos.y;
            let card_screen_rect = [card_screen_x, card_screen_y, card_size.x, card_size.y];

            // Hover/select. Updating `is_hovered` is a single-field
            // write that doesn't conflict with the iter_mut loop
            // because we drop the &mut borrow before starting it.
            let hovered = core.input.is_hovering(card_screen_rect);
            let clicked = core.input.is_clicked(card_screen_rect);
            container.cards[i].is_hovered = hovered;
            if clicked {
                for (j, c2) in container.cards.iter_mut().enumerate() {
                    c2.is_selected = j == i;
                }
            }

            // Card background. Pushed to `core.draw_list` directly (z=0.1)
            // so the active scroll-area scissor clips it to the
            // container, and so it sorts after the z=0.0 quads in
            // `draw_list.sorted_quads()`.
            let (fill, border) = if was_selected {
                (theme.primary, theme.primary)
            } else if hovered {
                (theme.base_100, theme.primary)
            } else {
                (theme.base_100, theme.base_300)
            };
            core.draw_list.push_quad(QuadCall {
                rect: card_screen_rect,
                fill: color_to_f32(fill),
                border_color: color_to_f32(border),
                corner_radii: [theme.radius_box; 4],
                border_width: theme.border_width,
                z: CARD_BG_Z,
                shadow_blur: 0.0,
                shadow_spread: 0.0,
                shadow_color: [0.0; 4],
                shadow_offset: [0.0; 2],
                _pad: [0.0; 2],
            });

            // Card content. Position labels in screen space inside the
            // card, with 12px left/right padding from the card edges.
            let pad = 12.0;
            let label_x = card_screen_x + pad;
            let mut label_y = card_screen_y + pad;
            let label_w = (card_size.x - pad * 2.0).max(20.0);

            match container.container_type {
                ContainerType::GitLogColumn | ContainerType::SearchResults => {
                    // Parse the content string for hash/author/date. The
                    // constructors write either
                    //   "<hash>\nAuthor: <name>\nDate: <time>"
                    // (git log, search results) or similar; we extract
                    // the first line as hash and any "Author:" / "Date:"
                    // lines after it.
                    let mut hash = "";
                    let mut author = "";
                    let mut date = "";
                    for line in doc.content.lines() {
                        if let Some(rest) = line.strip_prefix("Author: ") {
                            author = rest;
                        } else if let Some(rest) = line.strip_prefix("Date: ") {
                            date = rest;
                        } else if hash.is_empty() && !line.is_empty() {
                            hash = line;
                        }
                    }

                    // Hash — small, cyan.
                    if !hash.is_empty() {
                        let node = layout.new_leaf(Style {
                            position: Position::Absolute,
                            inset: Rect {
                                left: length(label_x),
                                top: length(label_y),
                                right: auto(),
                                bottom: auto(),
                            },
                            size: Size {
                                width: length(label_w),
                                height: length(HEADER_LINE_HEIGHT),
                            },
                            ..Default::default()
                        });
                        layout.add_child(overlay_node, node);
                        labels.push((node, hash.to_string(), theme.info));
                        label_y += HEADER_LINE_HEIGHT;
                    }

                    // Author — gray, smaller line.
                    if !author.is_empty() {
                        let node = layout.new_leaf(Style {
                            position: Position::Absolute,
                            inset: Rect {
                                left: length(label_x),
                                top: length(label_y),
                                right: auto(),
                                bottom: auto(),
                            },
                            size: Size {
                                width: length(label_w),
                                height: length(HEADER_LINE_HEIGHT),
                            },
                            ..Default::default()
                        });
                        layout.add_child(overlay_node, node);
                        labels.push((node, author.to_string(), theme.neutral_content));
                        label_y += HEADER_LINE_HEIGHT;
                    }

                    // Date — gray.
                    if !date.is_empty() {
                        let node = layout.new_leaf(Style {
                            position: Position::Absolute,
                            inset: Rect {
                                left: length(label_x),
                                top: length(label_y),
                                right: auto(),
                                bottom: auto(),
                            },
                            size: Size {
                                width: length(label_w),
                                height: length(HEADER_LINE_HEIGHT),
                            },
                            ..Default::default()
                        });
                        layout.add_child(overlay_node, node);
                        labels.push((node, date.to_string(), theme.neutral_content));
                        label_y += HEADER_LINE_HEIGHT;
                    }

                    // Thin separator quad between header and message.
                    let sep_y = label_y + LABEL_GAP;
                    core.draw_list.push_quad(QuadCall {
                        rect: [
                            label_x,
                            sep_y,
                            label_w,
                            1.0,
                        ],
                        fill: color_to_f32(theme.base_300),
                        border_color: [0.0; 4],
                        corner_radii: [0.0; 4],
                        border_width: 0.0,
                        z: SEPARATOR_Z,
                        shadow_blur: 0.0,
                        shadow_spread: 0.0,
                        shadow_color: [0.0; 4],
                        shadow_offset: [0.0; 2],
                        _pad: [0.0; 2],
                    });
                    label_y = sep_y + SEPARATOR_HEIGHT;

                    // Message — the title string, wrapped by the
                    // label's own width. Default text color.
                    let msg = if doc.title.is_empty() {
                        doc.content.clone()
                    } else {
                        doc.title.clone()
                    };
                    let remaining_h = (card_size.y - (label_y - card_screen_y) - pad).max(18.0);
                    let node = layout.new_leaf(Style {
                        position: Position::Absolute,
                        inset: Rect {
                            left: length(label_x),
                            top: length(label_y),
                            right: auto(),
                            bottom: auto(),
                        },
                        size: Size {
                            width: length(label_w),
                            height: length(remaining_h),
                        },
                        ..Default::default()
                    });
                    layout.add_child(overlay_node, node);
                    labels.push((node, msg, theme.base_content));
                }
                ContainerType::CodeSearchResults => {
                    // content is "<file_path>\n<line_start>:<line_end>"
                    let mut lines = doc.content.lines();
                    let file_path = lines.next().unwrap_or("");
                    let line_range = lines.next().unwrap_or("");

                    // File path — gray.
                    if !file_path.is_empty() {
                        let node = layout.new_leaf(Style {
                            position: Position::Absolute,
                            inset: Rect {
                                left: length(label_x),
                                top: length(label_y),
                                right: auto(),
                                bottom: auto(),
                            },
                            size: Size {
                                width: length(label_w),
                                height: length(HEADER_LINE_HEIGHT),
                            },
                            ..Default::default()
                        });
                        layout.add_child(overlay_node, node);
                        labels.push((node, file_path.to_string(), theme.neutral_content));
                        label_y += HEADER_LINE_HEIGHT;
                    }

                    // Line range — faint gray.
                    if !line_range.is_empty() {
                        let node = layout.new_leaf(Style {
                            position: Position::Absolute,
                            inset: Rect {
                                left: length(label_x),
                                top: length(label_y),
                                right: auto(),
                                bottom: auto(),
                            },
                            size: Size {
                                width: length(label_w),
                                height: length(HEADER_LINE_HEIGHT),
                            },
                            ..Default::default()
                        });
                        layout.add_child(overlay_node, node);
                        labels.push((node, line_range.to_string(), theme.neutral));
                        label_y += HEADER_LINE_HEIGHT;
                    }

                    // Identifier (title) — the prominent text.
                    label_y += LABEL_GAP;
                    let remaining_h = (card_size.y - (label_y - card_screen_y) - pad).max(18.0);
                    let node = layout.new_leaf(Style {
                        position: Position::Absolute,
                        inset: Rect {
                            left: length(label_x),
                            top: length(label_y),
                            right: auto(),
                            bottom: auto(),
                        },
                        size: Size {
                            width: length(label_w),
                            height: length(remaining_h),
                        },
                        ..Default::default()
                    });
                    layout.add_child(overlay_node, node);
                    labels.push((node, doc.title.clone(), theme.base_content));
                }
                ContainerType::DocumentGrid => {
                    // Icon + title with file-type emoji prefix; then
                    // content preview (truncated); then a metadata
                    // footer in small gray text.
                    let icon = icon_emoji(doc.file_type);
                    let title_with_icon = format!("{} {}", icon, doc.title);
                    let node = layout.new_leaf(Style {
                        position: Position::Absolute,
                        inset: Rect {
                            left: length(label_x),
                            top: length(label_y),
                            right: auto(),
                            bottom: auto(),
                        },
                        size: Size {
                            width: length(label_w),
                            height: length(HEADER_LINE_HEIGHT + 2.0),
                        },
                        ..Default::default()
                    });
                    layout.add_child(overlay_node, node);
                    labels.push((node, title_with_icon, theme.base_content));
                    label_y += HEADER_LINE_HEIGHT + 4.0;

                    // Truncated content preview.
                    const PREVIEW_MAX_CHARS: usize = 120;
                    let preview: String = if doc.content.chars().count() > PREVIEW_MAX_CHARS {
                        let mut s: String = doc.content.chars().take(PREVIEW_MAX_CHARS).collect();
                        s.push('…');
                        s
                    } else {
                        doc.content.clone()
                    };
                    let preview_h = (card_size.y - (label_y - card_screen_y) - pad - HEADER_LINE_HEIGHT).max(18.0);
                    let node = layout.new_leaf(Style {
                        position: Position::Absolute,
                        inset: Rect {
                            left: length(label_x),
                            top: length(label_y),
                            right: auto(),
                            bottom: auto(),
                        },
                        size: Size {
                            width: length(label_w),
                            height: length(preview_h),
                        },
                        ..Default::default()
                    });
                    layout.add_child(overlay_node, node);
                    labels.push((node, preview, theme.neutral_content));

                    // Metadata footer anchored to the bottom of the card.
                    let footer = format!("folder {} • {} chars", doc.folder_id, doc.content.chars().count());
                    let node = layout.new_leaf(Style {
                        position: Position::Absolute,
                        inset: Rect {
                            left: length(label_x),
                            top: length(card_screen_y + card_size.y - HEADER_LINE_HEIGHT - 4.0),
                            right: auto(),
                            bottom: auto(),
                        },
                        size: Size {
                            width: length(label_w),
                            height: length(HEADER_LINE_HEIGHT),
                        },
                        ..Default::default()
                    });
                    layout.add_child(overlay_node, node);
                    labels.push((node, footer, theme.neutral));
                }
            }
        }

        scroll_area_end(core);
    }

    // Compute the overlay sub-tree as a second pass — the rest of the
    // canvas tree (already computed for `canvas_node` in `handle_redraw`)
    // is unaffected because `overlay_node` is not a descendant.
    layout.compute(
        overlay_node,
        (Some(state.window_size.x), Some(state.window_size.y)),
        |_, _, _, _, _| Size::ZERO,
    );

    // Render the collected labels. Each `label` call pushes a TextCall
    // onto `core.draw_list` and respects the active scissor.
    for (node, text, color) in labels {
        label(core, &*layout, node, &text, color, &theme);
    }
}

// `akar_components::color::color_to_f32` is `pub(crate)` and not exposed
// in the public re-exports. The conversion is trivial — extract each
// 8-bit channel and divide by 255.0.
fn color_to_f32(c: u32) -> [f32; 4] {
    [
        ((c >> 24) & 0xFF) as f32 / 255.0,
        ((c >> 16) & 0xFF) as f32 / 255.0,
        ((c >> 8) & 0xFF) as f32 / 255.0,
        (c & 0xFF) as f32 / 255.0,
    ]
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

/// Renders the search box (commit search via Cmd+K, code search via
/// Cmd+Shift+K) and, when the query has changed, runs the search and
/// (re)creates the results container on the canvas.
///
/// Layout
/// ------
/// The box is 500×50px, centered horizontally, 70px above the window
/// bottom. A 16px-tall mode indicator (a `badge` reading "Commits" or
/// "Code") sits 4px above the box, left-aligned with its left edge.
///
/// All three nodes (rootless parent, indicator, box) are part of a
/// **rootless** taffy sub-tree. Per-frame `Layout::new()` churns the
/// `NodeId` slotmap keys, so a `u64::from(search_node)` from one frame
/// is meaningless the next; see the epic Risks row on
/// `focused_id` being u64-keyed. We mitigate by re-asserting focus from
/// `state.search_just_opened` each frame.
///
/// Input
/// -----
/// `text_input` mutates `value` (typed/deleted characters) and
/// `cursor_pos` (arrow/Home/End), and reports `changed` / `submitted`
/// via its response. The `value` and placeholder are picked from
/// `state.search_mode`; `cursor_pos` is shared across both modes
/// because only one search box is visible at a time.
///
/// Focus
/// -----
/// Before `text_input` runs we re-assert `core.input.focused_id`:
///   * on the frame the box opens (`state.search_just_opened`), or
///   * on any frame where nothing else has claimed focus and the box
///     is visible. This is the first of the two mitigations in the
///     Risks row — it lets the box hold focus across frames without
///     requiring a persistent `Layout`. Side effect: user-click-outside
///     to unfocus is undone on the same frame; closing the box (Escape)
///     is the only way out. The spec accepts this trade-off.
///
/// Search execution
/// ----------------
/// `resp.changed` (typed/backspaced) and `resp.submitted` (Enter) both
/// trigger `execute_search`. An empty query removes the
/// `SearchResults` / `CodeSearchResults` container; a non-empty query
/// runs hybrid search and replaces-or-creates the container at
/// world-space `(620, 20)`, width 500, height `window_size.y - 40`.
///
/// `dt` is the frame delta (already computed by the drawer's animation
/// block in `handle_redraw`). It drives the cursor blink at a 0.5s
/// half-cycle.
pub fn render_search(
    core: &mut AkarCore,
    layout: &mut Layout,
    state: &mut AppState,
    dt: f32,
) {
    let theme = match state.system_theme {
        state::SystemTheme::Dark => akar_components::AKAR_THEME_DARK,
        state::SystemTheme::Light => akar_components::AKAR_THEME_LIGHT,
    };

    // Cursor blink. Uses `dt` so the blink rate is fps-independent.
    state.cursor_timer += dt;
    if state.cursor_timer >= 0.5 {
        state.cursor_timer -= 0.5;
        state.cursor_visible = !state.cursor_visible;
    }

    // Geometry constants. Box centered horizontally, 70px above the
    // window's bottom edge; indicator is a 16px pill 4px above the box.
    const BOX_W: f32 = 500.0;
    const BOX_H: f32 = 50.0;
    const BOTTOM_GAP: f32 = 70.0;
    const INDICATOR_W: f32 = 80.0;
    const INDICATOR_H: f32 = 16.0;
    const INDICATOR_GAP: f32 = 4.0;
    let box_x = (state.window_size.x - BOX_W) / 2.0;
    let box_y = state.window_size.y - BOTTOM_GAP - BOX_H;
    let indicator_x = box_x;
    let indicator_y = box_y - INDICATOR_H - INDICATOR_GAP;

    // Rootless taffy sub-tree: a window-sized parent with two absolute
    // children. Computed independently of `canvas_node` so the canvas
    // tree (already computed in `handle_redraw`) is unaffected.
    let rootless = layout.new_leaf(Style {
        position: Position::Absolute,
        size: Size {
            width: length(state.window_size.x),
            height: length(state.window_size.y),
        },
        ..Default::default()
    });
    let indicator_node = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(indicator_x),
            top: length(indicator_y),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(INDICATOR_W),
            height: length(INDICATOR_H),
        },
        ..Default::default()
    });
    let search_node = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(box_x),
            top: length(box_y),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(BOX_W),
            height: length(BOX_H),
        },
        ..Default::default()
    });
    layout.add_child(rootless, indicator_node);
    layout.add_child(rootless, search_node);
    layout.compute(
        rootless,
        (Some(state.window_size.x), Some(state.window_size.y)),
        |_, _, _, _, _| Size::ZERO,
    );

    // Re-assert focus before `text_input`. See the function doc comment
    // and the epic Risks row.
    let id_u64 = u64::from(search_node);
    if state.search_just_opened {
        core.input.focused_id = Some(id_u64);
        state.search_just_opened = false;
    } else if (state.search_active || state.code_search_active)
        && core.input.focused_id.is_none()
    {
        core.input.focused_id = Some(id_u64);
    }

    let (value, placeholder): (&mut String, &str) = match state.search_mode {
        state::SearchMode::Commits => (
            &mut state.search_query,
            "Search documents... (Cmd+K)",
        ),
        state::SearchMode::Code => (
            &mut state.code_search_query,
            "Search code... (Cmd+Shift+K)",
        ),
    };

    let resp = text_input(
        core,
        &*layout,
        search_node,
        value,
        &mut state.cursor_pos,
        placeholder,
        state.cursor_visible,
        &theme,
    );

    // Mode indicator (cyan = Commits, green = Code). Rendered after the
    // input so its quad lands on top of any overlapping background.
    let (badge_variant, badge_text) = match state.search_mode {
        state::SearchMode::Commits => (BadgeVariant::Info, "Commits"),
        state::SearchMode::Code => (BadgeVariant::Success, "Code"),
    };
    badge(
        core,
        &*layout,
        indicator_node,
        badge_text,
        badge_variant,
        &theme,
    );

    if resp.changed || resp.submitted {
        execute_search(state);
    }
}

/// Run the search for the active mode using `state.search_query` /
/// `state.code_search_query`. Empty query → remove the results
/// container; otherwise call the indexer and (re)create the container
/// at a fixed world-space position so the user sees a stable results
/// panel as they type.
///
/// Both the `SearchResults` and `CodeSearchResults` containers use a
/// (id, position, width, viewport_height, results) constructor. The id
/// is reused from the previous container if one exists, so the id
/// space stays small and the card-click handlers in
/// `render_containers` don't see their selection state orphaned to a
/// ghost container.
fn execute_search(state: &mut AppState) {
    match state.search_mode {
        state::SearchMode::Commits => {
            if state.search_query.is_empty() {
                state.search_results.clear();
                state
                    .containers
                    .retain(|c| c.container_type != ContainerType::SearchResults);
                return;
            }
            if let Some(indexer) = state.indexer.as_ref() {
                match indexer.search_hybrid(&state.search_query, 20) {
                    Ok(results) => {
                        let existing_id = state
                            .containers
                            .iter()
                            .find(|c| c.container_type == ContainerType::SearchResults)
                            .map(|c| c.id);
                        state
                            .containers
                            .retain(|c| c.container_type != ContainerType::SearchResults);
                        let id = existing_id.unwrap_or_else(|| {
                            state.containers.iter().map(|c| c.id).max().unwrap_or(0) + 1
                        });
                        state.containers.push(Container::new_search_results(
                            id,
                            Vec2::new(620.0, 20.0),
                            500.0,
                            state.window_size.y - 40.0,
                            results,
                        ));
                    }
                    Err(e) => {
                        log::warn!("Commit search failed: {e}");
                    }
                }
            }
        }
        state::SearchMode::Code => {
            if state.code_search_query.is_empty() {
                state.code_search_results.clear();
                state
                    .containers
                    .retain(|c| c.container_type != ContainerType::CodeSearchResults);
                return;
            }
            if let Some(indexer) = state.indexer.as_ref() {
                match indexer.search_code_hybrid(&state.code_search_query, 20) {
                    Ok(results) => {
                        let existing_id = state
                            .containers
                            .iter()
                            .find(|c| c.container_type == ContainerType::CodeSearchResults)
                            .map(|c| c.id);
                        state
                            .containers
                            .retain(|c| c.container_type != ContainerType::CodeSearchResults);
                        let id = existing_id.unwrap_or_else(|| {
                            state.containers.iter().map(|c| c.id).max().unwrap_or(0) + 1
                        });
                        state.containers.push(Container::new_code_search_results(
                            id,
                            Vec2::new(620.0, 20.0),
                            500.0,
                            state.window_size.y - 40.0,
                            results,
                        ));
                    }
                    Err(e) => {
                        log::warn!("Code search failed: {e}");
                    }
                }
            }
        }
    }
}
