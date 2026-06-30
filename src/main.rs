mod renderer;
mod state;
mod input;
mod ui;
mod git_log;

use std::path::PathBuf;

use clap::Parser;
use state::AppState;
use renderer::Renderer;
use input::InputHandler;
use ui::UIManager;
use ui::container::Container;

#[derive(Parser)]
#[command(name = "text-explorer", about = "A text repository explorer")]
struct Args {
    /// Path to git repository to read log from
    #[arg(short, long, default_value = ".")]
    repo: PathBuf,

    /// Number of commits to display (default: all)
    #[arg(short, long)]
    count: Option<usize>,
}

fn main() {
    env_logger::init();

    let args = Args::parse();

    // Read git log
    let commits = match git_log::read_log(&args.repo) {
        Ok(mut commits) => {
            if let Some(count) = args.count {
                commits.truncate(count);
            }
            println!("Loaded {} commits from {:?}", commits.len(), args.repo);
            commits
        }
        Err(e) => {
            eprintln!("Failed to read git log: {}. Using empty log.", e);
            Vec::new()
        }
    };

    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    event_loop
        .run_app(&mut Application {
            state: None,
            renderer: None,
            input_handler: None,
            ui_manager: None,
            window: None,
            commits,
        })
        .unwrap();
}

struct Application {
    state: Option<AppState>,
    renderer: Option<Renderer>,
    input_handler: Option<InputHandler>,
    ui_manager: Option<UIManager>,
    window: Option<std::sync::Arc<winit::window::Window>>,
    commits: Vec<git_log::CommitInfo>,
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

        let mut state = AppState::new(window.inner_size().into());

        // Create git log container
        if !self.commits.is_empty() {
            let container_width = 500.0;
            let container_height = state.window_size.y - 40.0; // 20px padding top/bottom
            let container = Container::new_git_log(
                0,
                glam::Vec2::new(80.0, 20.0),
                container_width,
                container_height,
                self.commits.clone(),
            );
            state.containers.push(container);
        }

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
