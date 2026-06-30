use crate::state::AppState;
use crate::input::InputHandler;
use crate::renderer::TextAreaData;
use crate::ui::create_text_buffer;
use glam::Vec2;
use glyphon::{Color, FontSystem, TextBounds};

pub struct SearchBox {
    pub position: Vec2,
    pub size: Vec2,
    pub is_focused: bool,
    pub cursor_visible: bool,
    pub cursor_timer: f32,
}

impl SearchBox {
    pub fn new() -> Self {
        Self {
            position: Vec2::new(0.0, 0.0),
            size: Vec2::new(500.0, 50.0),
            is_focused: false,
            cursor_visible: true,
            cursor_timer: 0.0,
        }
    }

    pub fn update(&mut self, state: &mut AppState, _input: &InputHandler) {
        self.position = Vec2::new(
            (state.window_size.x - self.size.x) / 2.0,
            state.window_size.y - self.size.y - 20.0,
        );

        self.is_focused = state.search_active;

        self.cursor_timer += 1.0;
        if self.cursor_timer >= 30.0 {
            self.cursor_visible = !self.cursor_visible;
            self.cursor_timer = 0.0;
        }

        if !state.search_query.is_empty() {
            self.cursor_visible = true;
        }
    }

    pub fn render(
        &self,
        state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        let bg_color = if self.is_focused {
            Color::rgba(50, 50, 50, 240)
        } else {
            Color::rgba(40, 40, 40, 220)
        };

        let bg_text = " ".repeat((self.size.x * self.size.y / 100.0) as usize);
        let bg_buffer = create_text_buffer(
            font_system,
            &bg_text,
            10.0,
            12.0,
            Some(self.size.x),
            Some(self.size.y),
        );

        text_areas.push(TextAreaData {
            buffer: bg_buffer,
            left: self.position.x,
            top: self.position.y,
            scale: 1.0,
            bounds: TextBounds {
                left: self.position.x as i32,
                top: self.position.y as i32,
                right: (self.position.x + self.size.x) as i32,
                bottom: (self.position.y + self.size.y) as i32,
            },
            color: bg_color,
        });

        let border_color = if self.is_focused {
            Color::rgba(0, 122, 255, 200)
        } else {
            Color::rgba(80, 80, 80, 150)
        };

        let border_text = " ".repeat(self.size.x as usize);
        let border_buffer = create_text_buffer(
            font_system,
            &border_text,
            2.0,
            2.0,
            Some(self.size.x),
            Some(2.0),
        );

        text_areas.push(TextAreaData {
            buffer: border_buffer,
            left: self.position.x,
            top: self.position.y,
            scale: 1.0,
            bounds: TextBounds {
                left: self.position.x as i32,
                top: self.position.y as i32,
                right: (self.position.x + self.size.x) as i32,
                bottom: (self.position.y + 2.0) as i32,
            },
            color: border_color,
        });

        let icon_buffer = create_text_buffer(
            font_system,
            "🔍",
            16.0,
            20.0,
            Some(20.0),
            Some(self.size.y),
        );

        text_areas.push(TextAreaData {
            buffer: icon_buffer,
            left: self.position.x + 15.0,
            top: self.position.y + (self.size.y - 20.0) / 2.0,
            scale: 1.0,
            bounds: TextBounds {
                left: self.position.x as i32,
                top: self.position.y as i32,
                right: (self.position.x + self.size.x) as i32,
                bottom: (self.position.y + self.size.y) as i32,
            },
            color: Color::rgb(150, 150, 150),
        });

        let text_x = self.position.x + 45.0;
        let text_width = self.size.x - 90.0;

        if state.search_query.is_empty() {
            let placeholder_text = "Search documents... (⌘K)";
            let placeholder_buffer = create_text_buffer(
                font_system,
                placeholder_text,
                14.0,
                18.0,
                Some(text_width),
                Some(self.size.y),
            );

            text_areas.push(TextAreaData {
                buffer: placeholder_buffer,
                left: text_x,
                top: self.position.y + (self.size.y - 18.0) / 2.0,
                scale: 1.0,
                bounds: TextBounds {
                    left: text_x as i32,
                    top: self.position.y as i32,
                    right: (text_x + text_width) as i32,
                    bottom: (self.position.y + self.size.y) as i32,
                },
                color: Color::rgba(120, 120, 120, 200),
            });
        } else {
            let query_buffer = create_text_buffer(
                font_system,
                &state.search_query,
                14.0,
                18.0,
                Some(text_width),
                Some(self.size.y),
            );

            text_areas.push(TextAreaData {
                buffer: query_buffer,
                left: text_x,
                top: self.position.y + (self.size.y - 18.0) / 2.0,
                scale: 1.0,
                bounds: TextBounds {
                    left: text_x as i32,
                    top: self.position.y as i32,
                    right: (text_x + text_width) as i32,
                    bottom: (self.position.y + self.size.y) as i32,
                },
                color: Color::rgb(240, 240, 240),
            });

            if self.is_focused && self.cursor_visible {
                let cursor_text = "│";
                let cursor_buffer = create_text_buffer(
                    font_system,
                    cursor_text,
                    14.0,
                    18.0,
                    Some(10.0),
                    Some(self.size.y),
                );

                let char_width = 8.0;
                let cursor_x = text_x + state.search_query.len() as f32 * char_width;

                text_areas.push(TextAreaData {
                    buffer: cursor_buffer,
                    left: cursor_x,
                    top: self.position.y + (self.size.y - 18.0) / 2.0,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: cursor_x as i32,
                        top: self.position.y as i32,
                        right: (cursor_x + 10.0) as i32,
                        bottom: (self.position.y + self.size.y) as i32,
                    },
                    color: Color::rgb(0, 122, 255),
                });
            }

            let clear_x = self.position.x + self.size.x - 40.0;
            let clear_buffer = create_text_buffer(
                font_system,
                "✕",
                16.0,
                20.0,
                Some(20.0),
                Some(self.size.y),
            );

            text_areas.push(TextAreaData {
                buffer: clear_buffer,
                left: clear_x,
                top: self.position.y + (self.size.y - 20.0) / 2.0,
                scale: 1.0,
                bounds: TextBounds {
                    left: clear_x as i32,
                    top: self.position.y as i32,
                    right: (clear_x + 20.0) as i32,
                    bottom: (self.position.y + self.size.y) as i32,
                },
                color: Color::rgb(150, 150, 150),
            });
        }

