use crate::state::{AppState, CardData, DocumentData};
use crate::input::InputHandler;
use crate::renderer::TextAreaData;
use crate::ui::create_text_buffer;
use glam::Vec2;
use glyphon::{Color, FontSystem, TextBounds};

pub struct CardRenderer {
    min_size: Vec2,
    max_size: Vec2,
}

impl CardRenderer {
    pub fn new() -> Self {
        Self {
            min_size: Vec2::new(200.0, 150.0),
            max_size: Vec2::new(400.0, 300.0),
        }
    }
    
    pub fn update(&self, state: &mut AppState, input: &InputHandler) {
        // Update card hover states
        for i in 0..state.cards.len() {
            let card = &state.cards[i];
            let screen_pos = state.canvas_to_screen(card.position);
            let scaled_size = card.size * state.zoom;
            
            let is_hovered = input.is_mouse_over_rect(screen_pos, scaled_size);
            state.cards[i].is_hovered = is_hovered;
        }
    }
    
    pub fn render_card(
        &self,
        card: &CardData,
        doc: &DocumentData,
        state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        let screen_pos = state.canvas_to_screen(card.position);
        let scaled_size = card.size * state.zoom;
        
        // Card background
        let bg_color = if card.is_selected {
            Color::rgba(0, 122, 255, 60)
        } else if card.is_hovered {
            Color::rgba(60, 60, 60, 200)
        } else {
            Color::rgba(45, 45, 45, 220)
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
        
        // Card border
        let border_color = if card.is_selected {
            Color::rgba(0, 122, 255, 200)
        } else if card.is_hovered {
            Color::rgba(100, 100, 100, 200)
        } else {
            Color::rgba(70, 70, 70, 150)
        };
        
        // Top border
        let border_text = " ".repeat(scaled_size.x as usize);
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
        
        // File type icon and title bar
        let icon = self.get_file_icon(doc.file_type);
        let title_text = format!("{} {}", icon, doc.title);
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
            top: screen_pos.y + 10.0,
            scale: 1.0,
            bounds: TextBounds {
                left: screen_pos.x as i32,
                top: screen_pos.y as i32,
                right: (screen_pos.x + scaled_size.x) as i32,
                bottom: (screen_pos.y + scaled_size.y) as i32,
            },
            color: Color::rgb(240, 240, 240),
        });
        
        // Separator line
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
            top: screen_pos.y + 30.0,
            scale: 1.0,
            bounds: TextBounds {
                left: screen_pos.x as i32,
                top: screen_pos.y as i32,
                right: (screen_pos.x + scaled_size.x) as i32,
                bottom: (screen_pos.y + scaled_size.y) as i32,
            },
            color: Color::rgba(100, 100, 100, 150),
        });
        
        // Content preview
        let preview_text = self.truncate_content(&doc.content, card.size);
        let preview_buffer = create_text_buffer(
            font_system,
            &preview_text,
            12.0 * state.zoom,
            16.0 * state.zoom,
            Some(scaled_size.x - 20.0),
            Some(scaled_size.y - 60.0),
        );
        
        text_areas.push(TextAreaData {
            buffer: preview_buffer,
            left: screen_pos.x + 10.0,
            top: screen_pos.y + 50.0,
            scale: 1.0,
            bounds: TextBounds {
                left: screen_pos.x as i32,
                top: screen_pos.y as i32,
                right: (screen_pos.x + scaled_size.x) as i32,
                bottom: (screen_pos.y + scaled_size.y) as i32,
            },
            color: Color::rgb(180, 180, 180),
        });
        
        // Metadata footer
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
            top: screen_pos.y + scaled_size.y - 20.0,
            scale: 1.0,
            bounds: TextBounds {
                left: screen_pos.x as i32,
                top: screen_pos.y as i32,
                right: (screen_pos.x + scaled_size.x) as i32,
                bottom: (screen_pos.y + scaled_size.y) as i32,
            },
            color: Color::rgba(120, 120, 120, 200),
        });
    }
    
    fn truncate_content(&self, content: &str, card_size: Vec2) -> String {
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
}
