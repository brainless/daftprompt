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

use crate::view::{Block, LabelView, SpanView};

const BLOCK_PADDING: f32 = 12.0;
const BLOCK_GAP: f32 = 10.0;
const BADGE_HEIGHT: f32 = 22.0;
const BADGE_GAP: f32 = 6.0;
const ANSWER_LINE_HEIGHT: f32 = 20.0;
/// Keep widget/buffer ids distinct from the app layout and future portals.
const CONTENT_NAMESPACE: u64 = 2;

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

    let badge_specs = badges_for(&span.labels);
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
    for label in &span.labels {
        if let Some(target) = &label.target {
            let text = format!("answers → {target}");
            let node = layout.new_leaf(Style {
                size: Size {
                    width: Dimension::percent(1.0),
                    height: length(ANSWER_LINE_HEIGHT),
                },
                ..Default::default()
            });
            children.push(node);
            answer_lines.push((node, text));
        }
    }
    for answer in &span.answered_by {
        let text = format!("answered by: {answer}");
        let node = layout.new_leaf(Style {
            size: Size {
                width: Dimension::percent(1.0),
                height: length(ANSWER_LINE_HEIGHT),
            },
            ..Default::default()
        });
        children.push(node);
        answer_lines.push((node, text));
    }

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
            },
        },
        node,
    )
}

/// Badge text and variant for one label: kind plus status, and an assignee
/// handle as its own badge.
fn badges_for(labels: &[LabelView]) -> Vec<(String, BadgeVariant)> {
    let mut badges = Vec::new();
    for label in labels {
        let (text, variant) = match label.kind {
            daftprompt_types::LabelKind::Question => {
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
                (format!("question · {status}"), variant)
            }
            daftprompt_types::LabelKind::Answer => ("answer".to_owned(), BadgeVariant::Primary),
            daftprompt_types::LabelKind::Conflict => ("conflict".to_owned(), BadgeVariant::Error),
            daftprompt_types::LabelKind::Supersedes => {
                ("supersedes".to_owned(), BadgeVariant::Warning)
            }
            daftprompt_types::LabelKind::Stale => ("stale".to_owned(), BadgeVariant::Error),
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
pub fn draw_content(
    core: &mut AkarCore,
    content: &mut ContentLayout,
    rect: [f32; 4],
    scroll_y: &mut f32,
    theme: &AkarTheme,
) {
    if rect[2] <= 0.0 || rect[3] <= 0.0 {
        return;
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
            } => {
                draw_panel(core, &content.layout, entry.node, rect, theme);
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
            }
        }
    }

    scroll_area_end(core);
    draw_scrollbar(core, rect, *scroll_y, content.total_height(), theme);
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
    if text.is_empty() {
        return;
    }
    let rect = layout.rect(node);
    if rect[2] <= 0.0 || !intersects(rect, viewport) {
        return;
    }

    let buffer = core.text_pipeline.set_text_transient(
        layout.widget_id(node),
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
            answered_by: vec!["A first answer".to_owned()],
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
