//! Painting for parsed source blocks.
//!
//! The visible content is a small, rebuilt-only-on-change layout: one block per
//! unlabelled gap or validated span, in source order. A separate layout lets
//! the whole content stack scroll under a fixed viewport without touching the
//! app's main layout. Heights for paragraphs use a conservative wrap estimate
//! because akar measures shaped text through its text pipeline; drawing uses
//! glyphon metrics with an explicit viewport clip so scrolled text never paints
//! over the app chrome.

use akar_components::{
    AkarTheme, BadgeVariant, TextStyle, akar_paragraph, scroll_area_begin, scroll_area_end,
};
use akar_core::{AkarCore, QuadCall, TextCall, Z_BASE};
use akar_layout::{
    Dimension, Display, FlexDirection, FlexWrap, Layout, NodeId, Rect, Size, Style, length,
};

use std::collections::BTreeMap;

use crate::clarify::{BlockKind, BranchMark, PanelBlock};
use crate::view::{Block, LabelView, SpanView};

const BLOCK_PADDING: f32 = 12.0;
const BLOCK_GAP: f32 = 10.0;
const BADGE_HEIGHT: f32 = 22.0;
const BADGE_GAP: f32 = 6.0;
const ANSWER_LINE_HEIGHT: f32 = 20.0;
/// Keep widget/buffer ids distinct from the app layout and future portals.
const CONTENT_NAMESPACE: u64 = 2;
const PANEL_NAMESPACE: u64 = 3;
const CLARIFY_BUTTON_WIDTH: f32 = 168.0;
const CLARIFY_BUTTON_HEIGHT: f32 = 28.0;
const PANEL_CARD_PADDING: f32 = 8.0;

/// One prepared block in the scrollable content layout.
struct Entry {
    node: NodeId,
    kind: EntryKind,
}

enum EntryKind {
    Unlabelled {
        text: String,
    },
    Span {
        quote: NodeId,
        quote_text: String,
        badges: Vec<BadgeNode>,
        answer_lines: Vec<(NodeId, String)>,
        /// The Clarify action and the `#N` it opens.
        clarify: NodeId,
        display_id: u32,
    },
}

struct BadgeNode {
    node: NodeId,
    text: String,
    variant: BadgeVariant,
}

/// A content layout computed for one result and width.
pub struct ContentLayout {
    layout: Layout,
    entries: Vec<Entry>,
    total_height: f32,
}

impl ContentLayout {
    /// The measured total scrollable height at the width it was built for.
    pub fn total_height(&self) -> f32 {
        self.total_height
    }
}

/// Build the block layout for `blocks` at `width`.
///
/// Returns `None` for an empty result or an unusably narrow viewport. This is
/// called only when a new result arrives or the main column is resized.
pub fn build_content(blocks: &[Block], width: f32, theme: &AkarTheme) -> Option<ContentLayout> {
    if blocks.is_empty() || width <= 8.0 {
        return None;
    }

    let mut layout = Layout::new();
    layout.set_namespace_id(CONTENT_NAMESPACE);
    let mut entries = Vec::with_capacity(blocks.len());
    let mut children = Vec::with_capacity(blocks.len());

    for block in blocks {
        match block {
            Block::Text { text } => {
                let height = estimated_text_height(
                    text,
                    width,
                    theme.font_size_base,
                    theme.font_size_base * 1.5,
                );
                let node = layout.new_leaf(Style {
                    size: Size {
                        width: Dimension::percent(1.0),
                        height: length(height),
                    },
                    ..Default::default()
                });
                children.push(node);
                entries.push(Entry {
                    node,
                    kind: EntryKind::Unlabelled { text: text.clone() },
                });
            }
            Block::Span(span) => {
                let (entry, node) = build_span(&mut layout, span, width, theme);
                children.push(node);
                entries.push(entry);
            }
        }
    }

    let root = layout.new_with_children(
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            size: Size {
                width: Dimension::percent(1.0),
                height: Dimension::auto(),
            },
            gap: Size {
                width: length(0.0_f32),
                height: length(BLOCK_GAP),
            },
            padding: Rect {
                top: length(0.0_f32),
                right: length(0.0_f32),
                bottom: length(24.0_f32),
                left: length(0.0_f32),
            },
            ..Default::default()
        },
        &children,
    );

    layout.compute(root, (Some(width), None), |_, _, _, _, _| Size::ZERO);
    let total_height = layout.rect(root)[3].max(0.0);

    Some(ContentLayout {
        layout,
        entries,
        total_height,
    })
}

