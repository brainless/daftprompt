mod config;
mod coordinator;
mod git_log;
mod state;
mod ui;

use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use config::{AdapterArgs, DaftpromptConfig};
use coordinator::{start_coordinator, CoordinatorCommand, CoordinatorEvent};
use daftprompt_acp::{AcpClient, AcpClientConfig, PermissionOutcome};
use daftprompt_indexer::{CommitData, Indexer, IndexerConfig, SymbolKind, UnifiedSearchHit};
use daftprompt_prompt_builder::{RetrievalStatus, TruncationReason};
use daftprompt_storage::ConversationStore;
use state::AppState;
use tokio::sync::mpsc;
use ui::container::{Container, ContainerType};
use ui::conversation::render_conversation;
use ui::render::{render_canvas, render_drawer, render_search};

#[derive(Parser)]
#[command(name = "daftprompt", about = "A text repository explorer")]
struct Args {
    #[arg(short, long, default_value = ".")]
    repo: PathBuf,

    #[arg(short, long)]
    count: Option<usize>,

    /// Index all sources (git log, code, documents) incrementally.
    #[arg(long)]
    index: bool,

    /// Rebuild all indexes (git log, code, documents).
    #[arg(long)]
    reindex: bool,

    #[arg(long)]
    no_index: bool,

    /// Hybrid-search all sources and print one combined, cross-source ranked result list.
    #[arg(long)]
    search: Option<String>,

    // --- Source-specific git-log commands ---
    /// Incrementally index git log only.
    #[arg(long)]
    index_git_log: bool,

    /// Drop and rebuild git-log index only.
    #[arg(long)]
    reindex_git_log: bool,

    /// Hybrid-search git log only.
    #[arg(long)]
    search_git_log: Option<String>,

    // --- Source-specific code commands ---
    /// Incrementally index Rust, TypeScript, TSX, JavaScript, and JSX source code only.
    #[arg(long)]
    index_code: bool,

    /// Drop and rebuild code index only.
    #[arg(long)]
    reindex_code: bool,

    /// Hybrid-search code only.
    #[arg(long)]
    search_code: Option<String>,

    // --- Source-specific document commands ---
    /// Incrementally index documents (Markdown, plain text) only.
    #[arg(long)]
    index_documents: bool,

    /// Drop and rebuild document index only.
    #[arg(long)]
    reindex_documents: bool,

    /// Hybrid-search documents only.
    #[arg(long)]
    search_documents: Option<String>,

    /// Task 8: capture one frame to a PNG file after a 5s settle delay.
    /// Combine with `--exit` to terminate the GUI after the capture.
    #[arg(long)]
    screenshot: Option<PathBuf>,

    /// Task 8: exit the GUI after `--screenshot` writes its PNG. No-op
    /// without `--screenshot` (matches akar's demo behavior).
    #[arg(long)]
    exit: bool,

    #[command(flatten)]
    adapter_args: AdapterArgs,
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
    let config = DaftpromptConfig::from_args(&args.adapter_args);

    let indexer_config = IndexerConfig::default();

    // Validate: reject ambiguous combinations of generic + source-specific flags.
    let has_generic_index = args.index || args.reindex;
    let has_source_index = args.index_code
        || args.reindex_code
        || args.index_git_log
        || args.reindex_git_log
        || args.index_documents
        || args.reindex_documents;
    if has_generic_index && has_source_index {
        eprintln!("Error: cannot combine --index/--reindex with source-specific index flags.");
        std::process::exit(1);
    }
    let has_generic_search = args.search.is_some();
    let has_source_search = args.search_code.is_some()
        || args.search_git_log.is_some()
        || args.search_documents.is_some();
    if has_generic_search && has_source_search {
        eprintln!("Error: cannot combine --search with source-specific search flags.");
        std::process::exit(1);
    }

    // --- Source-specific git-log operations ---
    if args.index_git_log || args.reindex_git_log {
        let mut commits = git_log::read_log_all_branches(&args.repo)?;
        if let Some(count) = args.count {
            commits.truncate(count);
        }
        let commit_data: Vec<CommitData> = commits.iter().map(Into::into).collect();
        let mut indexer = Indexer::new(&args.repo, &indexer_config)?;
        let n = if args.reindex_git_log {
            indexer.reindex_commits(&commit_data)?
        } else {
            indexer.index_commits(&commit_data)?
        };
        println!("Indexed {} commits", n);
        if args.search_git_log.is_none() {
            return Ok(());
        }
    }

    if let Some(query) = &args.search_git_log {
        let indexer = Indexer::new(&args.repo, &indexer_config)?;
        let results = indexer.search_hybrid(query, 10)?;
        for r in &results {
            let id = r.short_hash.as_str();
            let title = r.text.lines().next().unwrap_or("");
            println!(
                "[{:.3}] {:<7} {} — {} [Git log]",
                r.score,
                id,
                r.author.as_deref().unwrap_or(""),
                title
            );
        }
        return Ok(());
    }

    // --- Source-specific code operations ---
    if args.index_code || args.reindex_code {
        let mut indexer = Indexer::new(&args.repo, &indexer_config)?;
        let report = if args.reindex_code {
            indexer.reindex_code()?
        } else {
            indexer.index_code()?
        };
        println!(
            "Code index: {} files scanned, {} changed, {} deleted, {} symbols indexed",
            report.files_scanned,
            report.files_changed,
            report.files_deleted,
            report.symbols_indexed
        );
        if args.search_code.is_none() {
            return Ok(());
        }
    }

