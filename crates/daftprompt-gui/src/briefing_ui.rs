//! Painting for the Project tab, the details area and the command footer.
//!
//! The Project tab and the details area share one scrollable stack of
//! [`Block`]s, laid out by hand (heights come from the same conservative wrap
//! estimate as `render.rs`) and drawn with an explicit viewport clip so
//! scrolled text never paints over the chrome. The footer is a single row of
//! buttons laid out left to right with the overflow math in `briefing_view`.
//!
//! akar paints all text above all non-foreground quads. Everything drawn here
//! therefore either sits alone in its area (the details area replaces the
//! content column body while open) or has no text from another widget behind
//! it (the footer owns its own strip).

use akar_components::{AkarTheme, scroll_area_begin, scroll_area_end};
use akar_core::{AkarCore, QuadCall, TextCall, Z_BASE};

use crate::briefing::{Details, Section};
use crate::briefing_view::{
    Action, Block, BlockKind, FooterStatus, button_width, more_label, visible_buttons,
};
use crate::fonts::{self, Role};
use crate::render::{
    color_to_f32, draw_scrollbar, draw_text_weighted, estimated_text_height, intersect, intersects,
};

/// Text buffer ids for this module, far from layout widget ids and the menu's.
const BLOCK_BUFFER_BASE: u64 = 0x4252_4946_0000_0000;
const FOOTER_BUFFER_BASE: u64 = 0x464f_4f54_0000_0000;

const ITEM_GAP: f32 = 6.0;
const TITLE_HEIGHT: f32 = 30.0;
const TITLE_BUTTON_HEIGHT: f32 = 24.0;
const ROW_PADDING: f32 = 8.0;
const BOTTOM_PADDING: f32 = 24.0;
/// Space kept at the right edge for the scrollbar.
const SCROLLBAR_GUTTER: f32 = 12.0;

pub const FOOTER_BUTTON_HEIGHT: f32 = 28.0;
pub const FOOTER_BUTTON_GAP: f32 = 8.0;
const FOOTER_RERUN_WIDTH: f32 = 74.0;

struct Item {
    y: f32,
    height: f32,
    block: Block,
}

/// A stack of blocks laid out for one width.
pub struct BlockLayout {
    items: Vec<Item>,
    total_height: f32,
}

impl BlockLayout {
    pub fn total_height(&self) -> f32 {
        self.total_height
    }
}

/// Font role, weight, size, line height and colour of a block kind.
fn style(kind: &BlockKind, theme: &AkarTheme) -> (Role, u16, f32, f32, u32) {
    let base = theme.font_size_base;
    let small = theme.font_size_sm;
    match kind {
        BlockKind::Title(_) => (Role::Ui, 600, base * 1.15, base * 1.5, theme.base_content),
        BlockKind::Heading => (Role::Ui, 600, base * 1.05, base * 1.5, theme.base_content),
        BlockKind::Body | BlockKind::Bullet => (
            Role::Text,
            400,
            base * 1.06,
            base * 1.06 * 1.5,
            theme.base_content,
        ),
        BlockKind::Code => (Role::Mono, 400, small, small * 1.5, theme.base_content),
        BlockKind::Citation => (Role::Ui, 400, small, small * 1.5, theme.muted_content),
        BlockKind::Notice => (Role::Ui, 400, small, small * 1.5, theme.warning),
        BlockKind::Muted => (Role::Ui, 400, small, small * 1.5, theme.muted_content),
        BlockKind::Error => (Role::Ui, 400, base, base * 1.5, theme.error),
        BlockKind::Field => (Role::Ui, 500, small, small * 1.4, theme.muted_content),
        BlockKind::Row(_) => (Role::Ui, 400, base, base * 1.4, theme.base_content),
    }
}