fn build_span(
    layout: &mut Layout,
    span: &SpanView,
    width: f32,
    theme: &AkarTheme,
) -> (Entry, NodeId) {
    let inner_width = (width - 2.0 * BLOCK_PADDING).max(8.0);
    let mut children = Vec::new();
    let mut badges = Vec::new();

    let mut badge_specs = vec![(format!("#{}", span.display_id), BadgeVariant::Default)];
    badge_specs.extend(badges_for(&span.labels));
    if !badge_specs.is_empty() {
        let mut badge_nodes = Vec::with_capacity(badge_specs.len());
        for (text, variant) in badge_specs {
            let node = layout.new_leaf(Style {
                flex_shrink: 0.0,
                size: Size {
                    width: length(badge_width(&text, theme)),
                    height: length(BADGE_HEIGHT),
                },
                ..Default::default()
            });
            badges.push(BadgeNode {
                node,
                text: text.clone(),
                variant,
            });
            badge_nodes.push(node);
        }
        let badge_row = layout.new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                size: Size {
                    width: Dimension::percent(1.0),
                    height: Dimension::auto(),
                },
                gap: Size {
                    width: length(BADGE_GAP),
                    height: length(BADGE_GAP),
                },
                ..Default::default()
            },
            &badge_nodes,
        );
        children.push(badge_row);
    }

    let quote_height = estimated_text_height(
        &span.text,
        inner_width,
        theme.font_size_base,
        theme.font_size_base * 1.5,
    );
    let quote = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(quote_height),
        },
        ..Default::default()
    });
    children.push(quote);

    let mut answer_lines = Vec::new();
    for text in relationship_lines(span) {
        let height =
            estimated_text_height(&text, inner_width, theme.font_size_sm, ANSWER_LINE_HEIGHT);
        let node = layout.new_leaf(Style {
            size: Size {
                width: Dimension::percent(1.0),
                height: length(height),
            },
            ..Default::default()
        });
        children.push(node);
        answer_lines.push((node, text));
    }

    let clarify = layout.new_leaf(Style {
        flex_shrink: 0.0,
        size: Size {
            width: length(CLARIFY_BUTTON_WIDTH),
            height: length(CLARIFY_BUTTON_HEIGHT),
        },
        ..Default::default()
    });
    children.push(clarify);

    let node = layout.new_with_children(
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            size: Size {
                width: Dimension::percent(1.0),
                height: Dimension::auto(),
            },
            padding: Rect {
                top: length(BLOCK_PADDING),
                right: length(BLOCK_PADDING),
                bottom: length(BLOCK_PADDING),
                left: length(BLOCK_PADDING),
            },
            gap: Size {
                width: length(0.0_f32),
                height: length(8.0_f32),
            },
            ..Default::default()
        },
        &children,
    );

    (
        Entry {
            node,
            kind: EntryKind::Span {
                quote,
                quote_text: span.text.clone(),
                badges,
                answer_lines,
                clarify,
                display_id: span.display_id,
            },
        },
        node,
    )
}

fn relationship_lines(span: &SpanView) -> Vec<String> {
    let mut lines = Vec::new();
    for label in &span.labels {
        if let Some(target) = &label.target {
            let verb = if label.kind == daftprompt_types::LabelKind::Answer {
                "answers"
            } else {
                "recommendation for"
            };
            lines.push(format!("{verb} → {target}"));
        }
    }
    for (prefix, targets) in [
        ("answered by", &span.answered_by),
        ("recommendations", &span.recommended_by),
        ("alternative to", &span.alternatives),
    ] {
        for target in targets {
            lines.push(format!("{prefix}: {target}"));
        }
    }
    lines
}

/// Badge text and variant for one label: kind plus status, and an assignee
/// handle as its own badge.
fn badges_for(labels: &[LabelView]) -> Vec<(String, BadgeVariant)> {
    let mut badges = Vec::new();
    for label in labels {
        let (text, variant) = match label.kind {
            daftprompt_types::LabelKind::Question | daftprompt_types::LabelKind::Task => {
                let status = match label.status {
                    Some(daftprompt_types::LabelStatus::Resolved) => "resolved",
                    Some(daftprompt_types::LabelStatus::Deferred) => "deferred",
                    Some(daftprompt_types::LabelStatus::Dismissed) => "dismissed",
                    Some(daftprompt_types::LabelStatus::Open) | None => "open",
                };
                let variant = match label.status {
                    Some(daftprompt_types::LabelStatus::Resolved) => BadgeVariant::Success,
                    Some(daftprompt_types::LabelStatus::Deferred) => BadgeVariant::Info,
                    Some(daftprompt_types::LabelStatus::Dismissed) => BadgeVariant::Default,
                    Some(daftprompt_types::LabelStatus::Open) | None => BadgeVariant::Warning,
                };
                (
                    format!(
                        "{} · {status}",
                        if label.kind == daftprompt_types::LabelKind::Task {
                            "task"
                        } else {
                            "question"
                        }
                    ),
                    variant,
                )
            }
            daftprompt_types::LabelKind::Recommendation => {
                ("recommendation".to_owned(), BadgeVariant::Primary)
            }
            daftprompt_types::LabelKind::RecommendationFor => {
                ("recommendation for".to_owned(), BadgeVariant::Primary)
            }
            daftprompt_types::LabelKind::AlternativeTo => {
                ("alternative".to_owned(), BadgeVariant::Primary)
            }

            daftprompt_types::LabelKind::Answer => ("answer".to_owned(), BadgeVariant::Primary),
            daftprompt_types::LabelKind::Conflict => ("conflict".to_owned(), BadgeVariant::Error),
            daftprompt_types::LabelKind::Supersedes => {
                ("supersedes".to_owned(), BadgeVariant::Warning)
            }
            daftprompt_types::LabelKind::Stale => ("stale".to_owned(), BadgeVariant::Error),
            daftprompt_types::LabelKind::Clarification => {
                ("clarification".to_owned(), BadgeVariant::Info)
            }
        };
        badges.push((text, variant));
        if let Some(assignee) = &label.assignee {
            badges.push((assignee.clone(), BadgeVariant::Info));
        }
    }
    badges
}