    if let Some(query) = &args.search_code {
        let indexer = Indexer::new(&args.repo, &indexer_config)?;
        let results = indexer.search_code_hybrid(query, 10)?;
        for r in &results {
            let _kind = format!("{:?}", r.symbol_kind).to_lowercase();
            let kind_abbr = match r.symbol_kind {
                SymbolKind::Function => "fn",
                SymbolKind::Struct => "struct",
                SymbolKind::Enum => "enum",
                SymbolKind::Trait => "trait",
                SymbolKind::ImplMethod => "fn",
                SymbolKind::TraitMethod => "fn",
                SymbolKind::TypeAlias => "type",
                SymbolKind::Const => "const",
                SymbolKind::Static => "static",
                SymbolKind::Module => "mod",
                SymbolKind::Macro => "macro",
                SymbolKind::Comments => "comments",
                SymbolKind::Imports => "imports",
                // Epic 009 Task 1: TypeScript/TSX kinds. These never
                // appear for Rust-only indexes, but the CLI must compile
                // against the shared `SymbolKind` enum.
                SymbolKind::Class => "class",
                SymbolKind::Interface => "iface",
                SymbolKind::Method => "method",
                SymbolKind::Variable => "var",
                SymbolKind::Unknown(_) => "?",
            };
            println!(
                "[{:.3}] {:<8} {}:{}       {} — {} [Code]",
                r.score,
                kind_abbr,
                r.file_path,
                r.line_start,
                r.identifier.split("::").last().unwrap_or(""),
                r.text.lines().next().unwrap_or("")
            );
        }
        return Ok(());
    }

    // --- Source-specific document operations ---
    if args.index_documents || args.reindex_documents {
        let mut indexer = Indexer::new(&args.repo, &indexer_config)?;
        let report = if args.reindex_documents {
            indexer.reindex_documents()?
        } else {
            indexer.index_documents()?
        };
        println!(
            "Document index: {} files scanned, {} changed, {} deleted, {} chunks indexed",
            report.files_scanned, report.files_changed, report.files_deleted, report.chunks_indexed
        );
        if args.search_documents.is_none() {
            return Ok(());
        }
    }

    if let Some(query) = &args.search_documents {
        let indexer = Indexer::new(&args.repo, &indexer_config)?;
        let results = indexer.search_document_hybrid(query, 10)?;
        for r in &results {
            let title = r.text.lines().next().unwrap_or("");
            println!("[{:.3}] {} — {} [Documents]", r.score, r.file_path, title);
        }
        return Ok(());
    }

    // --- Generic all-source operations ---
    if args.index || args.reindex {
        let mut indexer = Indexer::new(&args.repo, &indexer_config)?;

        // Git log
        let mut commits = git_log::read_log_all_branches(&args.repo)?;
        if let Some(count) = args.count {
            commits.truncate(count);
        }
        let commit_data: Vec<CommitData> = commits.iter().map(Into::into).collect();
        let n = if args.reindex {
            indexer.reindex_commits(&commit_data)?
        } else {
            indexer.index_commits(&commit_data)?
        };
        println!("Git log: indexed {} commits", n);

        // Code
        let code_report = if args.reindex {
            indexer.reindex_code()?
        } else {
            indexer.index_code()?
        };
        println!(
            "Code: {} files scanned, {} changed, {} deleted, {} symbols indexed",
            code_report.files_scanned,
            code_report.files_changed,
            code_report.files_deleted,
            code_report.symbols_indexed
        );

        // Documents
        let doc_report = if args.reindex {
            indexer.reindex_documents()?
        } else {
            indexer.index_documents()?
        };
        println!(
            "Documents: {} files scanned, {} changed, {} deleted, {} chunks indexed",
            doc_report.files_scanned,
            doc_report.files_changed,
            doc_report.files_deleted,
            doc_report.chunks_indexed
        );

        if args.search.is_none() {
            return Ok(());
        }
    }

    if let Some(query) = &args.search {
        let indexer = Indexer::new(&args.repo, &indexer_config)?;
        let results = indexer.search_all_hybrid(query, 10)?;
        for (i, hit) in results.combined.iter().enumerate() {
            match hit {
                UnifiedSearchHit::GitLog(r) => {
                    let id = r.short_hash.as_str();
                    let title = r.text.lines().next().unwrap_or("");
                    println!(
                        "{:<3} [{:.3}] {:<7} {} — {} [Git log]",
                        i + 1,
                        r.score,
                        id,
                        r.author.as_deref().unwrap_or(""),
                        title
                    );
                }
                UnifiedSearchHit::Code(r) => {
                    let kind_abbr = match r.symbol_kind {
                        SymbolKind::Function => "fn",
                        SymbolKind::Struct => "struct",
                        SymbolKind::Enum => "enum",
                        SymbolKind::Trait => "trait",
                        SymbolKind::ImplMethod => "fn",
                        SymbolKind::TraitMethod => "fn",
                        SymbolKind::TypeAlias => "type",
                        SymbolKind::Const => "const",
                        SymbolKind::Static => "static",
                        SymbolKind::Module => "mod",
                        SymbolKind::Macro => "macro",
                        SymbolKind::Comments => "comments",
                        SymbolKind::Imports => "imports",
                        // Epic 009 Task 1: TS/TSX kinds + Unknown fallback.
                        SymbolKind::Class => "class",
                        SymbolKind::Interface => "iface",
                        SymbolKind::Method => "method",
                        SymbolKind::Variable => "var",
                        SymbolKind::Unknown(_) => "?",
                    };
                    println!(
                        "{:<3} [{:.3}] {:<8} {}:{}       {} — {} [Code]",
                        i + 1,
                        r.score,
                        kind_abbr,
                        r.file_path,
                        r.line_start,
                        r.identifier.split("::").last().unwrap_or(""),
                        r.text.lines().next().unwrap_or("")
                    );
                }
                UnifiedSearchHit::Document(r) => {
                    let title = r.text.lines().next().unwrap_or("");
                    println!(
                        "{:<3} [{:.3}] {} — {} [Documents]",
                        i + 1,
                        r.score,
                        r.file_path,
                        title
                    );
                }
            }
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
                        let commit_data: Vec<CommitData> =
                            all_commits.iter().map(Into::into).collect();
                        match indexer.index_commits(&commit_data) {
                            Ok(n) => log::info!("Indexed {n} new commits"),
                            Err(e) => log::warn!("Failed to index commits: {e}"),
                        }
                    }
                    Err(e) => log::warn!("Failed to read all branches: {e}"),
                }
                match indexer.index_code() {
                    Ok(report) => log::info!(
                        "Indexed {} code symbols from {} files",
                        report.symbols_indexed,
                        report.files_changed
                    ),
                    Err(e) => log::warn!("Failed to index code: {e}"),
                }
                match indexer.index_documents() {
                    Ok(report) => log::info!(
                        "Indexed {} document chunks from {} files",
                        report.chunks_indexed,
                        report.files_changed
                    ),
                    Err(e) => log::warn!("Failed to index documents: {e}"),
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
            core: None,
            device: None,
            queue: None,
            surface: None,
            surface_config: None,
            window: None,
            commits,
            indexer,
            repo_path: args.repo,
            config,
            last_frame: None,
            screenshot_path: args.screenshot,
            exit_after_screenshot: args.exit,
            start_time: None,
            screenshot_taken: false,
            tokio_runtime: None,
            coordinator_cmd: None,
            coordinator_evt: None,
        })
        .unwrap();

    Ok(())
}

