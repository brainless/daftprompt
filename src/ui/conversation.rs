// Conversation surface (Task 5, Epic 014).
//
// Full-window panel toggled via Tab key. Contains a header bar with adapter
// status, a scrollable transcript, a prompt editor, and a collapsible
// enrichment inspector. A permission dialog modal overlays the panel when a
// permission round-trip is pending.
//
// Layout strategy: rootless taffy sub-tree (same pattern as `render_search`
// and `render_drawer`). The panel covers the entire window; children are
// absolute-positioned top-to-bottom.

#![allow(float_literal_f32_fallback)]

use akar_components::{
    akar_badge as badge, akar_button as button, akar_container as container, akar_label as label,
    akar_text_input as text_input, modal_begin, modal_end, scroll_area_begin, scroll_area_end,
    BadgeVariant, BoxStyle, ButtonVariant,
};
use akar_core::AkarCore;
use akar_layout::{auto, length, Layout, Position, Rect, Size, Style};

use crate::state::{self, AppState, TranscriptEntryKind};
use crate::ui::render::color_to_f32;

const HEADER_HEIGHT: f32 = 44.0;
const PROMPT_BAR_HEIGHT: f32 = 50.0;
const PAD: f32 = 10.0;
const ENTRY_LINE_HEIGHT: f32 = 18.0;
const ENTRY_GAP: f32 = 8.0;
const BUTTON_WIDTH: f32 = 70.0;
const BADGE_WIDTH: f32 = 80.0;
const CLOSE_BUTTON_WIDTH: f32 = 32.0;
const COPY_BUTTON_WIDTH: f32 = 112.0;