fn badge_width(text: &str, theme: &AkarTheme) -> f32 {
    let characters = text.chars().count() as f32;
    (characters * theme.font_size_sm * 0.56 + 22.0).clamp(48.0, 240.0)
}

/// Conservative paragraph height: assume a wider-than-average glyph so the
/// estimate errs towards extra space rather than clipping.
pub fn estimated_text_height(text: &str, width: f32, font_size: f32, line_height: f32) -> f32 {
    if width <= 0.0 {
        return line_height.max(1.0);
    }
    let average_character_width = (font_size * 0.62).max(1.0);
    let characters_per_line = (width / average_character_width).floor().max(1.0) as usize;

    let lines: usize = text
        .split('\n')
        .map(|line| {
            let characters = line.chars().count();
            if characters == 0 {
                1
            } else {
                characters.div_ceil(characters_per_line)
            }
        })
        .sum();

    lines.max(1) as f32 * line_height + 2.0
}

/// Draw a successful result into `rect`, applying scroll and clipping.
///
/// `marks` says which spans have a clarification branch and `open` which one the
/// panel shows. Returns the `#N` whose Clarify action was clicked this frame.
pub fn draw_content(
    core: &mut AkarCore,
    content: &mut ContentLayout,
    rect: [f32; 4],
    scroll_y: &mut f32,
    theme: &AkarTheme,
    marks: &BTreeMap<u32, BranchMark>,
    open: Option<u32>,
) -> Option<u32> {
    let mut clicked_span = None;
    if rect[2] <= 0.0 || rect[3] <= 0.0 {
        return clicked_span;
    }

    let response = scroll_area_begin(core, rect, scroll_y, content.total_height());
    content
        .layout
        .set_screen_origin([rect[0], response.content_y]);

    for entry in &content.entries {
        match &entry.kind {
            EntryKind::Unlabelled { text } => {
                draw_wrapped_text(
                    core,
                    &content.layout,
                    entry.node,
                    text,
                    theme.font_size_base,
                    theme.font_size_base * 1.5,
                    theme.muted_content,
                    rect,
                );
            }
            EntryKind::Span {
                quote,
                quote_text,
                badges,
                answer_lines,
                clarify,
                display_id,
            } => {
                draw_panel(core, &content.layout, entry.node, rect, theme);
                let mark = marks.get(display_id);
                if mark.is_some() {
                    draw_branch_marker(core, &content.layout, entry.node, rect, theme);
                }
                for badge in badges {
                    draw_badge(
                        core,
                        &content.layout,
                        badge.node,
                        &badge.text,
                        badge.variant,
                        rect,
                        theme,
                    );
                }
                draw_wrapped_text(
                    core,
                    &content.layout,
                    *quote,
                    quote_text,
                    theme.font_size_base,
                    theme.font_size_base * 1.5,
                    theme.base_content,
                    rect,
                );
                for (node, text) in answer_lines {
                    draw_wrapped_text(
                        core,
                        &content.layout,
                        *node,
                        text,
                        theme.font_size_sm,
                        ANSWER_LINE_HEIGHT,
                        theme.info,
                        rect,
                    );
                }
                let label = clarify_label(mark, open == Some(*display_id));
                if draw_clarify_button(
                    core,
                    &content.layout,
                    *clarify,
                    &label,
                    mark.is_some(),
                    rect,
                    theme,
                ) {
                    clicked_span = Some(*display_id);
                }
            }
        }
    }

    scroll_area_end(core);
    draw_scrollbar(core, rect, *scroll_y, content.total_height(), theme);
    clicked_span
}