struct Application {
    state: Option<AppState>,
    core: Option<akar_core::AkarCore>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    window: Option<std::sync::Arc<winit::window::Window>>,
    commits: Vec<git_log::CommitInfo>,
    indexer: Option<Indexer>,
    repo_path: PathBuf,
    config: DaftpromptConfig,
    // Timestamp of the previous `handle_redraw` call. Used by Task 4 (drawer)
    // to advance `state.drawer_animation` with a delta-time. None on the
    // first frame; the first `handle_redraw` then primes it.
    last_frame: Option<Instant>,
    // Task 8: visual-regression screenshot plumbing. Pattern follows
    // `akar/examples/demo-rust/src/main.rs` — start_time is primed on the
    // first frame after `screenshot_path` is set, then `handle_redraw`
    // captures once 5s have elapsed. `screenshot_taken` is a one-shot
    // guard so we only write the PNG once.
    screenshot_path: Option<PathBuf>,
    exit_after_screenshot: bool,
    start_time: Option<Instant>,
    screenshot_taken: bool,
    // Coordinator (Task 5): async runtime, command sender, event receiver.
    // The coordinator runs on a tokio runtime and communicates with the UI
    // through typed channels.
    tokio_runtime: Option<tokio::runtime::Runtime>,
    coordinator_cmd: Option<mpsc::UnboundedSender<CoordinatorCommand>>,
    coordinator_evt: Option<mpsc::UnboundedReceiver<CoordinatorEvent>>,
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

        let window = std::sync::Arc::new(event_loop.create_window(window_attributes).unwrap());

        let physical_size = window.inner_size();
        let scale_factor = window.scale_factor() as f32;

        let mut state = AppState::new(physical_size.into());
        state.scale_factor = scale_factor;
        state.indexer = self.indexer.take();

        // Create git log container. The data model is built here in `resumed` so
        // the canvas (Task 3) can immediately read it. The visual rendering of the
        // container is added in Task 5; for now it just lives in `state.containers`.
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

