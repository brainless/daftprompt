use crate::state::AppState;
use crate::input::InputHandler;
use crate::renderer::TextAreaData;
use crate::ui::create_text_buffer;
use glyphon::{Color, FontSystem, TextBounds};

pub struct Canvas {
    grid_visible: bool,
}

impl Canvas {
    pub fn new() -> Self {
        Self {
            grid_visible: true,
        }
    }
    
    pub fn update(&mut self, _state: &mut AppState, _input: &InputHandler) {
        // Canvas updates are handled in state
    }
    
    pub fn render(
        &self,
        state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        if !self.grid_visible {
            return;
        }
        
        // Render grid lines
        self.render_grid(state, text_areas, font_system);
        
        // Render zoom indicator
        self.render_zoom_indicator(state, text_areas, font_system);
    }
    
    fn render_grid(
        &self,
        state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        let grid_spacing = 50.0 * state.zoom;
        let offset_x = state.pan_offset.x * state.zoom % grid_spacing;
        let offset_y = state.pan_offset.y * state.zoom % grid_spacing;
        
        // Only render grid if spacing is large enough to be visible
        if grid_spacing < 10.0 {
            return;
        }
        
        let window_size = state.window_size;
        let mut x = offset_x;
        while x < window_size.x {
            // Vertical grid line indicator
            let line_text = format!("{}", ((x - offset_x) / grid_spacing + state.pan_offset.x / 50.0) as i32);
            let buffer = create_text_buffer(
                font_system,
                &line_text,
                10.0,
                12.0,
                Some(30.0),
                Some(12.0),
            );
            
            text_areas.push(TextAreaData {
                buffer,
                left: x,
                top: 5.0,
                scale: 1.0,
                bounds: TextBounds {
                    left: x as i32,
                    top: 0,
                    right: (x + 30.0) as i32,
                    bottom: 12,
                },
                color: Color::rgba(100, 100, 100, 128),
            });
            
            x += grid_spacing;
        }
        
        let mut y = offset_y;
        while y < window_size.y {
            // Horizontal grid line indicator
            let line_text = format!("{}", ((y - offset_y) / grid_spacing + state.pan_offset.y / 50.0) as i32);
            let buffer = create_text_buffer(
                font_system,
                &line_text,
                10.0,
                12.0,
                Some(30.0),
                Some(12.0),
            );
            
            text_areas.push(TextAreaData {
                buffer,
                left: 5.0,
                top: y,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: y as i32,
                    right: 30,
                    bottom: (y + 12.0) as i32,
                },
                color: Color::rgba(100, 100, 100, 128),
            });
            
            y += grid_spacing;
        }
    }
    
    fn render_zoom_indicator(
        &self,
        state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        let zoom_text = format!("{}%", (state.zoom * 100.0) as i32);
        let buffer = create_text_buffer(
            font_system,
            &zoom_text,
            14.0,
            18.0,
            Some(80.0),
            Some(18.0),
        );
        
        text_areas.push(TextAreaData {
            buffer,
            left: state.window_size.x - 80.0,
            top: state.window_size.y - 30.0,
            scale: 1.0,
            bounds: TextBounds {
                left: (state.window_size.x - 80.0) as i32,
                top: (state.window_size.y - 30.0) as i32,
                right: state.window_size.x as i32,
                bottom: state.window_size.y as i32,
            },
            color: Color::rgba(150, 150, 150, 200),
        });
    }
}