/// The Clarify button's text: the action, or the branch it reopens.
pub fn clarify_label(mark: Option<&BranchMark>, open: bool) -> String {
    match mark {
        None => "Clarify".to_owned(),
        Some(mark) if mark.pending => "Branch · asking…".to_owned(),
        Some(mark) => format!(
            "Branch · {} turn{}{}",
            mark.turns,
            if mark.turns == 1 { "" } else { "s" },
            if open { " (open)" } else { "" }
        ),
    }
}

/// A thin accent along the left edge of a card that has a branch.
fn draw_branch_marker(
    core: &mut AkarCore,
    layout: &Layout,
    node: NodeId,
    viewport: [f32; 4],
    theme: &AkarTheme,
) {
    let card = layout.rect(node);
    let bar = intersect([card[0], card[1], 4.0, card[3]], viewport);
    if bar[2] <= 0.0 || bar[3] <= 0.0 {
        return;
    }
    core.draw_list.push_quad(QuadCall {
        rect: bar,
        fill: color_to_f32(theme.info),
        border_color: [0.0; 4],
        corner_radii: [2.0; 4],
        border_width: 0.0,
        z: Z_BASE,
        shadow_blur: 0.0,
        shadow_spread: 0.0,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        _pad: [0.0; 2],
    });
}

/// Draw a button inside the scroll area. Hit testing uses the part inside the
/// viewport, so a partly scrolled button cannot be clicked through the header.
fn draw_clarify_button(
    core: &mut AkarCore,
    layout: &Layout,
    node: NodeId,
    label: &str,
    has_branch: bool,
    viewport: [f32; 4],
    theme: &AkarTheme,
) -> bool {
    let rect = layout.rect(node);
    let clipped = intersect(rect, viewport);
    if clipped[2] <= 0.0 || clipped[3] <= 0.0 {
        return false;
    }
    let hovered = core.input.is_hovering(clipped);
    let clicked = core.input.is_clicked(clipped);
    let (fill, border, content) = if has_branch {
        (theme.info, theme.info, theme.info_content)
    } else if hovered {
        (theme.base_300, theme.primary, theme.base_content)
    } else {
        (0x0000_0000, theme.primary, theme.base_content)
    };
    core.draw_list.push_quad(QuadCall {
        rect: clipped,
        fill: color_to_f32(fill),
        border_color: color_to_f32(border),
        corner_radii: [theme.radius_field; 4],
        border_width: theme.border_width,
        z: Z_BASE,
        shadow_blur: 0.0,
        shadow_spread: 0.0,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        _pad: [0.0; 2],
    });
    let font_size = theme.font_size_sm;
    let buffer = core.text_pipeline.set_text_transient(
        layout.widget_id(node),
        label,
        glyphon::Metrics::new(font_size, font_size * 1.2),
        None,
        None,
        None,
    );
    core.draw_list.push_text(TextCall {
        buffer_id: buffer,
        x: rect[0] + 10.0,
        y: rect[1] + (rect[3] - font_size) * 0.5,
        clip: clipped,
        color: color_to_f32(content),
        z: Z_BASE,
    });
    clicked
}