        // wgpu setup (daftprompt owns the window; akar owns the GPU pipeline)
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(event_loop.owned_display_handle()),
        ));
        let surface = instance
            .create_surface(window.clone())
            .expect("Failed to create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();

        let surface_format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: physical_size.width.max(1),
            height: physical_size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // AkarCore takes &Device/&Queue; create while borrows are live, then move in.
        let core = akar_core::AkarCore::new(&device, &queue, surface_format, akar_core::TextPipelineConfig::default());

        self.state = Some(state);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.core = Some(core);
        self.window = Some(window.clone());

        // Start the coordinator (Task 5). Requires the indexer; if the
        // indexer was not created (no_index or failure), the coordinator
        // is skipped and the conversation panel will show "Disconnected".
        if self.state.as_ref().unwrap().indexer.is_some() {
            // Pre-flight check: verify the adapter executable exists.
            if let Err(e) = config::check_adapter_executable(&self.config.adapter.executable) {
                log::error!("{e}");
                // Surface the error in the conversation transcript.
                if let Some(state) = self.state.as_mut() {
                    state.conversation.entries.push(state::TranscriptEntry {
                        kind: state::TranscriptEntryKind::Error,
                        text: e.clone(),
                        timestamp: timestamp_now(),
                    });
                }
            } else {
                match tokio::runtime::Runtime::new() {
                    Ok(rt) => {
                        let indexer_arc = std::sync::Arc::new(tokio::sync::Mutex::new(
                            daftprompt_indexer::Indexer::new(
                                &self.repo_path,
                                &IndexerConfig::default(),
                            )
                            .expect("Indexer::new for coordinator"),
                        ));
                        let store = match &self.config.trace.db_path {
                            Some(path) => ConversationStore::open(path)
                                .expect("ConversationStore::open"),
                            None => {
                                let db_path = ConversationStore::default_path_for_repo(&self.repo_path)
                                    .expect("default_path_for_repo");
                                ConversationStore::open(&db_path)
                                    .expect("ConversationStore::open")
                            }
                        };
                        let launch_profile = self.config.to_launch_profile();
                        let (acp_client, acp_events) = AcpClient::launch(
                            launch_profile,
                            AcpClientConfig {
                                request_timeout: std::time::Duration::from_secs(
                                    self.config.adapter.request_timeout_secs,
                                ),
                                ..AcpClientConfig::default()
                            },
                        );
                        let coord_config = self.config.to_coordinator_config();
                        let (cmd_tx, evt_rx) = start_coordinator(
                            indexer_arc,
                            store,
                            acp_client,
                            acp_events,
                            coord_config,
                        );
                        self.coordinator_cmd = Some(cmd_tx);
                        self.coordinator_evt = Some(evt_rx);
                        self.tokio_runtime = Some(rt);
                    }
                    Err(e) => {
                        log::warn!("Failed to create tokio runtime for coordinator: {e}");
                    }
                }
            }
        }

        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        // Forward to akar's input state. We pull `state` and `core` out so we can mutate them
        // freely without aliasing the other fields; the remaining fields are accessed via
        // `self.surface` / `self.window` directly inside the match.
        if let (Some(state), Some(core)) = (self.state.as_mut(), self.core.as_mut()) {
            // akar-winit forwards cursor/mouse/wheel/text but NOT modifiers. Track Cmd/Ctrl here.
            if let winit::event::WindowEvent::ModifiersChanged(m) = &event {
                state.cmd_or_ctrl = if cfg!(target_os = "macos") {
                    m.state().super_key()
                } else {
                    m.state().control_key()
                };
                state.shift_pressed = m.state().shift_key();
            }

            akar_winit::process_window_event(&mut core.input, &event);
        } else {
            return;
        }

        match event {
            winit::event::WindowEvent::Resized(size) => {
                if let (Some(state), Some(device), Some(surface)) = (
                    self.state.as_mut(),
                    self.device.as_ref(),
                    self.surface.as_ref(),
                ) {
                    state.resize(size.into());
                    let surface_config = self.surface_config.as_ref().unwrap();
                    let mut new_config = surface_config.clone();
                    new_config.width = size.width.max(1);
                    new_config.height = size.height.max(1);
                    surface.configure(device, &new_config);
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            winit::event::WindowEvent::CloseRequested => {
                self.shutdown_coordinator();
                event_loop.exit();
            }
            winit::event::WindowEvent::RedrawRequested => {
                if let Err(e) = self.handle_redraw() {
                    log::error!("Redraw failed: {e}");
                }
                // Task 8: after `--screenshot` writes its PNG, exit when
                // `--exit` was also passed. The actual `event_loop.exit()`
                // has to happen here because `handle_redraw` doesn't have
                // access to the `ActiveEventLoop` borrow.
                if self.screenshot_taken && self.exit_after_screenshot {
                    self.shutdown_coordinator();
                    event_loop.exit();
                }
            }
            winit::event::WindowEvent::CursorMoved { .. }
            | winit::event::WindowEvent::MouseInput { .. }
            | winit::event::WindowEvent::MouseWheel { .. }
            | winit::event::WindowEvent::KeyboardInput { .. } => {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

impl Application {
    fn handle_redraw(&mut self) -> anyhow::Result<()> {
        // Split borrows from `self` so we can read all fields in parallel. Each
        // `as_ref().unwrap()` / `as_mut().unwrap()` borrows a different field,
        // which the borrow checker accepts as disjoint.
        let core = self.core.as_mut().unwrap();
        let state = self.state.as_mut().unwrap();
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let surface = self.surface.as_mut().unwrap();

        // Task 8: prime the screenshot settle timer. The 5s delay mirrors
        // akar's demo-rust example so the UI has time to populate search
        // results / animations before the capture. `start_time` is
        // initialized lazily on the first frame where `screenshot_path` is
        // set, then `screenshot_pending` becomes true exactly once.
        if self.screenshot_path.is_some() && self.start_time.is_none() {
            self.start_time = Some(Instant::now());
        }
        let screenshot_pending = self.screenshot_path.is_some()
            && !self.screenshot_taken
            && self
                .start_time
                .is_some_and(|t| t.elapsed() >= std::time::Duration::from_secs(5));

        // Build a fresh per-frame layout tree. The whole tree is rebuilt every
        // frame (immediate mode) — for Task 3 it contains the canvas root and
        // the zoom-indicator overlay (absolute-positioned, child of canvas so
        // taffy can resolve its `inset`). Tasks 4/6 add drawer / search
        // subtrees.
        let mut layout = akar_layout::Layout::new();
        let canvas_node = layout.new_leaf(akar_layout::Style {
            size: akar_layout::Size {
                width: akar_layout::Dimension::percent(1.0),
                height: akar_layout::Dimension::percent(1.0),
            },
            ..Default::default()
        });
        // Zoom indicator. Absolute-positioned at the bottom-right of the
        // canvas. Must be a child of canvas_node (not rootless) so taffy can
        // resolve its `inset` against the canvas's containing block — a
        // rootless absolute node reports location (0,0) (verified locally
        // against taffy 0.11). The 10/10 insets give a small right/bottom
        // margin.
        let indicator_node = layout.new_leaf(akar_layout::Style {
            position: akar_layout::Position::Absolute,
            inset: akar_layout::Rect {
                left: akar_layout::auto(),
                top: akar_layout::auto(),
                right: akar_layout::length(10.0_f32),
                bottom: akar_layout::length(10.0_f32),
            },
            size: akar_layout::Size {
                width: akar_layout::length(80.0_f32),
                height: akar_layout::length(20.0_f32),
            },
            ..Default::default()
        });
        layout.add_child(canvas_node, indicator_node);
        // `Layout::compute` requires a measure closure even for fixed-size nodes.
        // Pass a no-op that returns Size::ZERO — no node in Task 3 needs content
        // measuring because every size is fully specified in the Style.
        layout.compute(
            canvas_node,
            (Some(state.window_size.x), Some(state.window_size.y)),
            |_, _, _, _, _| akar_layout::Size::ZERO,
        );

        // Start the frame. From this point until `core.end_frame`, we can read
        // `core.input.chars` / `core.input.keys_pressed` — they get cleared at
        // `end_frame`. Width/height are cast to u32 per the akar API; scale
        // factor is the winit value (set in `resumed`).
        let w = state.window_size.x as u32;
        let h = state.window_size.y as u32;
        core.begin_frame(w, h, state.scale_factor);

        // Keyboard shortcut detection. This must run between `begin_frame` and
        // `end_frame` so the per-frame input is still populated. The handlers
        // mutate `state` (toggle search_active, clear results, etc.) and any
        // changes are picked up by the render functions below.
        //
        // Preserves the semantics of the deleted `src/input.rs` (pre-migration
        // lines 81-138):
        //   Cmd+Shift+K toggles code search (clears commit search when ON,
        //   clears code search when OFF).
        //   Cmd+K toggles commit search (clears code search when ON, clears
        //   commit search when OFF).
        //   Toggling either mode ON clears the other's query/results/containers.
        //   Toggling either mode OFF clears its own query/results/containers.
        //   Escape cascade: code → commits → deselect-all-cards.
        // Also clears `core.input.focused_id` on Escape so any focused text
        // input loses focus (matters once Task 6 wires the search box).
        //
        // Tab toggles the conversation surface (Task 5). The Tab char is
        // consumed so text_input doesn't insert it.
        let cmd_or_ctrl = state.cmd_or_ctrl;
        if core.input.chars.contains(&'\t') {
            core.input.chars.retain(|&c| c != '\t');
            state.conversation.visible = !state.conversation.visible;
        }
        if cmd_or_ctrl && (core.input.chars.contains(&'k') || core.input.chars.contains(&'K')) {
            // `akar-winit` correctly forwards the textual "k" from the
            // shortcut as input. It opens the search UI, not part of the
            // search query, so consume it before `text_input` reads the
            // frame's characters below.
            core.input.chars.retain(|&c| c != 'k' && c != 'K');
            // Cmd+K: toggle unified search (all sources).
            state.search_active = !state.search_active;
            state.search_just_opened = state.search_active;
            state.search_query.clear();
            state.search_edit_state = Default::default();
            state.cursor_timer = 0.0;
            state.cursor_visible = true;
            state.search_results.clear();
            state.code_search_results.clear();
            state.document_search_results.clear();
            state
                .containers
                .retain(|c| c.container_type != ContainerType::SearchResults);
            state
                .containers
                .retain(|c| c.container_type != ContainerType::CodeSearchResults);
            state
                .containers
                .retain(|c| c.container_type != ContainerType::DocumentSearchResults);
        }
        if core.input.keys_pressed.contains(&akar_core::Key::Escape) {
            if state.conversation.visible {
                // Close conversation panel first.
                state.conversation.visible = false;
            } else if state.search_active {
                state.search_active = false;
                state.search_just_opened = false;
                state.search_query.clear();
                state.search_edit_state = Default::default();
                state.search_results.clear();
                state.code_search_results.clear();
                state.document_search_results.clear();
                state
                    .containers
                    .retain(|c| c.container_type != ContainerType::SearchResults);
                state
                    .containers
                    .retain(|c| c.container_type != ContainerType::CodeSearchResults);
                state
                    .containers
                    .retain(|c| c.container_type != ContainerType::DocumentSearchResults);
            } else {
                // Deselect all (cascade terminator).
                state.selected_folder = None;
                for container in &mut state.containers {
                    for card in &mut container.cards {
                        card.is_selected = false;
                    }
                }
            }
            // Any focused text input loses focus on Escape. No-op for Task 2
            // (no components set focused_id yet) but Task 6's text_input will
            // honor this.
            core.input.focused_id = None;
        }

        // Render layers. Order: canvas → drawer → search (search is on top).

        // Cmd+Left-drag pan (Task 3). akar's `PanButton` enum is only
        // `Middle`/`Right` (akar-components/src/canvas.rs:9), so daftprompt's
        // existing Cmd+Left-drag-to-pan convention is not covered by
        // `canvas_begin`. Furthermore, `canvas_begin` resets
        // `CanvasState::is_panning` every frame the configured button isn't
        // pressed (canvas.rs:134), so reusing it for Cmd+Left would be
        // cleared immediately. We track `state.cmd_panning` separately and
        // mutate `state.canvas_state.pan` here so the frame's
        // `world_to_screen` transform from `canvas_begin` already reflects
        // the drag. This must run AFTER `core.begin_frame` (so per-frame
        // input is populated) and BEFORE `render_canvas` (so
        // `canvas_begin` sees the updated pan).
        let canvas_rect_for_pan = layout.rect(canvas_node);
        if state.cmd_or_ctrl
            && core.input.mouse_buttons_pressed[0]
            && core.input.is_hovering(canvas_rect_for_pan)
        {
            state.cmd_panning = true;
        }
        if !core.input.mouse_buttons[0] {
            state.cmd_panning = false;
        }
        if state.cmd_panning {
            let delta =
                (core.input.mouse_pos - core.input.mouse_pos_prev) / state.canvas_state.zoom;
            state.canvas_state.pan -= delta;
        }

        render_canvas(core, &mut layout, canvas_node, indicator_node, state);

        // Drawer animation (Task 4). Delta-time based: advances
        // `state.drawer_animation` toward 1.0 (open) or 0.0 (closed) at a
        // rate of 6.0/sec, so a full open/close takes ~1/6 s. Runs BEFORE
        // `render_drawer` so it sees the updated value when computing
        // `panel_width`. Also keeps requesting redraws while the animation
        // is in flight so the lerp continues frame-to-frame.
        let now = Instant::now();
        let dt = match self.last_frame {
            Some(prev) => now.duration_since(prev).as_secs_f32(),
            None => 0.0,
        };
        self.last_frame = Some(now);
        let target = if state.drawer_open { 1.0 } else { 0.0 };
        let anim_speed = 6.0_f32;
        if state.drawer_animation < target {
            state.drawer_animation = (state.drawer_animation + dt * anim_speed).min(target);
        } else if state.drawer_animation > target {
            state.drawer_animation = (state.drawer_animation - dt * anim_speed).max(target);
        }
        if (state.drawer_open && state.drawer_animation < 1.0)
            || (!state.drawer_open && state.drawer_animation > 0.0)
        {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }

        if state.drawer_open {
            render_drawer(core, &mut layout, state);
        }
        if state.search_active {
            render_search(core, &mut layout, state, dt);
        }
        if state.conversation.visible {
            render_conversation(core, &mut layout, state);
        }

        // Drain coordinator events and handle UI signals (Task 5).
        drain_coordinator_events(self.coordinator_evt.as_mut(), state);
        handle_conversation_signals(
            self.coordinator_cmd.as_ref(),
            state,
        );

        // Acquire the surface texture. If acquisition fails, skip the frame
        // and request another redraw — same as Task 1.
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Outdated => {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("Surface lost; skipping frame");
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(anyhow::anyhow!("Surface validation error"));
            }
        };

        // Task 8: request a screenshot before the pass. The capture
        // machinery (`screenshot_capture.requested = true`) is what makes
        // `core.capture_target_view(...)` return `Some(view)` below. The
        // call must precede the pass so the render pass targets the
        // capture texture, not the surface.
        if screenshot_pending {
            core.request_screenshot();
        }

        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // `render_view` is the capture view when a screenshot is pending,
        // otherwise the normal surface view. The pass writes to it
        // identically; only the destination texture differs.
        let render_view = if screenshot_pending {
            core.capture_target_view(device, w, h)
                .expect("capture_target_view returns Some when request_screenshot was called")
        } else {
            surface_view
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Main Encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Main Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &render_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // `end_frame` flushes the draw list to the GPU AND clears
            // `core.input` (chars, keys_pressed, scroll_delta, mouse press/release).
            // All input reads for this frame must have happened above.
            if let Err(e) = core.end_frame(device, queue, &mut pass) {
                log::error!("end_frame: {e}");
            }
        }

        if screenshot_pending {
            // `take_screenshot` consumes the encoder, blits the capture
            // view onto the surface view, copies the result into a staging
            // buffer, maps it, and submits to the queue. The returned
            // `CapturedFrame` has BGRA-swapped, unpadded RGBA8 ready for
            // PNG encoding. Pattern from
            // `akar/examples/demo-rust/src/main.rs:1566-1614`.
            let screenshot_path = self
                .screenshot_path
                .clone()
                .expect("screenshot_pending implies screenshot_path is Some");
            let captured = core.take_screenshot(device, queue, encoder, &frame);
            match captured {
                Ok(frame_data) => match std::fs::File::create(&screenshot_path) {
                    Ok(file) => {
                        let mut png_encoder =
                            png::Encoder::new(file, frame_data.width, frame_data.height);
                        png_encoder.set_color(png::ColorType::Rgba);
                        png_encoder.set_depth(png::BitDepth::Eight);
                        match png_encoder.write_header() {
                            Ok(mut writer) => {
                                if let Err(e) = writer.write_image_data(&frame_data.rgba) {
                                    log::error!("Failed to write PNG data: {e}");
                                } else {
                                    log::info!("Screenshot saved to {}", screenshot_path.display());
                                }
                            }
                            Err(e) => log::error!("Failed to write PNG header: {e}"),
                        }
                    }
                    Err(e) => log::error!(
                        "Failed to create screenshot file {}: {e}",
                        screenshot_path.display()
                    ),
                },
                Err(e) => log::error!("Screenshot failed: {e}"),
            }
            self.screenshot_taken = true;
            // `event_loop.exit()` is called from `window_event` after this
            // method returns (see the `RedrawRequested` arm) — that
            // handler owns the `ActiveEventLoop` reference.
        } else {
            queue.submit(std::iter::once(encoder.finish()));
        }
        frame.present();

        Ok(())
    }

    fn shutdown_coordinator(&mut self) {
        if let Some(cmd) = self.coordinator_cmd.take() {
            let _ = cmd.send(CoordinatorCommand::Shutdown);
        }
        self.coordinator_evt = None;
    }
}

/// Drain all pending coordinator events and update `state.conversation`.
fn drain_coordinator_events(
    evt_rx: Option<&mut mpsc::UnboundedReceiver<CoordinatorEvent>>,
    state: &mut AppState,
) {
    let Some(rx) = evt_rx else { return };
    loop {
        match rx.try_recv() {
            Ok(event) => apply_coordinator_event(event, state),
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                state.conversation.adapter_status.connected = false;
                break;
            }
        }
    }
}

/// Apply a single coordinator event to `state.conversation`.
fn apply_coordinator_event(event: CoordinatorEvent, state: &mut AppState) {
    match event {
        CoordinatorEvent::SessionCreated {
            adapter_name,
            adapter_version,
            protocol_version,
            ..
        } => {
            state.conversation.adapter_status.connected = true;
            state.conversation.adapter_status.name = adapter_name.clone();
            state.conversation.adapter_status.version = adapter_version.clone();
            state.conversation.adapter_status.protocol_version = protocol_version.clone();
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::SystemMessage,
                text: format!(
                    "Session created. Adapter: {} v{}, protocol {}.",
                    adapter_name, adapter_version, protocol_version
                ),
                timestamp: timestamp_now(),
            });
        }
        CoordinatorEvent::TurnStarted { turn_id } => {
            state.conversation.active_turn_id = Some(turn_id);
            // The user prompt entry is added here rather than at send time
            // so it arrives with the turn_id for correlation.
            let prompt_text = state.conversation.prompt_input.clone();
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::UserPrompt,
                text: prompt_text,
                timestamp: timestamp_now(),
            });
        }
        CoordinatorEvent::RetrievalCompleted {
            status,
            candidate_count,
            included_count,
            ..
        } => {
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::SystemMessage,
                text: format!(
                    "Retrieval {status}: {candidate_count} candidates, {included_count} included"
                ),
                timestamp: timestamp_now(),
            });
        }
        CoordinatorEvent::EnrichedPromptReady {
            original,
            enriched,
            ..
        } => {
            let included_excerpts = enriched
                .included
                .iter()
                .map(|excerpt| state::InspectorExcerpt {
                    rank: excerpt.rank,
                    source: excerpt.source.as_str().to_string(),
                    identifier: excerpt.identifier.clone(),
                    match_type: format!("{:?}", excerpt.match_type),
                    text: excerpt.text.clone(),
                    truncated: excerpt.truncated,
                    truncation_reason: excerpt.truncation_reason.map(truncation_reason_label),
                })
                .collect();
            let excluded_candidates = enriched
                .excluded
                .iter()
                .map(|candidate| state::InspectorExcluded {
                    rank: candidate.rank,
                    source: candidate.source.as_str().to_string(),
                    identifier: candidate.identifier.clone(),
                    reason: candidate.reason.as_label(),
                })
                .collect();
            let retrieval_status = match &enriched.retrieval_status {
                RetrievalStatus::Ok => "ok".to_string(),
                RetrievalStatus::Empty => "empty".to_string(),
                RetrievalStatus::Error { message } => format!("error: {message}"),
            };

            state.conversation.enrichment_inspector =
                Some(state::EnrichmentInspectorState {
                    original_prompt: original,
                    enriched_prompt: enriched.text.clone(),
                    included_excerpts,
                    excluded_candidates,
                    retrieval_status,
                    formatter_version: enriched.formatter_version,
                    total_char_budget: enriched.budget.total_char_budget,
                    per_excerpt_char_limit: enriched.budget.per_excerpt_char_limit,
                });
        }
        CoordinatorEvent::AcpSessionUpdate { update_json, .. } => {
            parse_session_update(&update_json, state);
        }
        CoordinatorEvent::PermissionRequired {
            request_id,
            tool_call_json,
            options_json,
        } => {
            let tool_call_desc = serde_json::from_str::<serde_json::Value>(&tool_call_json)
                .ok()
                .and_then(|v| {
                    v.get("toolCall")
                        .or_else(|| v.get("tool"))
                        .and_then(|t| t.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| truncate_str_static(&tool_call_json, 100));

            let options: Vec<state::PermissionDialogOption> =
                serde_json::from_str::<Vec<serde_json::Value>>(&options_json)
                    .unwrap_or_default()
                    .iter()
                    .map(|opt| state::PermissionDialogOption {
                        id: opt
                            .get("optionId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        label: opt
                            .get("label")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Allow")
                            .to_string(),
                        kind: opt
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("allow_once")
                            .to_string(),
                    })
                    .collect();

            state.conversation.permission_dialog = Some(state::PermissionDialogState {
                request_id,
                tool_call_description: tool_call_desc,
                options,
            });
        }
        CoordinatorEvent::TurnCompleted { stop_reason, .. } => {
            state.conversation.active_turn_id = None;
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::SystemMessage,
                text: format!("Turn completed: {stop_reason}"),
                timestamp: timestamp_now(),
            });
        }
        CoordinatorEvent::TurnFailed { error, .. } => {
            state.conversation.active_turn_id = None;
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::Error,
                text: error,
                timestamp: timestamp_now(),
            });
        }
        CoordinatorEvent::AdapterError { error, kind } => {
            state.conversation.adapter_status.connected = false;
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::Error,
                text: format!("[{}] {error}", kind.label()),
                timestamp: timestamp_now(),
            });
        }
    }
}