/// Lay `blocks` out at `width`. Called when the content or the width changes.
pub fn build_layout(blocks: &[Block], width: f32, theme: &AkarTheme) -> BlockLayout {
    let width = width.max(16.0);
    let text_width = (width - SCROLLBAR_GUTTER).max(8.0);
    let mut items = Vec::with_capacity(blocks.len());
    let mut y = 0.0;
    for (index, block) in blocks.iter().enumerate() {
        let (_, _, size, line, _) = style(&block.kind, theme);
        let (top, height) = match &block.kind {
            BlockKind::Title(button) => {
                let reserved = button
                    .as_ref()
                    .map_or(0.0, |b| button_width(&b.label) + 8.0);
                let h = estimated_text_height(
                    &block.text,
                    (text_width - reserved).max(8.0),
                    size,
                    line,
                )
                .max(TITLE_HEIGHT);
                (if index == 0 { 0.0 } else { 16.0 }, h)
            }
            BlockKind::Heading => (
                6.0,
                estimated_text_height(&block.text, text_width, size, line),
            ),
            BlockKind::Field => (
                6.0,
                estimated_text_height(&block.text, text_width, size, line),
            ),
            BlockKind::Row(_) => (
                0.0,
                estimated_text_height(
                    &block.text,
                    (text_width - 2.0 * ROW_PADDING).max(8.0),
                    size,
                    line,
                ) + 2.0 * ROW_PADDING,
            ),
            _ => (
                0.0,
                estimated_text_height(&block.text, text_width, size, line),
            ),
        };
        if index > 0 {
            y += ITEM_GAP;
        }
        y += top;
        items.push(Item {
            y,
            height,
            block: block.clone(),
        });
        y += height;
    }
    BlockLayout {
        items,
        total_height: y + BOTTOM_PADDING,
    }
}

/// Draw the stack into `rect`, scrolled by `scroll_y`. Returns the action of a
/// button or row clicked this frame.
pub fn draw_blocks(
    core: &mut AkarCore,
    layout: &BlockLayout,
    rect: [f32; 4],
    scroll_y: &mut f32,
    theme: &AkarTheme,
) -> Option<Action> {
    if rect[2] <= 0.0 || rect[3] <= 0.0 {
        return None;
    }
    let mut action = None;
    let response = scroll_area_begin(core, rect, scroll_y, layout.total_height());
    let text_width = (rect[2] - SCROLLBAR_GUTTER).max(8.0);
    for (index, item) in layout.items.iter().enumerate() {
        let item_rect = [
            rect[0],
            response.content_y + item.y,
            text_width,
            item.height,
        ];
        if !intersects(item_rect, rect) {
            continue;
        }
        let id = BLOCK_BUFFER_BASE + index as u64 * 4;
        let (role, weight, size, line, color) = style(&item.block.kind, theme);
        match &item.block.kind {
            BlockKind::Title(button) => {
                let reserved = if let Some(button) = button {
                    let width = button_width(&button.label);
                    let button_rect = [
                        item_rect[0] + text_width - width,
                        item_rect[1] + (item_rect[3] - TITLE_BUTTON_HEIGHT) * 0.5,
                        width,
                        TITLE_BUTTON_HEIGHT,
                    ];
                    let look = if button.enabled {
                        Look::Outline
                    } else {
                        Look::Disabled
                    };
                    if draw_button(core, id + 1, button_rect, &button.label, look, rect, theme)
                        && button.enabled
                    {
                        action = Some(button.action.clone());
                    }
                    width + 8.0
                } else {
                    0.0
                };
                draw_text_weighted(
                    core,
                    id,
                    [
                        item_rect[0],
                        item_rect[1] + 2.0,
                        item_rect[2] - reserved,
                        item_rect[3],
                    ],
                    &item.block.text,
                    role,
                    weight,
                    size,
                    line,
                    color,
                    rect,
                );
            }
            BlockKind::Row(row_action) => {
                let hit = intersect(item_rect, rect);
                if hit[2] > 0.0 && hit[3] > 0.0 {
                    if core.input.is_hovering(hit) {
                        push_quad(core, hit, theme.base_300, 0, theme.radius_field, 0.0);
                    }
                    if core.input.is_clicked(hit) {
                        action = Some(row_action.clone());
                    }
                }
                draw_text_weighted(
                    core,
                    id,
                    [
                        item_rect[0] + ROW_PADDING,
                        item_rect[1] + ROW_PADDING,
                        item_rect[2] - 2.0 * ROW_PADDING,
                        item_rect[3] - 2.0 * ROW_PADDING,
                    ],
                    &item.block.text,
                    role,
                    weight,
                    size,
                    line,
                    color,
                    rect,
                );
            }
            _ => draw_text_weighted(
                core,
                id,
                item_rect,
                &item.block.text,
                role,
                weight,
                size,
                line,
                color,
                rect,
            ),
        }
    }
    scroll_area_end(core);
    draw_scrollbar(core, rect, *scroll_y, layout.total_height(), theme);
    action
}

/// A rounded panel behind the details area.
pub fn draw_card(core: &mut AkarCore, rect: [f32; 4], theme: &AkarTheme) {
    push_quad(
        core,
        rect,
        theme.base_200,
        theme.base_300,
        theme.radius_box,
        theme.border_width,
    );
}

