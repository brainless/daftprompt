pub mod canvas;
pub mod container;
pub mod drawer;
pub mod search;

use crate::state::AppState;
use crate::input::InputHandler;
use crate::renderer::TextAreaData;
use glyphon::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, TextBounds};

pub struct UIManager {
    pub canvas: canvas::Canvas,
    pub drawer: drawer::Drawer,
    pub search: search::SearchBox,
}

impl UIManager {
    pub fn new() -> Self {
        Self {
            canvas: canvas::Canvas::new(),
            drawer: drawer::Drawer::new(),
            search: search::SearchBox::new(),
        }
    }

    pub fn update(&mut self, state: &mut AppState, input: &InputHandler) {
        self.drawer.update(state, input);
        self.search.update(state, input);
        self.canvas.update(state, input);

        // Update card hover states for all containers
        let zoom = state.zoom;
        let pan_offset = state.pan_offset;
        let window_size = state.window_size;

        for container in &mut state.containers {
            for i in 0..container.cards.len() {
                let card = &container.cards[i];
                let card_abs_y = container.position.y + card.position.y - container.scroll_offset;
                let abs_pos = glam::Vec2::new(container.position.x + card.position.x, card_abs_y);
                let scaled_size = card.size * zoom;
                let screen_pos = (abs_pos - pan_offset) * zoom + window_size * 0.5;

                container.cards[i].is_hovered = input.is_mouse_over_rect(screen_pos, scaled_size);
            }
        }
    }

    pub fn render<'a>(&self, state: &AppState, text_areas: &mut Vec<TextAreaData>, font_system: &mut FontSystem) {
        // Render canvas background
        self.canvas.render(state, text_areas, font_system);

        // Render containers
        for container in &state.containers {
            self.render_container(container, state, text_areas, font_system);
        }

        // Render drawer
        self.drawer.render(state, text_areas, font_system);