/// Renders the conversation panel (full-window overlay).
///
/// The panel contains, top-to-bottom:
///   1. Header bar: adapter status badge, adapter name, new-session button, close button
///   2. Transcript area: scrollable list of transcript entries
///   3. Prompt editor: text input + Send/Cancel buttons
///   4. Enrichment inspector toggle + collapsible content
///
/// When a permission dialog is pending, a modal overlays the panel.
pub fn render_conversation(core: &mut AkarCore, layout: &mut Layout, state: &mut AppState) {
    if !state.conversation.visible {
        return;
    }

    let theme = match state.system_theme {
        state::SystemTheme::Dark => akar_components::AKAR_THEME_DARK,
        state::SystemTheme::Light => akar_components::AKAR_THEME_LIGHT,
    };

    let win_w = state.window_size.x;
    let win_h = state.window_size.y;

    // ── Rootless panel node ──
    let panel_node = layout.new_leaf(Style {
        position: Position::Absolute,
        size: Size {
            width: length(win_w),
            height: length(win_h),
        },
        ..Default::default()
    });

    // ── 1. Header bar ──
    let header_node = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(0.0),
            top: length(0.0),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(win_w),
            height: length(HEADER_HEIGHT),
        },
        ..Default::default()
    });
    layout.add_child(panel_node, header_node);

    // Status badge (left side)
    let status_badge = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(PAD),
            top: length(12.0),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(BADGE_WIDTH),
            height: length(20.0),
        },
        ..Default::default()
    });
    layout.add_child(header_node, status_badge);

    // Adapter name label
    let adapter_label = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(PAD + BADGE_WIDTH + 8.0),
            top: length(12.0),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(200.0),
            height: length(20.0),
        },
        ..Default::default()
    });
    layout.add_child(header_node, adapter_label);

    // Close button (right side)
    let close_btn = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(win_w - CLOSE_BUTTON_WIDTH - PAD),
            top: length(8.0),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(CLOSE_BUTTON_WIDTH),
            height: length(28.0),
        },
        ..Default::default()
    });
    layout.add_child(header_node, close_btn);

    // Inspector toggle button (right side, left of close)
    let inspector_btn = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(win_w - CLOSE_BUTTON_WIDTH - PAD - 120.0 - 8.0),
            top: length(8.0),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(120.0),
            height: length(28.0),
        },
        ..Default::default()
    });
    layout.add_child(header_node, inspector_btn);

    // ── 2. Transcript area ──
    let transcript_y = HEADER_HEIGHT;
    let inspector_h = if state.conversation.enrichment_inspector_visible
        && state.conversation.enrichment_inspector.is_some()
    {
        200.0
    } else {
        0.0
    };
    let transcript_h = (win_h - HEADER_HEIGHT - PROMPT_BAR_HEIGHT - inspector_h).max(0.0);
    let transcript_node = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(0.0),
            top: length(transcript_y),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(win_w),
            height: length(transcript_h),
        },
        ..Default::default()
    });
    layout.add_child(panel_node, transcript_node);

    // Create transcript entry label nodes inside the scroll area.
    // We'll compute the panel subtree first, then render.

    // ── 3. Prompt bar ──
    let prompt_y = win_h - PROMPT_BAR_HEIGHT;
    let prompt_bar = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(0.0),
            top: length(prompt_y),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(win_w),
            height: length(PROMPT_BAR_HEIGHT),
        },
        ..Default::default()
    });
    layout.add_child(panel_node, prompt_bar);

    // Prompt input (most of the bar width)
    let has_active_turn = state.conversation.active_turn_id.is_some();
    let input_w = win_w - PAD - BUTTON_WIDTH - PAD - (if has_active_turn { BUTTON_WIDTH + PAD } else { 0.0 }) - PAD;
    let prompt_input_node = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(PAD),
            top: length(8.0),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(input_w.max(100.0)),
            height: length(34.0),
        },
        ..Default::default()
    });
    layout.add_child(prompt_bar, prompt_input_node);

    // Send button
    let send_btn_x = PAD + input_w.max(100.0) + PAD;
    let send_btn = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(send_btn_x),
            top: length(8.0),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(BUTTON_WIDTH),
            height: length(34.0),
        },
        ..Default::default()
    });
    layout.add_child(prompt_bar, send_btn);

    // Cancel button (only when turn is active)
    let cancel_btn = if has_active_turn {
        let node = layout.new_leaf(Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(send_btn_x + BUTTON_WIDTH + PAD),
                top: length(8.0),
                right: auto(),
                bottom: auto(),
            },
            size: Size {
                width: length(BUTTON_WIDTH),
                height: length(34.0),
            },
            ..Default::default()
        });
        layout.add_child(prompt_bar, node);
        Some(node)
    } else {
        None
    };

    // ── 4. Enrichment inspector (collapsible) ──
    let inspector_node = if state.conversation.enrichment_inspector_visible
        && state.conversation.enrichment_inspector.is_some()
    {
        let inspector_y = win_h - PROMPT_BAR_HEIGHT - inspector_h;
        let node = layout.new_leaf(Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(0.0),
                top: length(inspector_y),
                right: auto(),
                bottom: auto(),
            },
            size: Size {
                width: length(win_w),
                height: length(inspector_h),
            },
            ..Default::default()
        });
        layout.add_child(panel_node, node);
        Some(node)
    } else {
        None
    };

    // ── Compute the panel sub-tree ──
    layout.compute(
        panel_node,
        (Some(win_w), Some(win_h)),
        |_, _, _, _, _| akar_layout::Size::ZERO,
    );

    // ── Render pass ──

    // Panel background
    let panel_rect = layout.rect(panel_node);
    core.draw_list.push_quad(akar_core::QuadCall {
        rect: panel_rect,
        fill: color_to_f32(theme.base_100),
        border_color: [0.0; 4],
        corner_radii: [0.0; 4],
        border_width: 0.0,
        z: 0.0,
        shadow_blur: 0.0,
        shadow_spread: 0.0,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        _pad: [0.0; 2],
    });

    // Header background
    let mut header_style = BoxStyle::panel(&theme);
    header_style.fill = theme.base_200;
    container(core, layout, header_node, &header_style);

    // Status badge
    let (badge_text, badge_variant) = if state.conversation.adapter_status.connected {
        ("Connected", BadgeVariant::Success)
    } else {
        ("Disconnected", BadgeVariant::Error)
    };
    badge(core, &*layout, status_badge, badge_text, badge_variant, &theme);

    // Adapter name label
    let adapter_text = if state.conversation.adapter_status.name.is_empty() {
        "No adapter".to_string()
    } else {
        format!(
            "{} v{}",
            state.conversation.adapter_status.name, state.conversation.adapter_status.version
        )
    };
    label(
        core,
        &*layout,
        adapter_label,
        &adapter_text,
        theme.neutral_content,
        &theme,
    );

    // Close button
    let close_resp = button(
        core,
        &*layout,
        close_btn,
        "\u{00d7}",
        ButtonVariant::Ghost,
        &theme,
    );
    if close_resp.clicked {
        state.conversation.visible = false;
    }

    // Inspector toggle button
    let inspector_label = if state.conversation.enrichment_inspector_visible {
        "Hide Inspector"
    } else {
        "Show Inspector"
    };
    let inspector_resp = button(
        core,
        &*layout,
        inspector_btn,
        inspector_label,
        ButtonVariant::Ghost,
        &theme,
    );
    if inspector_resp.clicked {
        state.conversation.enrichment_inspector_visible = !state.conversation.enrichment_inspector_visible;
    }

    // ── Transcript area ──
    let transcript_rect = layout.rect(transcript_node);
    let entry_count = state.conversation.entries.len();
    let content_height = (entry_count as f32 * (ENTRY_LINE_HEIGHT + ENTRY_GAP)) + PAD;

    let scroll_resp =
        scroll_area_begin(core, transcript_rect, &mut state.conversation.transcript_scroll_y, content_height);

    // Render transcript entries as labels positioned inside the scroll area.
    // We use manual positioning within the scissored rect rather than taffy
    // children, because the scroll offset shifts content each frame.
    for (i, entry) in state.conversation.entries.iter().enumerate() {
        let entry_y = scroll_resp.content_y + PAD + i as f32 * (ENTRY_LINE_HEIGHT + ENTRY_GAP);
        let entry_rect = [
            transcript_rect[0] + PAD,
            entry_y,
            transcript_rect[2] - PAD * 2.0,
            ENTRY_LINE_HEIGHT,
        ];

        // Skip entries that are entirely outside the visible rect.
        if entry_rect[1] + entry_rect[3] < transcript_rect[1]
            || entry_rect[1] > transcript_rect[1] + transcript_rect[3]
        {
            continue;
        }

        let (prefix, color) = match &entry.kind {
            TranscriptEntryKind::UserPrompt => ("You: ", theme.primary),
            TranscriptEntryKind::AgentText => ("Agent: ", theme.base_content),
            TranscriptEntryKind::ToolCall => ("Tool: ", theme.neutral_content),
            TranscriptEntryKind::ToolCallUpdate => ("Tool update: ", theme.neutral_content),
            TranscriptEntryKind::Thought => ("Thought: ", theme.neutral),
            TranscriptEntryKind::Plan => ("Plan: ", theme.info),
            TranscriptEntryKind::SystemMessage => ("System: ", theme.neutral),
            TranscriptEntryKind::Error => ("Error: ", theme.error),
            TranscriptEntryKind::Unknown(kind) => (kind.as_str(), theme.warning),
        };
        let display_text = format!("{prefix}{}", entry.text);

        // Render via a label node placed at the scroll-adjusted position.
        let label_node = layout.new_leaf(Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(entry_rect[0]),
                top: length(entry_rect[1]),
                right: auto(),
                bottom: auto(),
            },
            size: Size {
                width: length(entry_rect[2]),
                height: length(entry_rect[3]),
            },
            ..Default::default()
        });
        // We need to compute this node in the existing layout context.
        // Since it's absolute-positioned and rootless-attached to the panel,
        // we can compute it directly.
        layout.compute(
            label_node,
            (Some(entry_rect[2]), Some(entry_rect[3])),
            |_, _, _, _, _| akar_layout::Size::ZERO,
        );
        label(core, &*layout, label_node, &display_text, color, &theme);
    }

    scroll_area_end(core);

    // ── Prompt bar ──
    let prompt_bar_rect = layout.rect(prompt_bar);
    core.draw_list.push_quad(akar_core::QuadCall {
        rect: prompt_bar_rect,
        fill: color_to_f32(theme.base_200),
        border_color: [0.0; 4],
        corner_radii: [0.0; 4],
        border_width: 0.0,
        z: 0.0,
        shadow_blur: 0.0,
        shadow_spread: 0.0,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        _pad: [0.0; 2],
    });

    // Prompt input
    let prompt_widget_id = layout.widget_id(prompt_input_node);
    if !has_active_turn {
        core.input.focused_id = Some(prompt_widget_id);
    }
    state.conversation.prompt_edit_state.cursor = state
        .conversation
        .prompt_edit_state
        .cursor
        .min(state.conversation.prompt_input.len());
    state.conversation.prompt_edit_state.anchor = state
        .conversation
        .prompt_edit_state
        .anchor
        .min(state.conversation.prompt_input.len());

    let input_resp = text_input(
        core,
        &*layout,
        prompt_input_node,
        &mut state.conversation.prompt_input,
        &mut state.conversation.prompt_edit_state,
        "Type a prompt... (Enter to send)",
        true,
        &theme,
    );

    // Send button
    let send_label = if has_active_turn { "..." } else { "Send" };
    let send_resp = button(core, &*layout, send_btn, send_label, ButtonVariant::Solid, &theme);
    let should_send = (input_resp.submitted && !state.conversation.prompt_input.trim().is_empty())
        || (send_resp.clicked && !has_active_turn && !state.conversation.prompt_input.trim().is_empty());

    if should_send {
        // The actual send is handled in main.rs which has access to the
        // coordinator channel. We signal by leaving prompt_input non-empty;
        // main.rs will drain it after render_conversation returns.
        state.conversation.prompt_send_requested = true;
    }

    // Cancel button
    if let Some(cancel_node) = cancel_btn {
        let cancel_resp = button(
            core,
            &*layout,
            cancel_node,
            "Cancel",
            ButtonVariant::Outline,
            &theme,
        );
        if cancel_resp.clicked {
            state.conversation.cancel_requested = true;
        }
    }

    // ── Enrichment inspector ──
    if let Some(inspector_node) = inspector_node {
        let inspector_rect = layout.rect(inspector_node);
        core.draw_list.push_quad(akar_core::QuadCall {
            rect: inspector_rect,
            fill: color_to_f32(theme.base_200),
            border_color: color_to_f32(theme.base_300),
            corner_radii: [0.0; 4],
            border_width: 1.0,
            z: 0.0,
            shadow_blur: 0.0,
            shadow_spread: 0.0,
            shadow_color: [0.0; 4],
            shadow_offset: [0.0; 2],
            _pad: [0.0; 2],
        });

        let mut copy_requested = None;
        if let Some(inspector) = &state.conversation.enrichment_inspector {
            let ix = inspector_rect[0] + PAD;
            let iy = inspector_rect[1] + PAD;
            let iw = inspector_rect[2] - PAD * 2.0;
            let preview_width = (iw - COPY_BUTTON_WIDTH - 8.0).max(0.0);

            let mut y_offset = 0.0;

            // Original prompt label
            let orig_node = layout.new_leaf(Style {
                position: Position::Absolute,
                inset: Rect {
                    left: length(ix),
                    top: length(iy + y_offset),
                    right: auto(),
                    bottom: auto(),
                },
                size: Size {
                    width: length(preview_width),
                    height: length(ENTRY_LINE_HEIGHT),
                },
                ..Default::default()
            });
            layout.compute(
                orig_node,
                (Some(preview_width), Some(ENTRY_LINE_HEIGHT)),
                |_, _, _, _, _| akar_layout::Size::ZERO,
            );
            let orig_text = format!("Original: {}", truncate_str(&inspector.original_prompt, 80));
            label(
                core,
                &*layout,
                orig_node,
                &orig_text,
                theme.neutral_content,
                &theme,
            );

            let orig_copy_node = layout.new_leaf(Style {
                position: Position::Absolute,
                inset: Rect {
                    left: length(ix + iw - COPY_BUTTON_WIDTH),
                    top: length(iy + y_offset - 4.0),
                    right: auto(),
                    bottom: auto(),
                },
                size: Size {
                    width: length(COPY_BUTTON_WIDTH),
                    height: length(26.0),
                },
                ..Default::default()
            });
            layout.compute(
                orig_copy_node,
                (Some(COPY_BUTTON_WIDTH), Some(26.0)),
                |_, _, _, _, _| akar_layout::Size::ZERO,
            );
            let orig_copy_label = if matches!(
                state.conversation.clipboard_feedback,
                Some(state::ClipboardFeedback::Copied(
                    state::InspectorPromptKind::Original
                ))
            ) {
                "Copied!"
            } else {
                "Copy original"
            };
            if button(
                core,
                &*layout,
                orig_copy_node,
                orig_copy_label,
                ButtonVariant::Ghost,
                &theme,
            )
            .clicked
            {
                copy_requested = Some(state::InspectorPromptKind::Original);
            }
            y_offset += ENTRY_LINE_HEIGHT + 4.0;

            // Enriched prompt label
            let enriched_node = layout.new_leaf(Style {
                position: Position::Absolute,
                inset: Rect {
                    left: length(ix),
                    top: length(iy + y_offset),
                    right: auto(),
                    bottom: auto(),
                },
                size: Size {
                    width: length(preview_width),
                    height: length(ENTRY_LINE_HEIGHT),
                },
                ..Default::default()
            });
            layout.compute(
                enriched_node,
                (Some(preview_width), Some(ENTRY_LINE_HEIGHT)),
                |_, _, _, _, _| akar_layout::Size::ZERO,
            );
            let enriched_text = format!("Enriched: {}", truncate_str(&inspector.enriched_prompt, 80));
            label(
                core,
                &*layout,
                enriched_node,
                &enriched_text,
                theme.base_content,
                &theme,
            );

            let enriched_copy_node = layout.new_leaf(Style {
                position: Position::Absolute,
                inset: Rect {
                    left: length(ix + iw - COPY_BUTTON_WIDTH),
                    top: length(iy + y_offset - 4.0),
                    right: auto(),
                    bottom: auto(),
                },
                size: Size {
                    width: length(COPY_BUTTON_WIDTH),
                    height: length(26.0),
                },
                ..Default::default()
            });
            layout.compute(
                enriched_copy_node,
                (Some(COPY_BUTTON_WIDTH), Some(26.0)),
                |_, _, _, _, _| akar_layout::Size::ZERO,
            );
            let enriched_copy_label = if matches!(
                state.conversation.clipboard_feedback,
                Some(state::ClipboardFeedback::Copied(
                    state::InspectorPromptKind::Enriched
                ))
            ) {
                "Copied!"
            } else {
                "Copy enriched"
            };
            if button(
                core,
                &*layout,
                enriched_copy_node,
                enriched_copy_label,
                ButtonVariant::Ghost,
                &theme,
            )
            .clicked
            {
                copy_requested = Some(state::InspectorPromptKind::Enriched);
            }
            y_offset += ENTRY_LINE_HEIGHT + 4.0;

            // Retrieval status
            let status_node = layout.new_leaf(Style {
                position: Position::Absolute,
                inset: Rect {
                    left: length(ix),
                    top: length(iy + y_offset),
                    right: auto(),
                    bottom: auto(),
                },
                size: Size {
                    width: length(iw),
                    height: length(ENTRY_LINE_HEIGHT),
                },
                ..Default::default()
            });
            layout.compute(
                status_node,
                (Some(iw), Some(ENTRY_LINE_HEIGHT)),
                |_, _, _, _, _| akar_layout::Size::ZERO,
            );
            let status_text = format!(
                "Status: {} | {} included, {} excluded | budget: {} chars",
                inspector.retrieval_status,
                inspector.included_excerpts.len(),
                inspector.excluded_candidates.len(),
                inspector.total_char_budget,
            );
            label(
                core,
                &*layout,
                status_node,
                &status_text,
                theme.neutral,
                &theme,
            );
            y_offset += ENTRY_LINE_HEIGHT + 4.0;

            if let Some(feedback) = &state.conversation.clipboard_feedback {
                let (feedback_text, feedback_color) = match feedback {
                    state::ClipboardFeedback::Copied(target) => (
                        format!("Copied the {} prompt to the clipboard.", target.label()),
                        theme.success,
                    ),
                    state::ClipboardFeedback::Failed { target, message } => (
                        format!("Could not copy the {} prompt: {message}", target.label()),
                        theme.error,
                    ),
                };
                let feedback_node = layout.new_leaf(Style {
                    position: Position::Absolute,
                    inset: Rect {
                        left: length(ix),
                        top: length(iy + y_offset),
                        right: auto(),
                        bottom: auto(),
                    },
                    size: Size {
                        width: length(iw),
                        height: length(ENTRY_LINE_HEIGHT),
                    },
                    ..Default::default()
                });
                layout.compute(
                    feedback_node,
                    (Some(iw), Some(ENTRY_LINE_HEIGHT)),
                    |_, _, _, _, _| akar_layout::Size::ZERO,
                );
                label(
                    core,
                    &*layout,
                    feedback_node,
                    &truncate_str(&feedback_text, 140),
                    feedback_color,
                    &theme,
                );
                y_offset += ENTRY_LINE_HEIGHT + 4.0;
            }

            // Included excerpts summary
            for excerpt in inspector.included_excerpts.iter().take(3) {
                if iy + y_offset > inspector_rect[1] + inspector_rect[3] - ENTRY_LINE_HEIGHT {
                    break;
                }
                let excerpt_node = layout.new_leaf(Style {
                    position: Position::Absolute,
                    inset: Rect {
                        left: length(ix + 12.0),
                        top: length(iy + y_offset),
                        right: auto(),
                        bottom: auto(),
                    },
                    size: Size {
                        width: length(iw - 12.0),
                        height: length(ENTRY_LINE_HEIGHT),
                    },
                    ..Default::default()
                });
                layout.compute(
                    excerpt_node,
                    (Some(iw - 12.0), Some(ENTRY_LINE_HEIGHT)),
                    |_, _, _, _, _| akar_layout::Size::ZERO,
                );
                let trunc = if excerpt.truncated { " [truncated]" } else { "" };
                let excerpt_text = format!(
                    "#{} [{}] {}{}",
                    excerpt.rank, excerpt.source, excerpt.identifier, trunc,
                );
                label(
                    core,
                    &*layout,
                    excerpt_node,
                    &excerpt_text,
                    theme.info,
                    &theme,
                );
                y_offset += ENTRY_LINE_HEIGHT + 2.0;
            }

            // Excluded/truncated candidates summary (Task 5 acceptance:
            // "Retrieval omissions and truncation are visible, not silently
            // discarded").
            for excluded in inspector.excluded_candidates.iter().take(3) {
                if iy + y_offset > inspector_rect[1] + inspector_rect[3] - ENTRY_LINE_HEIGHT {
                    break;
                }
                let excluded_node = layout.new_leaf(Style {
                    position: Position::Absolute,
                    inset: Rect {
                        left: length(ix + 12.0),
                        top: length(iy + y_offset),
                        right: auto(),
                        bottom: auto(),
                    },
                    size: Size {
                        width: length(iw - 12.0),
                        height: length(ENTRY_LINE_HEIGHT),
                    },
                    ..Default::default()
                });
                layout.compute(
                    excluded_node,
                    (Some(iw - 12.0), Some(ENTRY_LINE_HEIGHT)),
                    |_, _, _, _, _| akar_layout::Size::ZERO,
                );
                let excluded_text = format!(
                    "excluded #{} [{}] {} ({})",
                    excluded.rank, excluded.source, excluded.identifier, excluded.reason,
                );
                label(
                    core,
                    &*layout,
                    excluded_node,
                    &excluded_text,
                    theme.warning,
                    &theme,
                );
                y_offset += ENTRY_LINE_HEIGHT + 2.0;
            }
        }
        if let Some(target) = copy_requested {
            state.conversation.request_prompt_copy(target);
        }
    }

    // ── Permission dialog modal ──
    if state.conversation.permission_dialog.is_some() {
        render_permission_dialog(core, layout, state);
    }
}

