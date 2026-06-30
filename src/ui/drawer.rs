use crate::state::{AppState, IconType};
use crate::input::InputHandler;
use crate::renderer::TextAreaData;
use crate::ui::create_text_buffer;
use glam::Vec2;
use glyphon::{Color, FontSystem, TextBounds};

pub struct Drawer {
    pub is_open: bool,
    pub width: f32,
    pub expanded_width: f32,
    pub hover_index: Option<usize>,
    pub animation_progress: f32,
}

impl Drawer {
    pub fn new() -> Self {
        Self {
            is_open: true,
            width: 60.0,
            expanded_width: 250.0,
            hover_index: None,
            animation_progress: 1.0,
        }
    }
    
    pub fn update(&mut self, state: &mut AppState, input: &InputHandler) {
        let drawer_rect = self.get_rect(state);
        
        // Check if mouse is over drawer
        if input.is_mouse_over_rect(drawer_rect, Vec2::new(self.get_current_width(), state.window_size.y)) {
            // Find which folder icon is hovered
            let icon_height = 60.0;
            let icon_padding = 10.0;
            let start_y = 20.0;
            
            for (i, _folder) in state.folders.iter().enumerate() {
                let icon_y = start_y + i as f32 * (icon_height + icon_padding);
                let icon_rect = Vec2::new(drawer_rect.x + 10.0, icon_y);
                let icon_size = Vec2::new(40.0, icon_height);
                
                if input.is_mouse_over_rect(icon_rect, icon_size) {
                    self.hover_index = Some(i);
                    break;
                }
            }
        } else {
            self.hover_index = None;
        }
        
        // Animate drawer
        if self.is_open {
            self.animation_progress = (self.animation_progress + 0.1).min(1.0);
        } else {
            self.animation_progress = (self.animation_progress - 0.1).max(0.0);
        }
    }
    
    pub fn get_rect(&self, _state: &AppState) -> Vec2 {
        Vec2::new(0.0, 0.0)
    }
    
    pub fn get_current_width(&self) -> f32 {
        self.width + (self.expanded_width - self.width) * self.animation_progress
    }
    
    pub fn render(
        &self,
        state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        let current_width = self.get_current_width();
        let drawer_height = state.window_size.y;
        
        // Drawer background
        let bg_text = " ".repeat((current_width * drawer_height / 100.0) as usize);
        let bg_buffer = create_text_buffer(
            font_system,
            &bg_text,
            10.0,
            12.0,
            Some(current_width),
            Some(drawer_height),
        );
        
        text_areas.push(TextAreaData {
            buffer: bg_buffer,
            left: 0.0,
            top: 0.0,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: 0,
                right: current_width as i32,
                bottom: drawer_height as i32,
            },
            color: Color::rgba(40, 40, 40, 200),
        });
        
        // Render folder icons
        let icon_height = 60.0;
        let icon_padding = 10.0;
        let start_y = 20.0;
        
        for (i, folder) in state.folders.iter().enumerate() {
            let icon_y = start_y + i as f32 * (icon_height + icon_padding);
            let is_selected = state.selected_folder == Some(i);
            let is_hovered = self.hover_index == Some(i);
            
            self.render_folder_icon(
                folder,
                i,
                is_selected,
                is_hovered,
                current_width,
                icon_y,
                icon_height,
                state,
                text_areas,
                font_system,
            );
        }
    }
    
    fn render_folder_icon(
        &self,
        folder: &crate::state::FolderData,
        _index: usize,
        is_selected: bool,
        is_hovered: bool,
        drawer_width: f32,
        y: f32,
        height: f32,
        _state: &AppState,
        text_areas: &mut Vec<TextAreaData>,
        font_system: &mut FontSystem,
    ) {
        let icon_x = 10.0;
        let icon_width = 40.0;
        
        // Icon background (highlight if selected/hovered)
        let bg_color = if is_selected {
            Color::rgba(0, 122, 255, 100)
        } else if is_hovered {
            Color::rgba(60, 60, 60, 150)
        } else {
            Color::rgba(50, 50, 50, 100)
        };
        
        let bg_text = " ".repeat((icon_width * height / 100.0) as usize);
        let bg_buffer = create_text_buffer(
            font_system,
            &bg_text,
            10.0,
            12.0,
            Some(icon_width),
            Some(height),
        );
        
        text_areas.push(TextAreaData {
            buffer: bg_buffer,
            left: icon_x,
            top: y,
            scale: 1.0,
            bounds: TextBounds {
                left: icon_x as i32,
                top: y as i32,
                right: (icon_x + icon_width) as i32,
                bottom: (y + height) as i32,
            },
            color: bg_color,
        });
        
        // Icon symbol
        let icon_symbol = self.get_icon_symbol(folder.icon);
        let icon_buffer = create_text_buffer(
            font_system,
            icon_symbol,
            20.0,
            24.0,
            Some(icon_width),
            Some(height),
        );
        
        text_areas.push(TextAreaData {
            buffer: icon_buffer,
            left: icon_x,
            top: y + (height - 24.0) / 2.0,
            scale: 1.0,
            bounds: TextBounds {
                left: icon_x as i32,
                top: y as i32,
                right: (icon_x + icon_width) as i32,
                bottom: (y + height) as i32,
            },
            color: Color::rgb(255, 255, 255),
        });
        
        // Folder name (when expanded)
        if self.animation_progress > 0.5 && drawer_width > 100.0 {
            let name_x = icon_x + icon_width + 10.0;
            let name_width = drawer_width - name_x - 10.0;
            
            if name_width > 0.0 {
                let name_buffer = create_text_buffer(
                    font_system,
                    &folder.name,
                    12.0,
                    16.0,
                    Some(name_width),
                    Some(height),
                );
                
                text_areas.push(TextAreaData {
                    buffer: name_buffer,
                    left: name_x,
                    top: y + (height - 16.0) / 2.0,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: name_x as i32,
                        top: y as i32,
                        right: (name_x + name_width) as i32,
                        bottom: (y + height) as i32,
                    },
                    color: Color::rgb(220, 220, 220),
                });
                
                // Document count
                let count_text = format!("{} docs", folder.document_count);
                let count_buffer = create_text_buffer(
                    font_system,
                    &count_text,
                    10.0,
                    12.0,
                    Some(name_width),
                    Some(12.0),
                );
                
                text_areas.push(TextAreaData {
                    buffer: count_buffer,
                    left: name_x,
                    top: y + height - 20.0,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: name_x as i32,
                        top: (y + height - 20.0) as i32,
                        right: (name_x + name_width) as i32,
                        bottom: (y + height) as i32,
                    },
                    color: Color::rgba(150, 150, 150, 200),
                });
            }
        }
    }
    
    fn get_icon_symbol(&self, icon_type: IconType) -> &'static str {
        match icon_type {
            IconType::Folder => "📁",
            IconType::GitRepo => "🔀",
            IconType::Document => "📄",
            IconType::Code => "💻",
            IconType::Markdown => "📝",
            IconType::Search => "🔍",
            IconType::Settings => "⚙️",
        }
    }
}