/// Draw wrapped text clipped to the scroll viewport.
///
/// akar components clip text to their own node rectangle; a scrolled block can
/// extend outside the viewport, so the content view sets the clip explicitly.
#[allow(clippy::too_many_arguments)]
fn draw_wrapped_text(
    core: &mut AkarCore,
    layout: &Layout,
    node: NodeId,
    text: &str,
    font_size: f32,
    line_height: f32,
    color: u32,
    viewport: [f32; 4],
) {
    draw_text_at(
        core,
        layout.widget_id(node),
        layout.rect(node),
        text,
        font_size,
        line_height,
        color,
        viewport,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_text_at(
    core: &mut AkarCore,
    widget_id: u64,
    rect: [f32; 4],
    text: &str,
    font_size: f32,
    line_height: f32,
    color: u32,
    viewport: [f32; 4],
) {
    if text.is_empty() || rect[2] <= 0.0 || !intersects(rect, viewport) {
        return;
    }

    let buffer = core.text_pipeline.set_text_transient(
        widget_id,
        text,
        glyphon::Metrics::new(font_size, line_height),
        Some(rect[2]),
        None,
        None,
    );
    core.draw_list.push_text(TextCall {
        buffer_id: buffer,
        x: rect[0],
        y: rect[1],
        clip: viewport,
        color: color_to_f32(color),
        z: Z_BASE,
    });
}

fn draw_panel(
    core: &mut AkarCore,
    layout: &Layout,
    node: NodeId,
    viewport: [f32; 4],
    theme: &AkarTheme,
) {
    let rect = intersect(layout.rect(node), viewport);
    if rect[2] <= 0.0 || rect[3] <= 0.0 {
        return;
    }
    core.draw_list.push_quad(QuadCall {
        rect,
        fill: color_to_f32(theme.base_200),
        border_color: color_to_f32(theme.base_300),
        corner_radii: [theme.radius_box; 4],
        border_width: theme.border_width,
        z: Z_BASE,
        shadow_blur: 0.0,
        shadow_spread: 0.0,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        _pad: [0.0; 2],
    });
}

fn draw_badge(
    core: &mut AkarCore,
    layout: &Layout,
    node: NodeId,
    text: &str,
    variant: BadgeVariant,
    viewport: [f32; 4],
    theme: &AkarTheme,
) {
    if text.is_empty() {
        return;
    }
    let rect = layout.rect(node);
    let clipped = intersect(rect, viewport);
    if clipped[2] <= 0.0 || clipped[3] <= 0.0 {
        return;
    }

    let (fill, content) = badge_colors(variant, theme);
    core.draw_list.push_quad(QuadCall {
        rect: clipped,
        fill: color_to_f32(fill),
        border_color: [0.0; 4],
        corner_radii: [theme.radius_field; 4],
        border_width: 0.0,
        z: Z_BASE,
        shadow_blur: 0.0,
        shadow_spread: 0.0,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        _pad: [0.0; 2],
    });

    let font_size = theme.font_size_sm * 0.9;
    let buffer = core.text_pipeline.set_text_transient(
        layout.widget_id(node),
        text,
        glyphon::Metrics::new(font_size, font_size * 1.2),
        None,
        None,
        None,
    );
    core.draw_list.push_text(TextCall {
        buffer_id: buffer,
        x: rect[0] + 8.0,
        y: rect[1] + (rect[3] - font_size) * 0.5,
        clip: clipped,
        color: color_to_f32(content),
        z: Z_BASE,
    });
}

fn badge_colors(variant: BadgeVariant, theme: &AkarTheme) -> (u32, u32) {
    match variant {
        BadgeVariant::Default => (theme.base_300, theme.base_content),
        BadgeVariant::Primary => (theme.primary, theme.primary_content),
        BadgeVariant::Success => (theme.success, theme.success_content),
        BadgeVariant::Warning => (theme.warning, theme.warning_content),
        BadgeVariant::Error => (theme.error, theme.error_content),
        BadgeVariant::Info => (theme.info, theme.info_content),
    }
}

fn intersects(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && a[0] + a[2] > b[0] && a[1] < b[1] + b[3] && a[1] + a[3] > b[1]
}

fn intersect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let x = a[0].max(b[0]);
    let y = a[1].max(b[1]);
    let right = (a[0] + a[2]).min(b[0] + b[2]);
    let bottom = (a[1] + a[3]).min(b[1] + b[3]);
    [x, y, (right - x).max(0.0), (bottom - y).max(0.0)]
}

/// A prepared clarification panel: the branch's blocks stacked at one width.
pub struct PanelLayout {
    layout: Layout,
    entries: Vec<PanelEntry>,
    total_height: f32,
}

struct PanelEntry {
    node: NodeId,
    kind: BlockKind,
    text: String,
}

impl PanelLayout {
    pub fn total_height(&self) -> f32 {
        self.total_height
    }
}

/// Font size, line height, color, and whether the block sits on a card.
fn panel_block_style(kind: BlockKind, theme: &AkarTheme) -> (f32, f32, u32, bool) {
    let base = theme.font_size_base;
    let small = theme.font_size_sm;
    match kind {
        BlockKind::Target => (small, small * 1.5, theme.muted_content, true),
        BlockKind::Question => (base, base * 1.5, theme.info, false),
        BlockKind::Explanation => (base, base * 1.5, theme.base_content, true),
        BlockKind::Citation => (small, small * 1.5, theme.muted_content, false),
        BlockKind::Meta => (small, small * 1.5, theme.muted_content, false),
        BlockKind::Notice => (small, small * 1.5, theme.muted_content, false),
        BlockKind::Error => (base, base * 1.5, theme.error, false),
    }
}

/// Build the panel layout for `blocks` at `width`. Returns `None` for no
/// blocks or an unusably narrow panel. Called when the branch or width changes.
pub fn build_panel(blocks: &[PanelBlock], width: f32, theme: &AkarTheme) -> Option<PanelLayout> {
    if blocks.is_empty() || width <= 8.0 {
        return None;
    }
    let mut layout = Layout::new();
    layout.set_namespace_id(PANEL_NAMESPACE);
    let mut entries = Vec::with_capacity(blocks.len());
    let mut children = Vec::with_capacity(blocks.len());
    for block in blocks {
        let (font_size, line_height, _, card) = panel_block_style(block.kind, theme);
        let pad = if card { PANEL_CARD_PADDING } else { 0.0 };
        let height = estimated_text_height(
            &block.text,
            (width - 2.0 * pad).max(8.0),
            font_size,
            line_height,
        ) + 2.0 * pad;
        let node = layout.new_leaf(Style {
            size: Size {
                width: Dimension::percent(1.0),
                height: length(height),
            },
            ..Default::default()
        });
        children.push(node);
        entries.push(PanelEntry {
            node,
            kind: block.kind,
            text: block.text.clone(),
        });
    }
    let root = layout.new_with_children(
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            size: Size {
                width: Dimension::percent(1.0),
                height: Dimension::auto(),
            },
            gap: Size {
                width: length(0.0_f32),
                height: length(8.0_f32),
            },
            padding: Rect {
                top: length(0.0_f32),
                right: length(8.0_f32),
                bottom: length(16.0_f32),
                left: length(0.0_f32),
            },
            ..Default::default()
        },
        &children,
    );
    layout.compute(root, (Some(width), None), |_, _, _, _, _| Size::ZERO);
    let total_height = layout.rect(root)[3].max(0.0);
    Some(PanelLayout {
        layout,
        entries,
        total_height,
    })
}