fn push_quad(core: &mut AkarCore, rect: [f32; 4], fill: u32, border: u32, radius: f32, bw: f32) {
    core.draw_list.push_quad(QuadCall {
        rect,
        fill: color_to_f32(fill),
        border_color: color_to_f32(border),
        corner_radii: [radius; 4],
        border_width: bw,
        z: Z_BASE,
        shadow_blur: 0.0,
        shadow_spread: 0.0,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        _pad: [0.0; 2],
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Outline,
    Active,
    Disabled,
}

/// A small text button. Hit testing and painting use the part of `rect` inside
/// `clip`, so a partly scrolled button cannot be clicked through the chrome.
fn draw_button(
    core: &mut AkarCore,
    id: u64,
    rect: [f32; 4],
    label: &str,
    look: Look,
    clip: [f32; 4],
    theme: &AkarTheme,
) -> bool {
    let hit = intersect(rect, clip);
    if hit[2] <= 0.0 || hit[3] <= 0.0 {
        return false;
    }
    let enabled = look != Look::Disabled;
    let hovered = enabled && core.input.is_hovering(hit);
    let clicked = enabled && core.input.is_clicked(hit);
    let (fill, border, content) = match look {
        Look::Disabled => (0x0000_0000, theme.base_300, theme.muted_content),
        Look::Active => (theme.primary, theme.primary, theme.primary_content),
        Look::Outline if hovered => (theme.base_300, theme.primary, theme.base_content),
        Look::Outline => (0x0000_0000, theme.base_300, theme.base_content),
    };
    push_quad(
        core,
        hit,
        fill,
        border,
        theme.radius_field,
        theme.border_width,
    );
    let size = theme.font_size_sm;
    let buffer = core.text_pipeline.set_text_transient(
        id,
        label,
        glyphon::Metrics::new(size, size * 1.2),
        None,
        None,
        Some(fonts::current().attrs(Role::Ui, 500)),
    );
    core.draw_list.push_text(TextCall {
        buffer_id: buffer,
        x: rect[0] + 12.0,
        y: rect[1] + (rect[3] - size) * 0.5 - 1.0,
        clip: hit,
        color: color_to_f32(content),
        z: Z_BASE,
    });
    clicked
}

fn footer_text(core: &mut AkarCore, id: u64, rect: [f32; 4], text: &str, color: u32, size: f32) {
    let buffer = core.text_pipeline.set_text_transient(
        id,
        text,
        glyphon::Metrics::new(size, size * 1.2),
        None,
        None,
        Some(fonts::current().attrs(Role::Ui, 400)),
    );
    core.draw_list.push_text(TextCall {
        buffer_id: buffer,
        x: rect[0],
        y: rect[1] + (rect[3] - size) * 0.5 - 1.0,
        clip: rect,
        color: color_to_f32(color),
        z: Z_BASE,
    });
}

/// Draw the footer strip in `rect`: one row of command buttons, a "+N more"
/// button when they overflow, and a re-run control. Returns the clicked action.
pub fn draw_footer(
    core: &mut AkarCore,
    rect: [f32; 4],
    status: &FooterStatus<'_>,
    selected: Option<&Details>,
    theme: &AkarTheme,
) -> Option<Action> {
    if rect[2] <= 0.0 || rect[3] <= 0.0 || *status == FooterStatus::Hidden {
        return None;
    }
    push_quad(
        core,
        [rect[0], rect[1], rect[2], 1.0],
        theme.base_300,
        0,
        0.0,
        0.0,
    );
    let row = [
        rect[0],
        rect[1] + (rect[3] - FOOTER_BUTTON_HEIGHT) * 0.5 + 1.0,
        rect[2],
        FOOTER_BUTTON_HEIGHT,
    ];
    let mut action = None;
    let rerun_rect = [
        row[0] + row[2] - FOOTER_RERUN_WIDTH,
        row[1],
        FOOTER_RERUN_WIDTH,
        row[3],
    ];
    let text_room = [
        row[0],
        row[1],
        (row[2] - FOOTER_RERUN_WIDTH - FOOTER_BUTTON_GAP).max(0.0),
        row[3],
    ];
    let mut rerun = |core: &mut AkarCore| {
        if draw_button(
            core,
            FOOTER_BUFFER_BASE + 1,
            rerun_rect,
            "Re-run",
            Look::Outline,
            rect,
            theme,
        ) {
            action = Some(Action::Rerun(Section::Commands));
        }
    };
    match status {
        FooterStatus::Hidden => {}
        FooterStatus::Loading => footer_text(
            core,
            FOOTER_BUFFER_BASE,
            row,
            "Finding commands…",
            theme.muted_content,
            theme.font_size_sm,
        ),
        FooterStatus::Failed(message) => {
            footer_text(
                core,
                FOOTER_BUFFER_BASE,
                text_room,
                message,
                theme.error,
                theme.font_size_sm,
            );
            rerun(core);
        }
        FooterStatus::Empty => {
            footer_text(
                core,
                FOOTER_BUFFER_BASE,
                text_room,
                "No commands found for this project.",
                theme.muted_content,
                theme.font_size_sm,
            );
            rerun(core);
        }
        FooterStatus::Commands(commands) => {
            let widths: Vec<f32> = commands.iter().map(|c| button_width(&c.label)).collect();
            let available = text_room[2];
            let keep = visible_buttons(&widths, available, FOOTER_BUTTON_GAP);
            let mut x = row[0];
            let mut clicked = None;
            for (index, command) in commands.iter().take(keep).enumerate() {
                let button = [x, row[1], widths[index], row[3]];
                x += widths[index] + FOOTER_BUTTON_GAP;
                let look = if matches!(selected, Some(Details::Command(id)) if *id == command.id) {
                    Look::Active
                } else {
                    Look::Outline
                };
                if draw_button(
                    core,
                    FOOTER_BUFFER_BASE + 16 + index as u64,
                    button,
                    &command.label,
                    look,
                    rect,
                    theme,
                ) {
                    clicked = Some(Action::OpenCommand(command.id.clone()));
                }
            }
            if keep < commands.len() {
                let label = more_label(commands.len() - keep);
                let button = [x, row[1], button_width(&label), row[3]];
                let look = if matches!(selected, Some(Details::List)) {
                    Look::Active
                } else {
                    Look::Outline
                };
                if draw_button(
                    core,
                    FOOTER_BUFFER_BASE + 2,
                    button,
                    &label,
                    look,
                    rect,
                    theme,
                ) {
                    clicked = Some(Action::OpenList);
                }
            }
            rerun(core);
            if clicked.is_some() {
                action = clicked;
            }
        }
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::DAFTPROMPT_THEME;
    use akar_core::DrawCall;

    fn blocks() -> Vec<Block> {
        vec![
            Block {
                kind: BlockKind::Title(None),
                text: "Recent work".into(),
            },
            Block {
                kind: BlockKind::Body,
                text: "word ".repeat(300),
            },
            Block {
                kind: BlockKind::Citation,
                text: "[1] a.rs:L1".into(),
            },
        ]
    }

    #[test]
    fn layout_stacks_blocks_and_grows_with_text() {
        let narrow = build_layout(&blocks(), 300.0, &DAFTPROMPT_THEME);
        let wide = build_layout(&blocks(), 900.0, &DAFTPROMPT_THEME);
        assert!(narrow.total_height() > wide.total_height());
        assert!(narrow.items[1].y >= narrow.items[0].y + narrow.items[0].height);
        assert!(narrow.items[2].y >= narrow.items[1].y + narrow.items[1].height);
    }

    #[test]
    fn scrolled_text_stays_inside_the_viewport() {
        let layout = build_layout(&blocks(), 400.0, &DAFTPROMPT_THEME);
        let mut core = AkarCore::mock();
        core.begin_frame(800, 600, 1.0);
        let viewport = [20.0, 100.0, 400.0, 200.0];
        let mut scroll = 120.0;
        draw_blocks(&mut core, &layout, viewport, &mut scroll, &DAFTPROMPT_THEME);
        let mut drawn = 0;
        for call in core.draw_list.text_calls() {
            let DrawCall::Text(call) = call else { continue };
            drawn += 1;
            assert!(
                call.clip[1] >= viewport[1] - 0.01
                    && call.clip[1] + call.clip[3] <= viewport[1] + viewport[3] + 0.01,
                "text must not paint outside the viewport: {:?}",
                call.clip
            );
        }
        assert!(drawn > 0);
    }

    #[test]
    fn footer_draws_nothing_without_a_project() {
        let mut core = AkarCore::mock();
        core.begin_frame(800, 600, 1.0);
        let action = draw_footer(
            &mut core,
            [0.0, 500.0, 600.0, 44.0],
            &FooterStatus::Hidden,
            None,
            &DAFTPROMPT_THEME,
        );
        assert!(action.is_none());
        assert!(core.draw_list.text_calls().is_empty());
    }
}