/// Parse a session update JSON and add the appropriate transcript entry.
fn parse_session_update(json: &str, state: &mut AppState) {
    // The coordinator forwards `serde_json::to_string(&SessionUpdate)`
    // (`daftprompt_acp::SessionUpdate`, an internally-tagged enum with
    // `#[serde(tag = "sessionUpdate", rename_all = "snake_case")]`), so
    // `"sessionUpdate"` is a *string* discriminator like
    // `"agent_message_chunk"`, not a nested object with its own `"type"`
    // field. Deserializing straight into the already-correct typed
    // representation (re-exported by daftprompt-acp so this crate does not
    // need its own ACP SDK dependency) avoids hand-rolling that shape again.
    let Ok(update) = serde_json::from_str::<daftprompt_acp::SessionUpdate>(json) else {
        state.conversation.entries.push(state::TranscriptEntry {
            kind: state::TranscriptEntryKind::Unknown("raw".to_string()),
            text: truncate_str_static(json, 200).to_string(),
            timestamp: timestamp_now(),
        });
        return;
    };

    match update {
        daftprompt_acp::SessionUpdate::UserMessageChunk(chunk) => {
            push_content_block_entry(state, state::TranscriptEntryKind::UserPrompt, &chunk.content);
        }
        daftprompt_acp::SessionUpdate::AgentMessageChunk(chunk) => {
            push_content_block_entry(state, state::TranscriptEntryKind::AgentText, &chunk.content);
        }
        daftprompt_acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            push_content_block_entry(state, state::TranscriptEntryKind::Thought, &chunk.content);
        }
        daftprompt_acp::SessionUpdate::ToolCall(tool_call) => {
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::ToolCall,
                text: format!("{} ({:?})", tool_call.title, tool_call.status),
                timestamp: timestamp_now(),
            });
        }
        daftprompt_acp::SessionUpdate::ToolCallUpdate(tool_call_update) => {
            let title = tool_call_update
                .fields
                .title
                .clone()
                .unwrap_or_else(|| tool_call_update.tool_call_id.to_string());
            let status = tool_call_update
                .fields
                .status
                .map(|s| format!("{s:?}"))
                .unwrap_or_else(|| "updated".to_string());
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::ToolCallUpdate,
                text: format!("{title}: {status}"),
                timestamp: timestamp_now(),
            });
        }
        daftprompt_acp::SessionUpdate::Plan(plan) => {
            let summary = if plan.entries.is_empty() {
                "(empty plan)".to_string()
            } else {
                plan.entries
                    .iter()
                    .map(|entry| format!("[{:?}] {}", entry.status, entry.content))
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::Plan,
                text: summary,
                timestamp: timestamp_now(),
            });
        }
        other => {
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::Unknown(format!("{other:?}")),
                text: truncate_str_static(json, 200).to_string(),
                timestamp: timestamp_now(),
            });
        }
    }
}