        // Render search box
        self.search.render(state, text_areas, font_system);
    }

    fn render_container(
        &self,
        container: &crate::ui::container::Container,
        state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        // Container background
        let screen_pos = state.canvas_to_screen(container.position);
        let scaled_size = container.size * state.zoom;

        if !self.is_visible(screen_pos, scaled_size, state.window_size) {
            return;
        }

        let bg_color = match container.container_type {
            crate::ui::container::ContainerType::GitLogColumn => {
                Color::rgba(25, 25, 30, 230)
            }
            crate::ui::container::ContainerType::DocumentGrid => {
                Color::rgba(30, 30, 35, 200)
            }
        };

        let bg_text = " ".repeat((scaled_size.x * scaled_size.y / 100.0) as usize);
        let bg_buffer = create_text_buffer(
            font_system,
            &bg_text,
            10.0,
            12.0,
            Some(scaled_size.x),
            Some(scaled_size.y),
        );

        text_areas.push(TextAreaData {
            buffer: bg_buffer,
            left: screen_pos.x,
            top: screen_pos.y,
            scale: 1.0,
            bounds: TextBounds {
                left: screen_pos.x as i32,
                top: screen_pos.y as i32,
                right: (screen_pos.x + scaled_size.x) as i32,
                bottom: (screen_pos.y + scaled_size.y) as i32,
            },
            color: bg_color,
        });

        // Container border
        let border_color = Color::rgba(60, 60, 65, 200);
        let border_text = " ".repeat(scaled_size.x as usize);

        // Top border
        let border_buffer = create_text_buffer(
            font_system,
            &border_text,
            2.0,
            2.0,
            Some(scaled_size.x),
            Some(2.0),
        );
        text_areas.push(TextAreaData {
            buffer: border_buffer,
            left: screen_pos.x,
            top: screen_pos.y,
            scale: 1.0,
            bounds: TextBounds {
                left: screen_pos.x as i32,
                top: screen_pos.y as i32,
                right: (screen_pos.x + scaled_size.x) as i32,
                bottom: (screen_pos.y + 2.0) as i32,
            },
            color: border_color,
        });

        // Render visible cards
        for (card, doc, abs_pos) in container.visible_cards() {
            let scaled_card_pos = state.canvas_to_screen(abs_pos);
            let scaled_card_size = card.size * state.zoom;

            // Clip to container bounds
            let clip_top = screen_pos.y.max(scaled_card_pos.y);
            let clip_bottom = (screen_pos.y + scaled_size.y).min(scaled_card_pos.y + scaled_card_size.y);

            if clip_top >= clip_bottom {
                continue;
            }

            self.render_card(
                card,
                doc,
                scaled_card_pos,
                scaled_card_size,
                clip_top,
                clip_bottom,
                container.container_type,
                state,
                text_areas,
                font_system,
            );
        }

        // Scroll indicator for GitLogColumn
        if container.container_type == crate::ui::container::ContainerType::GitLogColumn
            && container.content_height > container.size.y
        {
            let scroll_ratio = container.scroll_offset / (container.content_height - container.size.y);
            let indicator_height = (container.size.y * container.size.y / container.content_height).max(20.0);
            let indicator_y = container.position.y + scroll_ratio * (container.size.y - indicator_height);

            let indicator_screen_pos = state.canvas_to_screen(glam::Vec2::new(
                container.position.x + container.size.x - 6.0,
                indicator_y,
            ));
            let indicator_size = glam::Vec2::new(4.0, indicator_height) * state.zoom;

            let indicator_text = " ".repeat((indicator_size.x * indicator_size.y / 50.0) as usize);
            let indicator_buffer = create_text_buffer(
                font_system,
                &indicator_text,
                2.0,
                2.0,
                Some(indicator_size.x),
                Some(indicator_size.y),
            );

            text_areas.push(TextAreaData {
                buffer: indicator_buffer,
                left: indicator_screen_pos.x,
                top: indicator_screen_pos.y,
                scale: 1.0,
                bounds: TextBounds {
                    left: indicator_screen_pos.x as i32,
                    top: indicator_screen_pos.y as i32,
                    right: (indicator_screen_pos.x + indicator_size.x) as i32,
                    bottom: (indicator_screen_pos.y + indicator_size.y) as i32,
                },
                color: Color::rgba(100, 100, 110, 180),
            });
        }
    }

    fn render_card(
        &self,
        card: &crate::state::CardData,
        doc: &crate::state::DocumentData,
        screen_pos: glam::Vec2,
        scaled_size: glam::Vec2,
        clip_top: f32,
        clip_bottom: f32,
        container_type: crate::ui::container::ContainerType,
        state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        let clipped_height = clip_bottom - clip_top;
        let clipped_top_offset = clip_top - screen_pos.y;

        // Card background
        let bg_color = match container_type {
            crate::ui::container::ContainerType::GitLogColumn => {
                if card.is_hovered {
                    Color::rgba(50, 50, 58, 240)
                } else {
                    Color::rgba(38, 38, 44, 240)
                }
            }
            crate::ui::container::ContainerType::DocumentGrid => {
                if card.is_selected {
                    Color::rgba(0, 122, 255, 60)
                } else if card.is_hovered {
                    Color::rgba(60, 60, 60, 200)
                } else {
                    Color::rgba(45, 45, 45, 220)
                }
            }
        };

        let bg_text = " ".repeat((scaled_size.x * clipped_height / 100.0) as usize);
        let bg_buffer = create_text_buffer(
            font_system,
            &bg_text,
            10.0,
            12.0,
            Some(scaled_size.x),
            Some(clipped_height),
        );

        text_areas.push(TextAreaData {
            buffer: bg_buffer,
            left: screen_pos.x,
            top: clip_top,
            scale: 1.0,
            bounds: TextBounds {
                left: screen_pos.x as i32,
                top: clip_top as i32,
                right: (screen_pos.x + scaled_size.x) as i32,
                bottom: clip_bottom as i32,
            },
            color: bg_color,
        });

        match container_type {
            crate::ui::container::ContainerType::GitLogColumn => {
                self.render_git_card_content(
                    card, doc, screen_pos, scaled_size, clip_top, clip_bottom, clipped_top_offset,
                    state, text_areas, font_system,
                );
            }
            crate::ui::container::ContainerType::DocumentGrid => {
                self.render_doc_card_content(
                    card, doc, screen_pos, scaled_size, clip_top, clip_bottom, clipped_top_offset,
                    state, text_areas, font_system,
                );
            }
        }
    }

    fn render_git_card_content(
        &self,
        _card: &crate::state::CardData,
        doc: &crate::state::DocumentData,
        screen_pos: glam::Vec2,
        scaled_size: glam::Vec2,
        clip_top: f32,
        clip_bottom: f32,
        clipped_top_offset: f32,
        _state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        let zoom = _state.zoom;

        // Parse commit info from content
        let lines: Vec<&str> = doc.content.lines().collect();
        let hash = lines.first().unwrap_or(&"");
        let author_line = lines.get(1).unwrap_or(&"Author: ");
        let author = author_line.strip_prefix("Author: ").unwrap_or(author_line);
        let date_line = lines.get(2).unwrap_or(&"Date: ");
        let date = date_line.strip_prefix("Date: ").unwrap_or(date_line);

        let text_x = screen_pos.x + 10.0;
        let text_width = scaled_size.x - 20.0;

        // Header: hash (cyan)
        let header_y = screen_pos.y + 8.0 - clipped_top_offset;
        if header_y + 16.0 * zoom > clip_top && header_y < clip_bottom {
            let hash_buffer = create_text_buffer(
                font_system,
                hash,
                12.0 * zoom,
                16.0 * zoom,
                Some(80.0 * zoom),
                Some(16.0 * zoom),
            );
            text_areas.push(TextAreaData {
                buffer: hash_buffer,
                left: text_x,
                top: clip_top.max(header_y),
                scale: 1.0,
                bounds: TextBounds {
                    left: text_x as i32,
                    top: clip_top as i32,
                    right: (text_x + 80.0 * zoom) as i32,
                    bottom: clip_bottom as i32,
                },
                color: Color::rgb(100, 200, 255),
            });

            // Author
            let author_buffer = create_text_buffer(
                font_system,
                author,
                11.0 * zoom,
                16.0 * zoom,
                Some(120.0 * zoom),
                Some(16.0 * zoom),
            );
            text_areas.push(TextAreaData {
                buffer: author_buffer,
                left: text_x + 85.0 * zoom,
                top: clip_top.max(header_y),
                scale: 1.0,
                bounds: TextBounds {
                    left: (text_x + 85.0 * zoom) as i32,
                    top: clip_top as i32,
                    right: (text_x + 200.0 * zoom) as i32,
                    bottom: clip_bottom as i32,
                },
                color: Color::rgb(170, 170, 175),
            });

            // Date
            let date_buffer = create_text_buffer(
                font_system,
                date,
                10.0 * zoom,
                16.0 * zoom,
                Some(140.0 * zoom),
                Some(16.0 * zoom),
            );
            text_areas.push(TextAreaData {
                buffer: date_buffer,
                left: text_x + 205.0 * zoom,
                top: clip_top.max(header_y),
                scale: 1.0,
                bounds: TextBounds {
                    left: (text_x + 205.0 * zoom) as i32,
                    top: clip_top as i32,
                    right: (text_x + text_width) as i32,
                    bottom: clip_bottom as i32,
                },
                color: Color::rgb(130, 130, 135),
            });
        }

        // Separator
        let sep_y = screen_pos.y + 28.0 - clipped_top_offset;
        if sep_y + 2.0 > clip_top && sep_y < clip_bottom {
            let sep_text = "─".repeat((text_width / (7.0 * zoom)) as usize);
            let sep_buffer = create_text_buffer(
                font_system,
                &sep_text,
                10.0 * zoom,
                12.0 * zoom,
                Some(text_width),
                Some(2.0),
            );
            text_areas.push(TextAreaData {
                buffer: sep_buffer,
                left: text_x,
                top: clip_top.max(sep_y),
                scale: 1.0,
                bounds: TextBounds {
                    left: text_x as i32,
                    top: clip_top as i32,
                    right: (text_x + text_width) as i32,
                    bottom: clip_bottom as i32,
                },
                color: Color::rgba(70, 70, 78, 180),
            });
        }

        // Message title
        let msg_y = screen_pos.y + 36.0 - clipped_top_offset;
        let msg_height = scaled_size.y - 44.0;
        if msg_y + msg_height > clip_top && msg_y < clip_bottom {
            let msg_buffer = create_text_buffer(
                font_system,
                &doc.title,
                13.0 * zoom,
                18.0 * zoom,
                Some(text_width),
                Some(msg_height.max(0.0)),
            );
            text_areas.push(TextAreaData {
                buffer: msg_buffer,
                left: text_x,
                top: clip_top.max(msg_y),
                scale: 1.0,
                bounds: TextBounds {
                    left: text_x as i32,
                    top: clip_top as i32,
                    right: (text_x + text_width) as i32,
                    bottom: clip_bottom as i32,
                },
                color: Color::rgb(225, 225, 230),
            });
        }
    }

    fn render_doc_card_content(
        &self,
        _card: &crate::state::CardData,
        doc: &crate::state::DocumentData,
        screen_pos: glam::Vec2,
        scaled_size: glam::Vec2,
        clip_top: f32,
        clip_bottom: f32,
        clipped_top_offset: f32,
        state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        // File type icon and title bar
        let icon = self.get_file_icon(doc.file_type);
        let title_text = format!("{} {}", icon, doc.title);
        let title_y = screen_pos.y + 10.0 - clipped_top_offset;

        if title_y + 18.0 * state.zoom > clip_top && title_y < clip_bottom {
            let title_buffer = create_text_buffer(
                font_system,
                &title_text,
                14.0 * state.zoom,
                18.0 * state.zoom,
                Some(scaled_size.x - 20.0),
                Some(18.0 * state.zoom),
            );

            text_areas.push(TextAreaData {
                buffer: title_buffer,
                left: screen_pos.x + 10.0,
                top: clip_top.max(title_y),
                scale: 1.0,
                bounds: TextBounds {
                    left: screen_pos.x as i32,
                    top: clip_top as i32,
                    right: (screen_pos.x + scaled_size.x) as i32,
                    bottom: clip_bottom as i32,
                },
                color: Color::rgb(240, 240, 240),
            });
        }

        // Separator line
        let sep_y = screen_pos.y + 30.0 - clipped_top_offset;
        if sep_y + 14.0 * state.zoom > clip_top && sep_y < clip_bottom {
            let sep_text = "─".repeat((scaled_size.x / 8.0) as usize);
            let sep_buffer = create_text_buffer(
                font_system,
                &sep_text,
                12.0 * state.zoom,
                14.0 * state.zoom,
                Some(scaled_size.x - 20.0),
                Some(14.0 * state.zoom),
            );

            text_areas.push(TextAreaData {
                buffer: sep_buffer,
                left: screen_pos.x + 10.0,
                top: clip_top.max(sep_y),
                scale: 1.0,
                bounds: TextBounds {
                    left: screen_pos.x as i32,
                    top: clip_top as i32,
                    right: (screen_pos.x + scaled_size.x) as i32,
                    bottom: clip_bottom as i32,
                },
                color: Color::rgba(100, 100, 100, 150),
            });
        }

        // Content preview
        let preview_y = screen_pos.y + 50.0 - clipped_top_offset;
        let preview_height = (scaled_size.y - 60.0).max(0.0);
        if preview_y + preview_height > clip_top && preview_y < clip_bottom {
            let preview_text = self.truncate_content(&doc.content, _card.size);
            let preview_buffer = create_text_buffer(
                font_system,
                &preview_text,
                12.0 * state.zoom,
                16.0 * state.zoom,
                Some(scaled_size.x - 20.0),
                Some(preview_height),
            );

            text_areas.push(TextAreaData {
                buffer: preview_buffer,
                left: screen_pos.x + 10.0,
                top: clip_top.max(preview_y),
                scale: 1.0,
                bounds: TextBounds {
                    left: screen_pos.x as i32,
                    top: clip_top as i32,
                    right: (screen_pos.x + scaled_size.x) as i32,
                    bottom: clip_bottom as i32,
                },
                color: Color::rgb(180, 180, 180),
            });
        }

        // Metadata footer
        let meta_y = screen_pos.y + scaled_size.y - 20.0 - clipped_top_offset;
        if meta_y + 12.0 * state.zoom > clip_top && meta_y < clip_bottom {
            let line_count = doc.content.lines().count();
            let metadata_text = format!("{} lines", line_count);
            let metadata_buffer = create_text_buffer(
                font_system,
                &metadata_text,
                10.0 * state.zoom,
                12.0 * state.zoom,
                Some(scaled_size.x - 20.0),
                Some(12.0 * state.zoom),
            );

            text_areas.push(TextAreaData {
                buffer: metadata_buffer,
                left: screen_pos.x + 10.0,
                top: clip_top.max(meta_y),
                scale: 1.0,
                bounds: TextBounds {
                    left: screen_pos.x as i32,
                    top: clip_top as i32,
                    right: (screen_pos.x + scaled_size.x) as i32,
                    bottom: clip_bottom as i32,
                },
                color: Color::rgba(120, 120, 120, 200),
            });
        }
    }

    fn truncate_content(&self, content: &str, card_size: glam::Vec2) -> String {
        let max_chars = (card_size.x * card_size.y / 100.0) as usize;
        let max_lines = (card_size.y / 20.0) as usize;

        let lines: Vec<&str> = content.lines().collect();
        let truncated_lines: Vec<&str> = lines.iter().take(max_lines).copied().collect();

        let mut result = truncated_lines.join("\n");
        if result.len() > max_chars {
            result.truncate(max_chars - 3);
            result.push_str("...");
        } else if lines.len() > max_lines {
            result.push_str("\n...");
        }

        result
    }

    fn get_file_icon(&self, file_type: crate::state::IconType) -> &'static str {
        match file_type {
            crate::state::IconType::Folder => "📁",
            crate::state::IconType::GitRepo => "🔀",
            crate::state::IconType::Document => "📄",
            crate::state::IconType::Code => "💻",
            crate::state::IconType::Markdown => "📝",
            crate::state::IconType::Search => "🔍",
            crate::state::IconType::Settings => "⚙️",
        }
    }

    fn is_visible(&self, position: glam::Vec2, size: glam::Vec2, window_size: glam::Vec2) -> bool {
        position.x + size.x > 0.0
            && position.x < window_size.x
            && position.y + size.y > 0.0
            && position.y < window_size.y
    }
}

pub fn create_text_buffer(
    font_system: &mut FontSystem,
    text: &str,
    font_size: f32,
    line_height: f32,
    width: Option<f32>,
    height: Option<f32>,
) -> Buffer {
    let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));
    buffer.set_size(font_system, width, height);
    buffer.set_text(
        font_system,
        text,
        &Attrs::new().family(Family::SansSerif),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(font_system, false);
    buffer
}