/// Draw the panel into `rect` with its own scroll offset and clipping.
pub fn draw_panel_content(
    core: &mut AkarCore,
    panel: &mut PanelLayout,
    rect: [f32; 4],
    scroll_y: &mut f32,
    theme: &AkarTheme,
) {
    if rect[2] <= 0.0 || rect[3] <= 0.0 {
        return;
    }
    let response = scroll_area_begin(core, rect, scroll_y, panel.total_height());
    panel
        .layout
        .set_screen_origin([rect[0], response.content_y]);
    for entry in &panel.entries {
        let (font_size, line_height, color, card) = panel_block_style(entry.kind, theme);
        let mut text_rect = panel.layout.rect(entry.node);
        if card {
            draw_panel(core, &panel.layout, entry.node, rect, theme);
            text_rect = [
                text_rect[0] + PANEL_CARD_PADDING,
                text_rect[1] + PANEL_CARD_PADDING,
                (text_rect[2] - 2.0 * PANEL_CARD_PADDING).max(1.0),
                (text_rect[3] - 2.0 * PANEL_CARD_PADDING).max(1.0),
            ];
        }
        draw_text_at(
            core,
            panel.layout.widget_id(entry.node),
            text_rect,
            &entry.text,
            font_size,
            line_height,
            color,
            rect,
        );
    }
    scroll_area_end(core);
    draw_scrollbar(core, rect, *scroll_y, panel.total_height(), theme);
}

/// Draw a non-success state: idle, loading, or an explicit error.
pub fn draw_notice(
    core: &mut AkarCore,
    layout: &Layout,
    node: NodeId,
    text: &str,
    color: u32,
    theme: &AkarTheme,
) {
    let style = TextStyle {
        color: Some(color),
        font_size: Some(theme.font_size_base),
        line_height: Some(theme.font_size_base * 1.5),
        ..TextStyle::empty()
    };
    akar_paragraph(core, layout, node, text, Some(style), theme);
}