/// Extracts the text of a streamed content chunk (only `ContentBlock::Text`
/// carries renderable text today; images/audio/resource links have no plain
/// text and are surfaced as a non-fatal `Unknown` entry instead of being
/// dropped, per Design Decision #3).
fn push_content_block_entry(
    state: &mut AppState,
    kind: state::TranscriptEntryKind,
    content: &daftprompt_acp::ContentBlock,
) {
    match content {
        daftprompt_acp::ContentBlock::Text(text_content) => {
            state.conversation.entries.push(state::TranscriptEntry {
                kind,
                text: text_content.text.clone(),
                timestamp: timestamp_now(),
            });
        }
        other => {
            state.conversation.entries.push(state::TranscriptEntry {
                kind: state::TranscriptEntryKind::Unknown(format!("{other:?}")),
                text: String::new(),
                timestamp: timestamp_now(),
            });
        }
    }
}

/// Handle UI signal flags set by `render_conversation`.
fn handle_conversation_signals(
    cmd_tx: Option<&mpsc::UnboundedSender<CoordinatorCommand>>,
    state: &mut AppState,
) {
    // Send prompt
    if state.conversation.prompt_send_requested {
        state.conversation.prompt_send_requested = false;
        let prompt = state.conversation.prompt_input.trim().to_string();
        if !prompt.is_empty() {
            if let Some(tx) = cmd_tx {
                let _ = tx.send(CoordinatorCommand::SubmitPrompt { original: prompt });
                // Clear the input after sending.
                state.conversation.prompt_input.clear();
                state.conversation.prompt_edit_state = Default::default();
            } else {
                state.conversation.entries.push(state::TranscriptEntry {
                    kind: state::TranscriptEntryKind::Error,
                    text: "No coordinator connected.".to_string(),
                    timestamp: timestamp_now(),
                });
            }
        }
    }

    // Cancel turn
    if state.conversation.cancel_requested {
        state.conversation.cancel_requested = false;
        if let Some(tx) = cmd_tx {
            let _ = tx.send(CoordinatorCommand::CancelTurn);
        }
    }

    // Permission response
    if let Some(option_id) = state.conversation.permission_response.take() {
        if let (Some(tx), Some(dialog)) = (cmd_tx, state.conversation.permission_dialog.take()) {
            let _ = tx.send(CoordinatorCommand::RespondPermission {
                request_id: dialog.request_id,
                outcome: PermissionOutcome::Selected(option_id),
            });
        }
    }

    // Permission cancel
    if state.conversation.permission_cancel_requested {
        state.conversation.permission_cancel_requested = false;
        if let (Some(tx), Some(dialog)) = (cmd_tx, state.conversation.permission_dialog.take()) {
            let _ = tx.send(CoordinatorCommand::RespondPermission {
                request_id: dialog.request_id,
                outcome: PermissionOutcome::Cancelled,
            });
        }
    }
}

