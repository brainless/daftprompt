pub mod canvas;
pub mod drawer;
pub mod card;
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
        // Update UI components based on input
        self.drawer.update(state, input);
        self.search.update(state, input);
        self.canvas.update(state, input);
    }
    
    pub fn render<'a>(&self, state: &AppState, text_areas: &mut Vec<TextAreaData>, font_system: &mut FontSystem) {
        // Render canvas background
        self.canvas.render(state, text_areas, font_system);
        
        // Render document cards
        for card in &state.cards {
            if let Some(doc) = state.documents.get(card.document_id) {
                let screen_pos = state.canvas_to_screen(card.position);
                
                // Viewport culling - only render if visible
                if self.is_visible(screen_pos, card.size, state.window_size) {
                    self.render_card(card, doc, state, text_areas, font_system);
                }
            }
        }
        
        // Render drawer
        self.drawer.render(state, text_areas, font_system);
        
        // Render search box
        self.search.render(state, text_areas, font_system);
    }
    
    fn is_visible(&self, position: glam::Vec2, size: glam::Vec2, window_size: glam::Vec2) -> bool {
        position.x + size.x > 0.0
            && position.x < window_size.x
            && position.y + size.y > 0.0
            && position.y < window_size.y
    }
    
    fn render_card(
        &self,
        card: &crate::state::CardData,
        doc: &crate::state::DocumentData,
        state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        let screen_pos = state.canvas_to_screen(card.position);
        let scaled_size = card.size * state.zoom;
        
        // Card title
        let title_text = doc.title.clone();
        let mut title_buffer = Buffer::new(font_system, Metrics::new(14.0 * state.zoom, 20.0 * state.zoom));
        title_buffer.set_size(font_system, Some(scaled_size.x - 20.0), Some(20.0 * state.zoom));
        title_buffer.set_text(
            font_system,
            &title_text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        title_buffer.shape_until_scroll(font_system, false);
        
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
            color: if state.system_theme == crate::state::SystemTheme::Dark {
                Color::rgb(255, 255, 255)
            } else {
                Color::rgb(0, 0, 0)
            },
        });
        
        // Card content preview
        let preview_text = if doc.content.len() > 100 {
            format!("{}...", &doc.content[..100])
        } else {
            doc.content.clone()
        };
        
        let mut preview_buffer = Buffer::new(font_system, Metrics::new(12.0 * state.zoom, 16.0 * state.zoom));
        preview_buffer.set_size(font_system, Some(scaled_size.x - 20.0), Some(scaled_size.y - 60.0));
        preview_buffer.set_text(
            font_system,
            &preview_text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        preview_buffer.shape_until_scroll(font_system, false);
        
        text_areas.push(TextAreaData {
            buffer: preview_buffer,
            left: screen_pos.x + 10.0,
            top: screen_pos.y + 40.0,
            scale: 1.0,
            bounds: TextBounds {
                left: screen_pos.x as i32,
                top: screen_pos.y as i32,
                right: (screen_pos.x + scaled_size.x) as i32,
                bottom: (screen_pos.y + scaled_size.y) as i32,
            },
            color: if state.system_theme == crate::state::SystemTheme::Dark {
                Color::rgb(180, 180, 180)
            } else {
                Color::rgb(80, 80, 80)
            },
        });
    }
}

fn create_text_buffer(
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