        if !state.search_query.is_empty() {
            let matching_count = self.count_matching_cards(state);
            let result_text = format!("{} results", matching_count);
            let result_buffer = create_text_buffer(
                font_system,
                &result_text,
                12.0,
                16.0,
                Some(80.0),
                Some(16.0),
            );

            text_areas.push(TextAreaData {
                buffer: result_buffer,
                left: self.position.x + self.size.x + 10.0,
                top: self.position.y + (self.size.y - 16.0) / 2.0,
                scale: 1.0,
                bounds: TextBounds {
                    left: (self.position.x + self.size.x + 10.0) as i32,
                    top: self.position.y as i32,
                    right: (self.position.x + self.size.x + 90.0) as i32,
                    bottom: (self.position.y + self.size.y) as i32,
                },
                color: Color::rgba(150, 150, 150, 200),
            });
        }
    }

    fn count_matching_cards(&self, state: &AppState) -> usize {
        if state.search_query.is_empty() {
            return state.containers.iter().map(|c| c.cards.len()).sum();
        }

        let query = state.search_query.to_lowercase();
        state.containers.iter().flat_map(|c| c.documents.iter()).filter(|doc| {
            doc.title.to_lowercase().contains(&query) ||
            doc.content.to_lowercase().contains(&query)
        }).count()
    }
}