fn draw_scrollbar(
    core: &mut AkarCore,
    rect: [f32; 4],
    scroll_y: f32,
    content_height: f32,
    theme: &AkarTheme,
) {
    if content_height <= rect[3] || rect[3] <= 0.0 {
        return;
    }
    let max_scroll = content_height - rect[3];
    let thumb_height = (rect[3] * rect[3] / content_height).max(24.0).min(rect[3]);
    let travel = rect[3] - thumb_height;
    let ratio = if max_scroll > 0.0 {
        (scroll_y / max_scroll).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb = [
        rect[0] + rect[2] - 6.0,
        rect[1] + travel * ratio,
        4.0,
        thumb_height,
    ];

    core.draw_list.push_quad(QuadCall {
        rect: thumb,
        fill: color_to_f32(theme.base_300),
        border_color: [0.0; 4],
        corner_radii: [2.0; 4],
        border_width: 0.0,
        z: Z_BASE + 0.1,
        shadow_blur: 0.0,
        shadow_spread: 0.0,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        _pad: [0.0; 2],
    });
}

fn color_to_f32(color: u32) -> [f32; 4] {
    [
        ((color >> 24) & 0xFF) as f32 / 255.0,
        ((color >> 16) & 0xFF) as f32 / 255.0,
        ((color >> 8) & 0xFF) as f32 / 255.0,
        (color & 0xFF) as f32 / 255.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{LabelView, SpanView};
    use akar_components::AKAR_THEME_DARK;
    use akar_core::DrawCall;
    use daftprompt_types::{LabelKind, LabelStatus};

    fn span(text: &str) -> Block {
        Block::Span(SpanView {
            display_id: 1,
            node_id: "n".to_owned(),
            start_byte: 0,
            end_byte: text.len(),
            text: text.to_owned(),
            labels: vec![LabelView {
                kind: LabelKind::Question,
                status: Some(LabelStatus::Open),
                assignee: Some("@infra-team".to_owned()),
                target: None,
            }],
            answered_by: vec!["#2 · A first answer".to_owned()],
            recommended_by: Vec::new(),
            alternatives: Vec::new(),
        })
    }

    #[test]
    fn taller_text_needs_more_height() {
        let short = estimated_text_height("hello", 300.0, 16.0, 24.0);
        let long = estimated_text_height(&"word ".repeat(200), 300.0, 16.0, 24.0);
        assert!(long > short, "{long} should be taller than {short}");
    }

    #[test]
    fn explicit_newlines_add_lines() {
        let one = estimated_text_height("a", 300.0, 16.0, 24.0);
        let three = estimated_text_height("a\nb\nc", 300.0, 16.0, 24.0);
        assert!(three > one);
    }

    #[test]
    fn badges_show_status_and_assignee() {
        let badges = badges_for(&[LabelView {
            kind: LabelKind::Question,
            status: Some(LabelStatus::Resolved),
            assignee: Some("@team".to_owned()),
            target: None,
        }]);
        assert_eq!(badges.len(), 2);
        assert_eq!(
            badges[0],
            ("question · resolved".to_owned(), BadgeVariant::Success)
        );
        assert_eq!(badges[1], ("@team".to_owned(), BadgeVariant::Info));
    }

    #[test]
    fn new_badges_and_relationship_lines_are_distinct() {
        let Block::Span(mut span) = span("Suggestion") else {
            unreachable!()
        };
        span.labels = vec![
            LabelView {
                kind: LabelKind::Task,
                status: Some(LabelStatus::Open),
                assignee: Some("@alex".into()),
                target: None,
            },
            LabelView {
                kind: LabelKind::Recommendation,
                status: None,
                assignee: None,
                target: None,
            },
        ];
        span.recommended_by = vec!["#3 · Suggested option".into()];
        span.alternatives = vec!["#4 · Other option".into()];
        let badges = badges_for(&span.labels);
        assert_eq!(badges[0].0, "task · open");
        assert_eq!(badges[1].0, "@alex");
        assert_eq!(badges[2].0, "recommendation");
        assert_eq!(
            relationship_lines(&span),
            [
                "answered by: #2 · A first answer",
                "recommendations: #3 · Suggested option",
                "alternative to: #4 · Other option"
            ]
        );
        let content = build_content(&[Block::Span(span)], 500.0, &AKAR_THEME_DARK).unwrap();
        let EntryKind::Span { badges, .. } = &content.entries[0].kind else {
            unreachable!()
        };
        assert_eq!(badges[0].text, "#1");
    }

    #[test]
    fn content_layout_stacks_blocks_and_answers() {
        let blocks = vec![
            Block::Text {
                text: "Intro".to_owned(),
            },
            span("Should we cache tokens?"),
        ];
        let content = build_content(&blocks, 500.0, &AKAR_THEME_DARK)
            .expect("content should build for a normal width");
        assert!(content.total_height() > 100.0);
        assert_eq!(content.entries.len(), 2);
        // The span has an answered-by line in addition to its badge row and quote.
        let EntryKind::Span { answer_lines, .. } = &content.entries[1].kind else {
            panic!("expected a span entry");
        };
        assert_eq!(answer_lines.len(), 1);

        let first = content.layout.rect(content.entries[0].node);
        let second = content.layout.rect(content.entries[1].node);
        assert!(
            second[1] >= first[1] + first[3],
            "blocks stack vertically with a gap"
        );
        assert_eq!(second[2], 500.0, "blocks use the full content width");
        let EntryKind::Span { quote, badges, .. } = &content.entries[1].kind else {
            panic!("expected a span entry");
        };
        assert!(content.layout.rect(*quote)[2] > 300.0);
        let badge_rect = content.layout.rect(badges[0].node);
        assert!(badge_rect[2] > 0.0 && badge_rect[3] > 0.0);
        assert!(
            badge_rect[1] >= second[1] && badge_rect[1] < second[1] + second[3],
            "badges sit inside their span card"
        );
        assert!(
            content.layout.rect(*quote)[1] > badge_rect[1],
            "the quote follows the badge row"
        );
    }

    #[test]
    fn content_text_is_clipped_to_the_scroll_viewport() {
        let blocks = vec![
            Block::Text {
                text: "Intro text before the question.".to_owned(),
            },
            span("Should we cache tokens?"),
        ];
        let mut content = build_content(&blocks, 500.0, &AKAR_THEME_DARK)
            .expect("content should build for a normal width");
        let mut core = AkarCore::mock();
        core.begin_frame(800, 600, 1.0);
        let viewport = [20.0, 100.0, 400.0, 220.0];
        let mut scroll = 0.0;
        draw_content(
            &mut core,
            &mut content,
            viewport,
            &mut scroll,
            &AKAR_THEME_DARK,
            &BTreeMap::new(),
            None,
        );

        let text_calls = core.draw_list.text_calls();
        assert!(!text_calls.is_empty(), "content should draw text");
        let mut viewport_clips = 0;
        for call in text_calls {
            let DrawCall::Text(call) = call else {
                continue;
            };
            assert!(
                call.clip[0] >= viewport[0]
                    && call.clip[1] >= viewport[1]
                    && call.clip[0] + call.clip[2] <= viewport[0] + viewport[2] + 0.01
                    && call.clip[1] + call.clip[3] <= viewport[1] + viewport[3] + 0.01,
                "block text must not paint outside the viewport: {:?}",
                call.clip
            );
            if call.clip == viewport {
                viewport_clips += 1;
            }
        }
        assert!(
            viewport_clips > 0,
            "paragraph text uses the full viewport clip"
        );
    }

    #[test]
    fn span_cards_get_a_clarify_action_inside_the_card() {
        let content = build_content(&[span("Should we cache tokens?")], 500.0, &AKAR_THEME_DARK)
            .expect("content");
        let EntryKind::Span {
            clarify,
            display_id,
            ..
        } = &content.entries[0].kind
        else {
            unreachable!()
        };
        assert_eq!(*display_id, 1);
        let card = content.layout.rect(content.entries[0].node);
        let button = content.layout.rect(*clarify);
        assert!(button[2] > 0.0 && button[3] > 0.0);
        assert!(button[1] + button[3] <= card[1] + card[3]);
    }

    #[test]
    fn branch_marker_labels_describe_the_branch() {
        assert_eq!(clarify_label(None, false), "Clarify");
        let mark = BranchMark {
            turns: 2,
            pending: false,
        };
        assert_eq!(clarify_label(Some(&mark), true), "Branch · 2 turns (open)");
        let one = BranchMark {
            turns: 1,
            pending: false,
        };
        assert_eq!(clarify_label(Some(&one), false), "Branch · 1 turn");
        let pending = BranchMark {
            turns: 0,
            pending: true,
        };
        assert_eq!(clarify_label(Some(&pending), false), "Branch · asking…");
    }

    #[test]
    fn the_panel_stacks_blocks_and_clips_to_its_viewport() {
        let blocks = vec![
            PanelBlock {
                kind: BlockKind::Target,
                text: "#1 · Should we cache tokens?".to_owned(),
            },
            PanelBlock {
                kind: BlockKind::Question,
                text: "Q: why?".to_owned(),
            },
            PanelBlock {
                kind: BlockKind::Explanation,
                text: "Because of cost [1]. ".repeat(20),
            },
            PanelBlock {
                kind: BlockKind::Citation,
                text: "[1] src/cache.rs:L1–L9".to_owned(),
            },
        ];
        let mut panel = build_panel(&blocks, 320.0, &AKAR_THEME_DARK).expect("panel");
        let first = panel.layout.rect(panel.entries[0].node);
        let third = panel.layout.rect(panel.entries[2].node);
        assert!(third[1] > first[1] + first[3]);
        assert!(third[3] > first[3], "a long explanation is taller");

        let mut core = AkarCore::mock();
        core.begin_frame(800, 600, 1.0);
        let viewport = [400.0, 100.0, 320.0, 150.0];
        let mut scroll = 0.0;
        draw_panel_content(
            &mut core,
            &mut panel,
            viewport,
            &mut scroll,
            &AKAR_THEME_DARK,
        );
        for call in core.draw_list.text_calls() {
            let DrawCall::Text(call) = call else { continue };
            assert_eq!(call.clip, viewport);
        }
        assert!(build_panel(&[], 320.0, &AKAR_THEME_DARK).is_none());
        assert!(build_panel(&blocks, 2.0, &AKAR_THEME_DARK).is_none());
    }

    #[test]
    fn empty_blocks_or_narrow_widths_do_not_build() {
        assert!(build_content(&[], 500.0, &AKAR_THEME_DARK).is_none());
        assert!(
            build_content(
                &[Block::Text {
                    text: "x".to_owned()
                }],
                2.0,
                &AKAR_THEME_DARK
            )
            .is_none()
        );
    }
}
