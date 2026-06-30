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
use sugacode_indexer::{Indexer, IndexerConfig, CommitData};

#[derive(Parser)]
#[command(name = "text-explorer", about = "A text repository explorer")]
struct Args {
    #[arg(short, long, default_value = ".")]
    repo: PathBuf,

    #[arg(short, long)]
    count: Option<usize>,

    #[arg(long)]
    index: bool,

    #[arg(long)]
    reindex: bool,

    #[arg(long)]
    no_index: bool,

    #[arg(long)]
    search: Option<String>,
}

impl From<&git_log::CommitInfo> for CommitData {
    fn from(c: &git_log::CommitInfo) -> Self {
        CommitData {
            sha: c.sha.clone(),
            short_hash: c.short_hash.clone(),
            author_name: c.author_name.clone(),
            time: c.time.clone(),
            message_title: c.message_title.clone(),
            message_body: c.message_body.clone(),
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    let indexer_config = IndexerConfig::default();

    if args.index || args.reindex {
        let mut commits = git_log::read_log_all_branches(&args.repo)?;
        if let Some(count) = args.count {
            commits.truncate(count);
        }
        let commit_data: Vec<CommitData> = commits.iter().map(Into::into).collect();
        let mut indexer = Indexer::new(&args.repo, &indexer_config)?;
        let n = if args.reindex {
            indexer.reindex_commits(&commit_data)?
        } else {
            indexer.index_commits(&commit_data)?
        };
        println!("Indexed {} commits", n);
        if args.search.is_none() {
            return Ok(());
        }
    }

    if let Some(query) = &args.search {
        let indexer = Indexer::new(&args.repo, &indexer_config)?;
        let results = indexer.search_hybrid(query, 10)?;
        for r in &results {
            let id = r.short_hash.as_str();
            let title = r.text.lines().next().unwrap_or("");
            println!("[{:.3}] {:<7} {} — {}", r.score, id, r.author.as_deref().unwrap_or(""), title);
        }
        return Ok(());
    }

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

    let indexer = if !args.no_index {
        match Indexer::new(&args.repo, &indexer_config) {
            Ok(mut indexer) => {
                match git_log::read_log_all_branches(&args.repo) {
                    Ok(all_commits) => {
                        let commit_data: Vec<CommitData> = all_commits.iter().map(Into::into).collect();
                        match indexer.index_commits(&commit_data) {
                            Ok(n) => log::info!("Indexed {n} new commits"),
                            Err(e) => log::warn!("Failed to index commits: {e}"),
                        }
                    }
                    Err(e) => log::warn!("Failed to read all branches: {e}"),
                }
                Some(indexer)
            }
            Err(e) => {
                log::warn!("Failed to create indexer: {e}");
                None
            }
        }
    } else {
        None
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
            indexer,
        })
        .unwrap();

    Ok(())
}

struct Application {
    state: Option<AppState>,
    renderer: Option<Renderer>,
    input_handler: Option<InputHandler>,
    ui_manager: Option<UIManager>,
    window: Option<std::sync::Arc<winit::window::Window>>,
    commits: Vec<git_log::CommitInfo>,
    indexer: Option<Indexer>,
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

        state.indexer = self.indexer.take();

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
