mod state;
mod ui;
mod git_log;

use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use state::AppState;
use sugacode_indexer::{Indexer, IndexerConfig, CommitData, SymbolKind};
use ui::container::{Container, ContainerType};
use ui::render::{render_canvas, render_drawer, render_search};

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

    #[arg(long)]
    index_code: bool,

    #[arg(long)]
    reindex_code: bool,

    #[arg(long)]
    search_code: Option<String>,

    /// Task 8: capture one frame to a PNG file after a 5s settle delay.
    /// Combine with `--exit` to terminate the GUI after the capture.
    #[arg(long)]
    screenshot: Option<PathBuf>,

    /// Task 8: exit the GUI after `--screenshot` writes its PNG. No-op
    /// without `--screenshot` (matches akar's demo behavior).
    #[arg(long)]
    exit: bool,
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

    if args.index_code || args.reindex_code {
        let mut indexer = Indexer::new(&args.repo, &indexer_config)?;
        let report = if args.reindex_code {
            indexer.reindex_code()?
        } else {
            indexer.index_code()?
        };
        println!("Code index: {} files scanned, {} changed, {} deleted, {} symbols indexed",
            report.files_scanned, report.files_changed, report.files_deleted, report.symbols_indexed);
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
            };
            println!("[{:.3}] {:<8} {}:{}       {} — {}",
                r.score, kind_abbr, r.file_path, r.line_start,
                r.identifier.split("::").last().unwrap_or(""),
                r.text.lines().next().unwrap_or(""));
        }
        return Ok(());
    }

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
                match indexer.index_code() {
                    Ok(report) => log::info!("Indexed {} code symbols from {} files", report.symbols_indexed, report.files_changed),
                    Err(e) => log::warn!("Failed to index code: {e}"),
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
            last_frame: None,
            screenshot_path: args.screenshot,
            exit_after_screenshot: args.exit,
            start_time: None,
            screenshot_taken: false,
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

        // wgpu setup (sugacode owns the window; akar owns the GPU pipeline)
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(
            event_loop.owned_display_handle(),
        )));
        let surface = instance.create_surface(window.clone()).expect("Failed to create surface");
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
        let core = akar_core::AkarCore::new(&device, &queue, surface_format);

        self.state = Some(state);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.core = Some(core);
        self.window = Some(window.clone());

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
                right: akar_layout::length(10.0),
                bottom: akar_layout::length(10.0),
            },
            size: akar_layout::Size {
                width: akar_layout::length(80.0),
                height: akar_layout::length(20.0),
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
        let cmd_or_ctrl = state.cmd_or_ctrl;
        let shift = state.shift_pressed;
        if cmd_or_ctrl
            && (core.input.chars.contains(&'k') || core.input.chars.contains(&'K'))
        {
            if shift {
                // Cmd+Shift+K: toggle code search.
                state.code_search_active = !state.code_search_active;
                state.code_search_just_opened = state.code_search_active;
                state.search_mode = state::SearchMode::Code;
                if state.code_search_active {
                    // Turning code search ON: clear commit search.
                    state.search_active = false;
                    state.search_query.clear();
                    state.search_results.clear();
                    state.containers
                        .retain(|c| c.container_type != ContainerType::SearchResults);
                }
                // Always clear code search's own state (covers both the OFF
                // toggle and the "fresh open" case where the query was left
                // over from a previous session).
                state.code_search_query.clear();
                state.code_search_results.clear();
                state.containers
                    .retain(|c| c.container_type != ContainerType::CodeSearchResults);
            } else {
                // Cmd+K: toggle commit search.
                state.search_active = !state.search_active;
                state.search_just_opened = state.search_active;
                state.search_mode = state::SearchMode::Commits;
                if state.search_active {
                    // Turning commit search ON: clear code search.
                    state.code_search_active = false;
                    state.code_search_query.clear();
                    state.code_search_results.clear();
                    state.containers
                        .retain(|c| c.container_type != ContainerType::CodeSearchResults);
                }
                state.search_query.clear();
                state.search_results.clear();
                state.containers
                    .retain(|c| c.container_type != ContainerType::SearchResults);
            }
        }
        if core.input.keys_pressed.contains(&akar_core::Key::Escape) {
            if state.code_search_active {
                state.code_search_active = false;
                state.code_search_just_opened = false;
                state.code_search_query.clear();
                state.code_search_results.clear();
                state.containers
                    .retain(|c| c.container_type != ContainerType::CodeSearchResults);
            } else if state.search_active {
                state.search_active = false;
                state.search_just_opened = false;
                state.search_query.clear();
                state.search_results.clear();
                state.containers
                    .retain(|c| c.container_type != ContainerType::SearchResults);
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
        // `Middle`/`Right` (akar-components/src/canvas.rs:9), so sugacode's
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
            let delta = (core.input.mouse_pos - core.input.mouse_pos_prev)
                / state.canvas_state.zoom;
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
        if state.search_active || state.code_search_active {
            render_search(core, &mut layout, state, dt);
        }

        // Acquire the surface texture. If acquisition fails, skip the frame
        // and request another redraw — same as Task 1.
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
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
                                    log::info!(
                                        "Screenshot saved to {}",
                                        screenshot_path.display()
                                    );
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
}
