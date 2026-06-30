use glam::Vec2;
use winit::event::{WindowEvent, MouseButton, ElementState, MouseScrollDelta};
use crate::state::AppState;

pub struct InputHandler {
    pub mouse_position: Vec2,
    pub mouse_buttons: Vec<MouseButton>,
    pub modifiers: winit::keyboard::ModifiersState,
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            mouse_position: Vec2::ZERO,
            mouse_buttons: Vec::new(),
            modifiers: winit::keyboard::ModifiersState::empty(),
        }
    }
    
    pub fn handle_event(&mut self, event: &WindowEvent, state: &mut AppState) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_position = Vec2::new(position.x as f32, position.y as f32);
                
                if state.is_panning {
                    state.update_panning(self.mouse_position);
                }
            }
            WindowEvent::MouseInput { state: button_state, button, .. } => {
                match button_state {
                    ElementState::Pressed => {
                        if !self.mouse_buttons.contains(button) {
                            self.mouse_buttons.push(*button);
                        }
                        
                        // Start panning with middle mouse or left+space
                        if *button == MouseButton::Middle || 
                           (*button == MouseButton::Left && self.modifiers.super_key()) {
                            state.start_panning(self.mouse_position);
                        }
                    }
                    ElementState::Released => {
                        self.mouse_buttons.retain(|b| b != button);
                        
                        if *button == MouseButton::Middle || 
                           (*button == MouseButton::Left && state.is_panning) {
                            state.stop_panning();
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y * 0.1,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32 * 0.001,
                };
                
                let zoom_factor = 1.0 + scroll_amount;
                state.zoom_at_point(self.mouse_position, zoom_factor);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event: key_event, .. } => {
                if key_event.state == ElementState::Pressed {
                    // Handle keyboard shortcuts
                    match &key_event.logical_key {
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) => {
                            // Escape - Close search or deselect
                            if state.search_active {
                                state.search_active = false;
                                state.search_query.clear();
                            } else {
                                state.selected_folder = None;
                                for card in &mut state.cards {
                                    card.is_selected = false;
                                }
                            }
                        }
                        winit::keyboard::Key::Character(c) => {
                            if c.as_ref() == "k" && self.modifiers.super_key() {
                                // Cmd+K or Ctrl+K - Toggle search
                                state.search_active = !state.search_active;
                                if state.search_active {
                                    state.search_query.clear();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::Focused(_) => {
                // Reset modifiers when window loses focus
                self.modifiers = winit::keyboard::ModifiersState::empty();
            }
            _ => {}
        }
    }
    
    pub fn is_mouse_over_rect(&self, position: Vec2, size: Vec2) -> bool {
        self.mouse_position.x >= position.x
            && self.mouse_position.x <= position.x + size.x
            && self.mouse_position.y >= position.y
            && self.mouse_position.y <= position.y + size.y
    }
}
