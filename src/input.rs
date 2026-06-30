use glam::Vec2;
use winit::event::{WindowEvent, MouseButton, ElementState, MouseScrollDelta};
use crate::state::AppState;
use crate::ui::container::ContainerType;

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

                        // Start panning with middle mouse or left+super
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
                    MouseScrollDelta::LineDelta(_, y) => *y * 20.0,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                };

                // Check if mouse is over a container - if so, scroll it
                let canvas_pos = state.screen_to_canvas(self.mouse_position);
                let mut scrolled_container = false;

                for container in &mut state.containers {
                    if container.is_mouse_over(canvas_pos) {
                        container.scroll(-scroll_amount);
                        scrolled_container = true;
                        break;
                    }
                }

                // If not over a container, zoom the canvas
                if !scrolled_container {
                    let zoom_factor = 1.0 + scroll_amount * 0.005;
                    state.zoom_at_point(self.mouse_position, zoom_factor);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event: key_event, .. } => {
                if key_event.state == ElementState::Pressed {
                    match &key_event.logical_key {
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) => {
                            if state.search_active {
                                state.search_active = false;
                                state.search_query.clear();
                                state.search_results.clear();
                                state.containers.retain(|c| c.container_type != ContainerType::SearchResults);
                            } else {
                                state.selected_folder = None;
                                for container in &mut state.containers {
                                    for card in &mut container.cards {
                                        card.is_selected = false;
                                    }
                                }
                            }
                        }
                        winit::keyboard::Key::Character(c) => {
                            if c.as_ref() == "k" && self.modifiers.super_key() {
                                state.search_active = !state.search_active;
                                if state.search_active {
                                    state.search_query.clear();
                                } else {
                                    state.search_query.clear();
                                    state.search_results.clear();
                                    state.containers.retain(|c| c.container_type != ContainerType::SearchResults);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::Focused(_) => {
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
