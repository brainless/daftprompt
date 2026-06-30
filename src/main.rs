mod renderer;
mod state;
mod input;
mod ui;

use state::AppState;
use renderer::Renderer;
use input::InputHandler;
use ui::UIManager;

fn main() {
    env_logger::init();
    
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    event_loop
        .run_app(&mut Application {
            state: None,
            renderer: None,
            input_handler: None,
            ui_manager: None,
            window: None,
        })
        .unwrap();
}

struct Application {
    state: Option<AppState>,
    renderer: Option<Renderer>,
    input_handler: Option<InputHandler>,
    ui_manager: Option<UIManager>,
    window: Option<std::sync::Arc<winit::window::Window>>,
}

impl winit::application::ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attributes = winit::window::Window::default_attributes()
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0))
            .with_title("Text Explorer")
            .with_maximized(true);
        
        let window = std::sync::Arc::new(
            event_loop.create_window(window_attributes).unwrap()
        );

        let state = AppState::new(window.inner_size().into());
        let renderer = pollster::block_on(Renderer::new(window.clone(), event_loop));
        let input_handler = InputHandler::new();
        let ui_manager = UIManager::new();

        self.state = Some(state);
        self.renderer = Some(renderer);
        self.input_handler = Some(input_handler);
        self.ui_manager = Some(ui_manager);
        self.window = Some(window.clone());
        
        // Request initial redraw
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let (Some(state), Some(renderer), Some(input_handler), Some(ui_manager), Some(window)) = 
            (&mut self.state, &mut self.renderer, &mut self.input_handler, &mut self.ui_manager, &self.window)
        else {
            return;
        };

        // Handle input events
        input_handler.handle_event(&event, state);

        // Update UI based on input
        ui_manager.update(state, input_handler);

        match event {
            winit::event::WindowEvent::Resized(size) => {
                state.resize(size.into());
                renderer.resize(size.into());
                window.request_redraw();
            }
            winit::event::WindowEvent::RedrawRequested => {
                renderer.render(state, ui_manager);
            }
            winit::event::WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            winit::event::WindowEvent::CursorMoved { .. } => {
                window.request_redraw();
            }
            winit::event::WindowEvent::MouseInput { .. } => {
                window.request_redraw();
            }
            winit::event::WindowEvent::MouseWheel { .. } => {
                window.request_redraw();
            }
            winit::event::WindowEvent::KeyboardInput { .. } => {
                window.request_redraw();
            }
            _ => {}
        }
    }
}