fn timestamp_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

fn truncation_reason_label(reason: TruncationReason) -> String {
    match reason {
        TruncationReason::PerExcerptLimit => "per_excerpt_limit".to_string(),
        TruncationReason::TotalBudgetRemaining => "total_budget_remaining".to_string(),
    }
}

fn truncate_str_static(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Realistic wire payloads matching `SessionUpdate`'s actual internal
    // tagging (`#[serde(tag = "sessionUpdate", rename_all = "snake_case")]`
    // on `agent_client_protocol::schema::v1::SessionUpdate`): the
    // `sessionUpdate` key is the variant discriminator string itself, not a
    // nested object with its own `type`/`updateType` field. This is the bug
    // `parse_session_update` used to get wrong -- every one of these used to
    // fall through to the `Unknown` branch.

    #[test]
    fn parses_agent_message_chunk() {
        let json = r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hello there"}}"#;
        let mut state = AppState::new((800, 600));
        parse_session_update(json, &mut state);

        assert_eq!(state.conversation.entries.len(), 1);
        let entry = &state.conversation.entries[0];
        assert!(matches!(entry.kind, state::TranscriptEntryKind::AgentText));
        assert_eq!(entry.text, "Hello there");
    }

    #[test]
    fn parses_agent_thought_chunk() {
        let json = r#"{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"thinking..."}}"#;
        let mut state = AppState::new((800, 600));
        parse_session_update(json, &mut state);

        assert_eq!(state.conversation.entries.len(), 1);
        let entry = &state.conversation.entries[0];
        assert!(matches!(entry.kind, state::TranscriptEntryKind::Thought));
        assert_eq!(entry.text, "thinking...");
    }

    #[test]
    fn parses_tool_call() {
        let json = r#"{"sessionUpdate":"tool_call","toolCallId":"call_1","title":"Read file","status":"in_progress"}"#;
        let mut state = AppState::new((800, 600));
        parse_session_update(json, &mut state);

        assert_eq!(state.conversation.entries.len(), 1);
        let entry = &state.conversation.entries[0];
        assert!(matches!(entry.kind, state::TranscriptEntryKind::ToolCall));
        assert!(entry.text.contains("Read file"));
        assert!(entry.text.contains("InProgress"));
    }

    #[test]
    fn parses_tool_call_update() {
        let json = r#"{"sessionUpdate":"tool_call_update","toolCallId":"call_1","status":"completed","title":"Read file"}"#;
        let mut state = AppState::new((800, 600));
        parse_session_update(json, &mut state);

        assert_eq!(state.conversation.entries.len(), 1);
        let entry = &state.conversation.entries[0];
        assert!(matches!(entry.kind, state::TranscriptEntryKind::ToolCallUpdate));
        assert!(entry.text.contains("Read file"));
        assert!(entry.text.contains("Completed"));
    }

    #[test]
    fn parses_plan() {
        let json = r#"{"sessionUpdate":"plan","entries":[
            {"content":"Step 1","priority":"high","status":"pending"},
            {"content":"Step 2","priority":"medium","status":"in_progress"}
        ]}"#;
        let mut state = AppState::new((800, 600));
        parse_session_update(json, &mut state);

        assert_eq!(state.conversation.entries.len(), 1);
        let entry = &state.conversation.entries[0];
        assert!(matches!(entry.kind, state::TranscriptEntryKind::Plan));
        assert!(entry.text.contains("Step 1"));
        assert!(entry.text.contains("Step 2"));
    }

    #[test]
    fn malformed_json_becomes_unknown_not_a_panic() {
        let json = "not json at all";
        let mut state = AppState::new((800, 600));
        parse_session_update(json, &mut state);

        assert_eq!(state.conversation.entries.len(), 1);
        let entry = &state.conversation.entries[0];
        assert!(matches!(entry.kind, state::TranscriptEntryKind::Unknown(_)));
    }
}