/// Renders the permission dialog as a modal overlay.
fn render_permission_dialog(core: &mut AkarCore, layout: &mut Layout, state: &mut AppState) {
    let theme = match state.system_theme {
        state::SystemTheme::Dark => akar_components::AKAR_THEME_DARK,
        state::SystemTheme::Light => akar_components::AKAR_THEME_LIGHT,
    };

    let viewport_rect = [0.0, 0.0, state.window_size.x, state.window_size.y];
    let dialog_w = 450.0_f32.min(state.window_size.x - 40.0);
    let dialog_h = 250.0_f32.min(state.window_size.y - 40.0);

    let modal_resp = modal_begin(
        core,
        layout,
        viewport_rect,
        "Permission Required",
        dialog_w,
        dialog_h,
        &theme,
    );

    if modal_resp.close_requested {
        // Treat close as cancel
        state.conversation.permission_cancel_requested = true;
        modal_end(core);
        return;
    }

    let content_rect = modal_resp.content_rect;
    let cx = content_rect[0] + PAD;
    let mut cy = content_rect[1] + PAD;
    let cw = content_rect[2] - PAD * 2.0;

    let dialog = state.conversation.permission_dialog.as_ref().unwrap();

    // Tool call description
    let desc_node = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(cx),
            top: length(cy),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(cw),
            height: length(ENTRY_LINE_HEIGHT * 2.0),
        },
        ..Default::default()
    });
    layout.compute(
        desc_node,
        (Some(cw), Some(ENTRY_LINE_HEIGHT * 2.0)),
        |_, _, _, _, _| akar_layout::Size::ZERO,
    );
    label(
        core,
        &*layout,
        desc_node,
        &dialog.tool_call_description,
        theme.base_content,
        &theme,
    );
    cy += ENTRY_LINE_HEIGHT * 2.0 + PAD;

    // Option buttons
    let mut option_clicked: Option<String> = None;
    for option in &dialog.options {
        let opt_node = layout.new_leaf(Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(cx),
                top: length(cy),
                right: auto(),
                bottom: auto(),
            },
            size: Size {
                width: length(cw.min(300.0)),
                height: length(32.0),
            },
            ..Default::default()
        });
        layout.compute(
            opt_node,
            (Some(cw.min(300.0)), Some(32.0)),
            |_, _, _, _, _| akar_layout::Size::ZERO,
        );
        let variant = if option.kind == "allow_always" || option.kind == "allow_once" {
            ButtonVariant::Solid
        } else {
            ButtonVariant::Outline
        };
        let opt_resp = button(core, &*layout, opt_node, &option.label, variant, &theme);
        if opt_resp.clicked {
            option_clicked = Some(option.id.clone());
        }
        cy += 36.0;
    }

    // Cancel button at the bottom
    let cancel_node = layout.new_leaf(Style {
        position: Position::Absolute,
        inset: Rect {
            left: length(cx),
            top: length(cy + 4.0),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(cw.min(300.0)),
            height: length(32.0),
        },
        ..Default::default()
    });
    layout.compute(
        cancel_node,
        (Some(cw.min(300.0)), Some(32.0)),
        |_, _, _, _, _| akar_layout::Size::ZERO,
    );
    let cancel_resp = button(
        core,
        &*layout,
        cancel_node,
        "Cancel",
        ButtonVariant::Ghost,
        &theme,
    );

    if cancel_resp.clicked {
        state.conversation.permission_cancel_requested = true;
    } else if let Some(option_id) = option_clicked {
        state.conversation.permission_response = Some(option_id);
    }

    modal_end(core);
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let visible: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{visible}...")
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_str;

    #[test]
    fn prompt_preview_truncates_unicode_at_character_boundaries() {
        assert_eq!(truncate_str("λ漢字abcdef", 6), "λ漢字...");
    }

    #[test]
    fn prompt_preview_does_not_modify_short_text() {
        assert_eq!(truncate_str("  exact\n", 20), "  exact\n");
    }
}
