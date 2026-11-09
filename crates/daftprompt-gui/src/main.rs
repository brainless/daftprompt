//! Native akar app for daftprompt.
//!
//! Two columns: a left composer with a multiline textarea anchored at the
//! bottom and a Parse button, and a wider source view that renders the API's
//! payload as labelled span blocks with the unlabelled text between them.
//!
//! HTTP runs on a background thread and reports through a channel. While a
//! request is in flight the event loop wakes on a short deadline to poll that
//! channel; rendering never blocks. This crate talks only to `daftprompt-types`
//! and the documented `POST /parse` endpoint; it never links the parser. At
//! startup it may run the compiled `daftprompt-api` binary as a child process
//! (see `spawn`), stopped when the app quits or the window closes.

mod api;
mod briefing;
mod briefing_ui;
mod briefing_view;
mod clarify;
mod clipboard;
mod fonts;
mod projects;
mod render;
mod spawn;
mod theme;
mod view;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use akar_components::{
    AkarTheme, ButtonStyle, ButtonVariant, FontWeight, HeadingLevel, TextEditState, TextStyle,
    akar_button_styled, akar_container, akar_heading, akar_paragraph, akar_textarea_styled,
};
use akar_core::{AkarCore, InputState, Key};
use akar_layout::{
    Dimension, Display, FlexDirection, Layout, NodeId, PageConfig, Rect, Size, Style, length,
};
use akar_winit::{ActivationProgress, StartupActivation, process_window_event};
use api::{ApiError, ParseClient, build_parse_request};
use briefing::{BriefingMessage, BriefingState, Details, Reply, Request, Section, Tab};
use briefing_ui::BlockLayout;
use briefing_view::{Action, Block as ViewBlock};
use clarify::{Branches, build_branch_view, panel_blocks};
use clipboard::{ClipboardRequest, SystemClipboard};
use daftprompt_types::{ClarifyRequest, ClarifyResult, ParseResult};
use projects::{
    DropAction, MenuAction, NoticeKind, ProjectMessage, Projects, Stamp, classify_drop,
};
use render::{
    ContentLayout, PanelLayout, build_panel, draw_content, draw_drop_frame, draw_notice,
    draw_panel_content, draw_project_menu,
};
use theme::DAFTPROMPT_THEME;
use view::{Block, ParseView, Phase, build_blocks, result_summary, truncate_for_display};
use wgpu::{
    CompositeAlphaMode, CurrentSurfaceTexture, InstanceDescriptor, PresentMode, TextureUsages,
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowAttributes},
};

const SIDEBAR_WIDTH: f32 = 340.0;
const PICKER_HEIGHT: f32 = 36.0;
const TAB_WIDTH: f32 = 104.0;
const TAB_HEIGHT: f32 = 32.0;
const FOOTER_HEIGHT: f32 = 44.0;
/// Padding inside the details card.
const DETAILS_PADDING: f32 = 16.0;
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const CURSOR_BLINK: Duration = Duration::from_millis(500);
/// The API child is spawned without a readiness wait, so the first project
/// fetch retries refused connections for up to about ten seconds.
const STARTUP_LIST_ATTEMPTS: u32 = 40;
const STARTUP_LIST_RETRY: Duration = Duration::from_millis(250);
const API_CHECK_INTERVAL: Duration = Duration::from_secs(1);

fn main() {
    let api = match ParseClient::from_env() {
        Ok(api) => api,
        Err(error) => {
            eprintln!("daftprompt-gui: {error}");
            std::process::exit(2);
        }
    };
    println!(
        "daftprompt-gui: POST {} on a background thread (set DAFTPROMPT_API_URL to change it)",
        api.url()
    );

    // Starting the child is quick (no readiness wait); a request made before
    // the API is up simply reports the connection error.
    let (api_process, api_note) = match spawn::start_from_env() {
        spawn::Startup::NotManaged => (None, None),
        spawn::Startup::Spawned(process) => (Some(process), None),
        spawn::Startup::Failed(note) => {
            eprintln!("daftprompt-gui: {note}");
            (None, Some(note))
        }
    };

    let event_loop = EventLoop::new().expect("could not create the event loop");
    let mut app = App {
        state: None,
        activation: StartupActivation::for_unbundled_launch(),
        api,
        api_process,
        api_note,
    };
    let result = event_loop.run_app(&mut app);
    // Drop explicitly so the child is stopped even on the error path, where
    // `process::exit` would skip destructors.
    drop(app);
    if let Err(error) = result {
        eprintln!("daftprompt-gui: the event loop stopped: {error}");
        std::process::exit(1);
    }
}

struct WorkerMessage {
    submission: u64,
    /// The selection the request was made for.
    stamp: Option<Stamp>,
    outcome: Result<ParseResult, ApiError>,
}

struct ClarifyMessage {
    display_id: u32,
    token: u64,
    /// The selection the request was made for.
    stamp: Option<Stamp>,
    outcome: Result<ClarifyResult, ApiError>,
}

struct App {
    state: Option<GuiState>,
    activation: StartupActivation,
    api: ParseClient,
    /// The API child started by this app, killed and waited on when dropped.
    api_process: Option<spawn::ApiProcess>,
    /// A spawn failure or early child exit to show in the UI.
    api_note: Option<String>,
}

/// A project selection that needs the API.
enum SelectJob {
    /// Open a registered project.
    Open(String),
    /// Register a folder (idempotent), then open the resulting project.
    Register(String),
}

/// `(briefing revision, rounded width)` a cached block layout was built for.
type BlockKey = (u64, i32);

struct GuiState {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    core: AkarCore,
    layout: Layout,
    page: akar_layout::PageLayout,

    project_picker: NodeId,
    left_title: NodeId,
    left_hint: NodeId,
    input_node: NodeId,
    parse_node: NodeId,
    left_footer: NodeId,
    tab_project: NodeId,
    tab_parse: NodeId,
    main_title: NodeId,
    main_meta: NodeId,
    main_body: NodeId,
    main_row: NodeId,
    footer: NodeId,
    panel_col: NodeId,
    panel_title: NodeId,
    panel_close: NodeId,
    panel_body: NodeId,
    panel_input: NodeId,
    panel_send: NodeId,

    /// The briefing of the current project, the active tab and the open
    /// command details.
    briefing: BriefingState,
    briefing_tx: Sender<BriefingMessage>,
    briefing_rx: Receiver<BriefingMessage>,
    /// Cached layouts of the Project tab and of the details area, each with
    /// the `(briefing revision, width)` it was built for, and their scroll.
    project_layout: Option<(BlockKey, BlockLayout)>,
    project_scroll: f32,
    details_layout: Option<(BlockKey, BlockLayout)>,
    details_scroll: f32,

    /// The selected project and the registry list; every new request reads
    /// its project id from here.
    projects: Projects,
    /// The picker list is open and replaces the sidebar body.
    project_menu_open: bool,
    /// "Choose folder…" was picked; the dialog opens after the frame.
    want_folder_dialog: bool,
    /// The native folder dialog is showing.
    folder_dialog_open: bool,
    /// Files are being dragged over the window.
    drag_hover: bool,
    /// A click closed the list from outside; its release must not also click
    /// whatever lies beneath.
    swallow_release: bool,
    project_tx: Sender<ProjectMessage>,
    project_rx: Receiver<ProjectMessage>,

    text: String,
    edit_state: TextEditState,
    input_scroll: f32,
    /// Created on first use; only touched on the main thread.
    clipboard: SystemClipboard,
    /// Clipboard work requested by the textarea this frame, served after the
    /// frame because akar clears one-frame input in `end_frame`.
    clipboard_request: ClipboardRequest,
    /// The textarea the pending clipboard request came from.
    clipboard_target: u64,
    /// The last clipboard failure, shown in the composer footer.
    clipboard_note: Option<String>,

    parse_view: ParseView,
    blocks: Vec<Block>,
    content: Option<ContentLayout>,
    content_width: f32,
    scroll_y: f32,

    /// Clarification branches of the current parse result, by span `#N`.
    branches: Branches,
    /// The span whose branch the panel shows.
    open_branch: Option<u32>,
    followup_text: String,
    followup_edit: TextEditState,
    followup_scroll: f32,
    panel: Option<PanelLayout>,
    /// `(span, branch revision, width)` the panel layout was built for.
    panel_key: Option<(u32, u64, i32)>,
    panel_scroll: f32,
    panel_open_applied: bool,

    api: ParseClient,
    api_note: Option<String>,
    tx: Sender<WorkerMessage>,
    rx: Receiver<WorkerMessage>,
    clarify_tx: Sender<ClarifyMessage>,
    clarify_rx: Receiver<ClarifyMessage>,
    started: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attrs = WindowAttributes::default()
            .with_title("daftprompt — Scribe preview")
            .with_inner_size(LogicalSize::new(1180.0, 760.0))
            .with_min_inner_size(LogicalSize::new(960.0, 520.0));
        let window = Arc::new(
            event_loop
                .create_window(window_attrs)
                .expect("create window"),
        );

        let instance = wgpu::Instance::new(InstanceDescriptor::new_with_display_handle(Box::new(
            event_loop.owned_display_handle(),
        )));
        let surface = instance
            .create_surface(window.clone())
            .expect("create wgpu surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("request a graphics adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("request a graphics device");

        let size = window.inner_size();
        let mut surface_config = surface
            .get_default_config(&adapter, size.width, size.height)
            .expect("surface configuration");
        surface_config.usage = TextureUsages::RENDER_ATTACHMENT;
        surface_config.present_mode = PresentMode::Fifo;
        surface_config.alpha_mode = CompositeAlphaMode::Opaque;
        let surface_format = surface_config.format;
        surface.configure(&device, &surface_config);

        let mut core = AkarCore::new(
            &device,
            &queue,
            surface_format,
            akar_core::TextPipelineConfig::default(),
        );
        fonts::Fonts::load(&mut core.text_pipeline).install();

        let (layout, page, nodes) = build_layout();
        let (tx, rx) = mpsc::channel();
        let (clarify_tx, clarify_rx) = mpsc::channel();
        let (project_tx, project_rx) = mpsc::channel();
        let (briefing_tx, briefing_rx) = mpsc::channel();

        self.state = Some(GuiState {
            window,
            device,
            queue,
            surface,
            surface_config,
            core,
            layout,
            page,
            project_picker: nodes.project_picker,
            left_title: nodes.left_title,
            left_hint: nodes.left_hint,
            input_node: nodes.input_node,
            parse_node: nodes.parse_node,
            left_footer: nodes.left_footer,
            tab_project: nodes.tab_project,
            tab_parse: nodes.tab_parse,
            main_title: nodes.main_title,
            main_meta: nodes.main_meta,
            main_body: nodes.main_body,
            main_row: nodes.main_row,
            footer: nodes.footer,
            panel_col: nodes.panel_col,
            panel_title: nodes.panel_title,
            panel_close: nodes.panel_close,
            panel_body: nodes.panel_body,
            panel_input: nodes.panel_input,
            panel_send: nodes.panel_send,
            briefing: BriefingState::default(),
            briefing_tx,
            briefing_rx,
            project_layout: None,
            project_scroll: 0.0,
            details_layout: None,
            details_scroll: 0.0,
            projects: Projects::default(),
            project_menu_open: false,
            want_folder_dialog: false,
            folder_dialog_open: false,
            drag_hover: false,
            swallow_release: false,
            project_tx,
            project_rx,
            text: String::new(),
            edit_state: TextEditState::default(),
            input_scroll: 0.0,
            clipboard: SystemClipboard::default(),
            clipboard_request: ClipboardRequest::default(),
            clipboard_target: 0,
            clipboard_note: None,
            parse_view: ParseView::default(),
            blocks: Vec::new(),
            content: None,
            content_width: 0.0,
            scroll_y: 0.0,
            branches: Branches::default(),
            open_branch: None,
            followup_text: String::new(),
            followup_edit: TextEditState::default(),
            followup_scroll: 0.0,
            panel: None,
            panel_key: None,
            panel_scroll: 0.0,
            panel_open_applied: false,
            api: self.api.clone(),
            api_note: self.api_note.clone(),
            tx,
            rx,
            clarify_tx,
            clarify_rx,
            started: Instant::now(),
        });

        // Preselect the most recently opened project.
        if let Some(state) = &mut self.state {
            spawn_list(state, true);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = &mut self.state else {
            return;
        };

        if let WindowEvent::Resized(new_size) = event {
            if new_size.width == 0 || new_size.height == 0 {
                return;
            }
            state.surface_config.width = new_size.width;
            state.surface_config.height = new_size.height;
            state
                .surface
                .configure(&state.device, &state.surface_config);
            akar_winit::set_presents_with_transaction(&state.surface, true);
            let window_id = state.window.id();
            self.window_event(event_loop, window_id, WindowEvent::RedrawRequested);
            if let Some(state) = &self.state {
                akar_winit::set_presents_with_transaction(&state.surface, false);
            }
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => render(state),
            WindowEvent::HoveredFile(_) => state.drag_hover = true,
            WindowEvent::HoveredFileCancelled | WindowEvent::DroppedFile(_) => {
                state.drag_hover = false;
            }
            _ => {}
        }

        // akar-winit forwards winit's key text; for Cmd/Ctrl shortcuts that
        // would type the shortcut letter next to the clipboard action.
        let chars_before = state.core.input.chars.len();
        process_window_event(&mut state.core.input, &event);
        if matches!(event, WindowEvent::KeyboardInput { .. }) {
            clipboard::discard_shortcut_chars(
                &mut state.core.input.chars,
                chars_before,
                state.core.input.modifiers,
            );
        }

        if !matches!(event, WindowEvent::RedrawRequested) {
            state.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let mut wake_at = None;

        if self.api_note.is_none()
            && let Some(process) = &mut self.api_process
            && let Some(note) = process.exit_note()
        {
            // The child is gone; keep the guard so it is still reaped on drop.
            self.api_note = Some(note);
            if let Some(state) = &mut self.state {
                state.api_note = self.api_note.clone();
                state.window.request_redraw();
            }
        }

        if let Some(state) = &self.state {
            if state.parse_view.is_loading()
                || state.branches.any_pending()
                || state.projects.is_busy()
                || state.briefing.is_loading()
            {
                wake_at = Some(now + POLL_INTERVAL);
                state.window.request_redraw();
            } else if textarea_focused(state) {
                wake_at = Some(now + CURSOR_BLINK);
                state.window.request_redraw();
            }

            if let ActivationProgress::Pending {
                wake_at: activation_wake,
            } = self.activation.poll(&state.window, now)
            {
                wake_at =
                    Some(wake_at.map_or(activation_wake, |current| current.min(activation_wake)));
            }
        }

        if self.api_process.is_some() && self.api_note.is_none() {
            // Notice an early child exit without busy-waiting.
            let check = now + API_CHECK_INTERVAL;
            wake_at = Some(wake_at.map_or(check, |current| current.min(check)));
        }

        match wake_at {
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // On macOS, Cmd+Q terminates the process from inside the event loop,
        // so `run_app` never returns and `main` never drops the app; stop the
        // child here instead.
        drop(self.api_process.take());
    }
}

fn render(state: &mut GuiState) {
    poll_worker(state);

    let size = state.window.inner_size();
    let scale = state.window.scale_factor() as f32;
    if size.width == 0 || size.height == 0 {
        return;
    }

    // Acquire before touching input: a failed acquisition is a clean no-op
    // and must not consume one-frame input before `end_frame` clears it.
    let output = match state.surface.get_current_texture() {
        CurrentSurfaceTexture::Success(texture) | CurrentSurfaceTexture::Suboptimal(texture) => {
            texture
        }
        CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
            state
                .surface
                .configure(&state.device, &state.surface_config);
            state.window.request_redraw();
            return;
        }
        CurrentSurfaceTexture::Timeout
        | CurrentSurfaceTexture::Occluded
        | CurrentSurfaceTexture::Validation => {
            state.window.request_redraw();
            return;
        }
    };

    let theme = &DAFTPROMPT_THEME;
    let now = Instant::now();
    state.core.begin_frame(size.width, size.height, scale);
    if state.swallow_release && state.core.input.mouse_buttons_released[0] {
        state.core.input.mouse_buttons_released[0] = false;
        state.swallow_release = false;
    }
    handle_drops(state);

    state.layout.compute(
        state.page.root,
        (
            Some(size.width as f32 / scale),
            Some(size.height as f32 / scale),
        ),
        |_, _, _, _, _| Size::ZERO,
    );

    draw_shell(state, theme);
    draw_sidebar(state, theme, now);
    ensure_content(state);
    draw_main(state, theme);
    if state.drag_hover {
        let (width, height) = (size.width as f32 / scale, size.height as f32 / scale);
        draw_drop_frame(&mut state.core, width, height, theme);
    }

    let mut encoder = state
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("daftprompt-gui frame"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let _ = state.core.end_frame(&state.device, &state.queue, &mut pass);
    }

    state.queue.submit(std::iter::once(encoder.finish()));
    output.present();

    serve_clipboard(state);
    if std::mem::take(&mut state.want_folder_dialog) {
        start_folder_dialog(state);
    }

    if state.parse_view.is_loading()
        || state.branches.any_pending()
        || state.projects.is_busy()
        || state.briefing.is_loading()
    {
        state.window.request_redraw();
    }
}

fn textarea_focused(state: &GuiState) -> bool {
    let focused = state.core.input.focused_id;
    focused == Some(state.layout.widget_id(state.input_node))
        || (state.open_branch.is_some()
            && focused == Some(state.layout.widget_id(state.panel_input)))
}

struct LayoutNodes {
    project_picker: NodeId,
    left_title: NodeId,
    left_hint: NodeId,
    input_node: NodeId,
    parse_node: NodeId,
    left_footer: NodeId,
    tab_project: NodeId,
    tab_parse: NodeId,
    main_title: NodeId,
    main_meta: NodeId,
    main_body: NodeId,
    main_row: NodeId,
    footer: NodeId,
    panel_col: NodeId,
    panel_title: NodeId,
    panel_close: NodeId,
    panel_body: NodeId,
    panel_input: NodeId,
    panel_send: NodeId,
}

fn build_layout() -> (Layout, akar_layout::PageLayout, LayoutNodes) {
    let mut layout = Layout::new();
    let page = layout.page(PageConfig {
        header_height: None,
        footer_height: None,
        sidebar_left_width: Some(SIDEBAR_WIDTH),
        sidebar_right_width: None,
    });

    let sidebar = page.sidebar_left.expect("left sidebar");
    layout.set_style(
        sidebar,
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            size: Size {
                width: length(SIDEBAR_WIDTH),
                height: Dimension::percent(1.0),
            },
            padding: Rect {
                top: length(20.0_f32),
                right: length(20.0_f32),
                bottom: length(20.0_f32),
                left: length(20.0_f32),
            },
            ..Default::default()
        },
    );

    let project_picker = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(PICKER_HEIGHT),
        },
        margin: Rect {
            top: length(0.0_f32),
            right: length(0.0_f32),
            bottom: length(14.0_f32),
            left: length(0.0_f32),
        },
        ..Default::default()
    });
    let left_title = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(32.0_f32),
        },
        ..Default::default()
    });
    let left_hint = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(112.0_f32),
        },
        ..Default::default()
    });
    let left_spacer = layout.new_leaf(Style {
        flex_grow: 1.0,
        flex_basis: Dimension::auto(),
        ..Default::default()
    });
    let input_node = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(210.0_f32),
        },
        ..Default::default()
    });
    let parse_node = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(42.0_f32),
        },
        margin: Rect {
            top: length(12.0_f32),
            right: length(0.0_f32),
            bottom: length(0.0_f32),
            left: length(0.0_f32),
        },
        ..Default::default()
    });
    let left_footer = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(52.0_f32),
        },
        margin: Rect {
            top: length(10.0_f32),
            right: length(0.0_f32),
            bottom: length(0.0_f32),
            left: length(0.0_f32),
        },
        ..Default::default()
    });
    layout.set_children(
        sidebar,
        &[
            project_picker,
            left_title,
            left_hint,
            left_spacer,
            input_node,
            parse_node,
            left_footer,
        ],
    );

    let main = page.main;
    layout.set_style(
        main,
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            size: Size {
                width: Dimension::auto(),
                height: Dimension::percent(1.0),
            },
            padding: Rect {
                top: length(24.0_f32),
                right: length(24.0_f32),
                bottom: length(16.0_f32),
                left: length(24.0_f32),
            },
            ..Default::default()
        },
    );

    let tab_project = layout.new_leaf(Style {
        flex_shrink: 0.0,
        size: Size {
            width: length(TAB_WIDTH),
            height: length(TAB_HEIGHT),
        },
        ..Default::default()
    });
    let tab_parse = layout.new_leaf(Style {
        flex_shrink: 0.0,
        size: Size {
            width: length(TAB_WIDTH),
            height: length(TAB_HEIGHT),
        },
        ..Default::default()
    });
    let tab_row = layout.new_with_children(
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            size: Size {
                width: Dimension::percent(1.0),
                height: length(TAB_HEIGHT),
            },
            gap: Size {
                width: length(8.0_f32),
                height: length(0.0_f32),
            },
            ..Default::default()
        },
        &[tab_project, tab_parse],
    );
    let main_title = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(30.0_f32),
        },
        margin: Rect {
            top: length(14.0_f32),
            right: length(0.0_f32),
            bottom: length(0.0_f32),
            left: length(0.0_f32),
        },
        ..Default::default()
    });
    let main_meta = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(24.0_f32),
        },
        margin: Rect {
            top: length(4.0_f32),
            right: length(0.0_f32),
            bottom: length(0.0_f32),
            left: length(0.0_f32),
        },
        ..Default::default()
    });
    let main_body = layout.new_leaf(Style {
        flex_grow: 1.0,
        flex_basis: Dimension::auto(),
        size: Size {
            width: Dimension::auto(),
            height: Dimension::percent(1.0),
        },
        ..Default::default()
    });

    // The clarification panel sits beside the source view. Closed, it is zero
    // wide and the source view keeps the whole row.
    let panel_title = layout.new_leaf(Style {
        flex_grow: 1.0,
        size: Size {
            width: Dimension::auto(),
            height: length(30.0_f32),
        },
        ..Default::default()
    });
    let panel_close = layout.new_leaf(Style {
        flex_shrink: 0.0,
        size: Size {
            width: length(72.0_f32),
            height: length(30.0_f32),
        },
        ..Default::default()
    });
    let panel_header = layout.new_with_children(
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            size: Size {
                width: Dimension::percent(1.0),
                height: length(30.0_f32),
            },
            ..Default::default()
        },
        &[panel_title, panel_close],
    );
    let panel_body = layout.new_leaf(Style {
        flex_grow: 1.0,
        flex_basis: Dimension::auto(),
        size: Size {
            width: Dimension::percent(1.0),
            height: Dimension::auto(),
        },
        margin: Rect {
            top: length(8.0_f32),
            right: length(0.0_f32),
            bottom: length(0.0_f32),
            left: length(0.0_f32),
        },
        ..Default::default()
    });
    let panel_input = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(96.0_f32),
        },
        margin: Rect {
            top: length(8.0_f32),
            right: length(0.0_f32),
            bottom: length(0.0_f32),
            left: length(0.0_f32),
        },
        ..Default::default()
    });
    let panel_send = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(38.0_f32),
        },
        margin: Rect {
            top: length(8.0_f32),
            right: length(0.0_f32),
            bottom: length(0.0_f32),
            left: length(0.0_f32),
        },
        ..Default::default()
    });
    let panel_col = layout.new_with_children(
        panel_style(false),
        &[panel_header, panel_body, panel_input, panel_send],
    );
    let main_row = layout.new_with_children(
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            flex_grow: 1.0,
            flex_basis: Dimension::auto(),
            size: Size {
                width: Dimension::percent(1.0),
                height: Dimension::auto(),
            },
            margin: Rect {
                top: length(12.0_f32),
                right: length(0.0_f32),
                bottom: length(0.0_f32),
                left: length(0.0_f32),
            },
            ..Default::default()
        },
        &[main_body, panel_col],
    );
    // The command footer is fixed at the bottom of the main column.
    let footer = layout.new_leaf(Style {
        flex_shrink: 0.0,
        size: Size {
            width: Dimension::percent(1.0),
            height: length(FOOTER_HEIGHT),
        },
        margin: Rect {
            top: length(8.0_f32),
            right: length(0.0_f32),
            bottom: length(0.0_f32),
            left: length(0.0_f32),
        },
        ..Default::default()
    });
    layout.set_children(main, &[tab_row, main_title, main_meta, main_row, footer]);

    (
        layout,
        page,
        LayoutNodes {
            project_picker,
            left_title,
            left_hint,
            input_node,
            parse_node,
            left_footer,
            tab_project,
            tab_parse,
            main_title,
            main_meta,
            main_body,
            main_row,
            footer,
            panel_col,
            panel_title,
            panel_close,
            panel_body,
            panel_input,
            panel_send,
        },
    )
}

/// The panel column: collapsed to nothing when closed, otherwise a share of
/// the row beside the source view.
fn panel_style(open: bool) -> Style {
    Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        flex_shrink: 0.0,
        size: Size {
            width: if open {
                Dimension::percent(0.44)
            } else {
                length(0.0_f32)
            },
            height: Dimension::percent(1.0),
        },
        margin: Rect {
            top: length(0.0_f32),
            right: length(0.0_f32),
            bottom: length(0.0_f32),
            left: length(if open { 16.0_f32 } else { 0.0_f32 }),
        },
        ..Default::default()
    }
}

fn draw_shell(state: &mut GuiState, theme: &AkarTheme) {
    akar_container(
        &mut state.core,
        &state.layout,
        state.page.root,
        &akar_components::BoxStyle::flat(theme.base_100),
    );
    if let Some(sidebar) = state.page.sidebar_left {
        akar_container(
            &mut state.core,
            &state.layout,
            sidebar,
            &akar_components::BoxStyle::flat(theme.base_200),
        );
    }
}

/// UI-font text style (Inter) at the given weight.
fn ui_style(weight: FontWeight) -> TextStyle {
    fonts::current().style(fonts::Role::Ui, weight)
}

/// Text-font style (Source Serif 4) for the textareas.
fn text_style() -> TextStyle {
    fonts::current().style(fonts::Role::Text, FontWeight::Normal)
}

fn draw_sidebar(state: &mut GuiState, theme: &AkarTheme, now: Instant) {
    if draw_project_picker(state, theme) {
        // The open list replaces the rest of the sidebar.
        return;
    }
    akar_heading(
        &mut state.core,
        &state.layout,
        state.left_title,
        "daftprompt",
        HeadingLevel::H3,
        Some(ui_style(FontWeight::Semibold)),
        theme,
    );

    // A project message takes the hint's place until the next project action.
    let (hint_text, hint_color) = match state.projects.notice() {
        Some(notice) => (
            notice.text.as_str(),
            match notice.kind {
                NoticeKind::Error => theme.error,
                NoticeKind::Info => theme.warning,
            },
        ),
        None => (
            "Paste chat, code, email, or document text. Scribe marks questions, local \
             answer links, statuses, and explicit @assignees. Unlabelled text stays \
             unlabelled.",
            theme.muted_content,
        ),
    };
    let hint_style = TextStyle {
        color: Some(hint_color),
        font_size: Some(theme.font_size_sm),
        line_height: Some(theme.font_size_sm * 1.5),
        ..ui_style(FontWeight::Normal)
    };
    akar_paragraph(
        &mut state.core,
        &state.layout,
        state.left_hint,
        hint_text,
        Some(hint_style),
        theme,
    );

    let input_id = state.layout.widget_id(state.input_node);
    let focused = state.core.input.focused_id == Some(input_id);
    let elapsed_ms = now.duration_since(state.started).as_millis();
    let cursor_visible = !focused || (elapsed_ms / CURSOR_BLINK.as_millis()).is_multiple_of(2);
    let response = akar_textarea_styled(
        &mut state.core,
        &state.layout,
        state.input_node,
        &mut state.text,
        &mut state.edit_state,
        &mut state.input_scroll,
        "Paste or type text here…",
        cursor_visible,
        Some(&text_style()),
        theme,
    );
    let request = ClipboardRequest::from_response(&response);
    if !request.is_empty() {
        state.clipboard_request = request;
        state.clipboard_target = input_id;
    }

    let mut button_style = ButtonStyle::empty();
    button_style.text_style = Some(ui_style(FontWeight::Medium));
    let busy = state.parse_view.is_loading();
    if busy {
        button_style.fill = Some(theme.base_300);
        button_style.hover_fill = Some(theme.base_300);
        button_style.pressed_fill = Some(theme.base_300);
        button_style.border_color = Some(theme.base_300);
        button_style.content_color = Some(theme.muted_content);
    }
    let button = akar_button_styled(
        &mut state.core,
        &state.layout,
        state.parse_node,
        if busy { "Parsing…" } else { "Parse text" },
        ButtonVariant::Solid,
        &button_style,
        theme,
    );

    let footer_style = TextStyle {
        color: Some(theme.muted_content),
        font_size: Some(12.0),
        line_height: Some(16.0),
        ..ui_style(FontWeight::Normal)
    };
    let mut footer = format!(
        "POST {}\nEach Parse replaces the previous blocks.",
        state.api.url()
    );
    if let Some(note) = &state.clipboard_note {
        footer.push_str(&format!("\n{note}"));
    }
    akar_paragraph(
        &mut state.core,
        &state.layout,
        state.left_footer,
        &footer,
        Some(footer_style),
        theme,
    );

    if button.clicked && !busy {
        submit(state);
    }
}

/// Draw the project button and, while open, its list. Returns whether the list
/// covered the sidebar body this frame, so the caller skips drawing beneath it.
fn draw_project_picker(state: &mut GuiState, theme: &AkarTheme) -> bool {
    let name = truncate_for_display(state.projects.label(), 30);
    let mut style = ButtonStyle::empty();
    style.text_style = Some(ui_style(FontWeight::Medium));
    style.border_color = Some(theme.base_300);
    style.hover_fill = Some(theme.primary);
    style.content_color = Some(if state.projects.current().is_some() {
        theme.base_content
    } else {
        theme.muted_content
    });
    let button = akar_button_styled(
        &mut state.core,
        &state.layout,
        state.project_picker,
        &format!("{name}  \u{25BE}"),
        ButtonVariant::Outline,
        &style,
        theme,
    );
    if button.clicked {
        state.project_menu_open = !state.project_menu_open;
        if state.project_menu_open {
            state.projects.dismiss_notice();
        }
        state.window.request_redraw();
    }
    if !state.project_menu_open {
        return false;
    }

    let picker = state.layout.rect(state.project_picker);
    let sidebar = state
        .page
        .sidebar_left
        .map_or(picker, |node| state.layout.rect(node));
    let top = picker[1] + picker[3] + 8.0;
    let area = [
        picker[0],
        top,
        picker[2],
        sidebar[1] + sidebar[3] - 20.0 - top,
    ];

    let outside = state.core.input.mouse_buttons_pressed[0]
        && !state.core.input.is_hovering(picker)
        && !state.core.input.is_hovering(area);
    if outside || state.core.input.keys_pressed.contains(&Key::Escape) {
        state.project_menu_open = false;
        state.swallow_release = outside;
        state.window.request_redraw();
        return false;
    }

    let entries = state.projects.entries();
    if let Some(index) = draw_project_menu(&mut state.core, area, &entries, theme) {
        state.project_menu_open = false;
        apply_menu_action(state, &entries[index].action);
        state.window.request_redraw();
    }
    true
}

fn apply_menu_action(state: &mut GuiState, action: &MenuAction) {
    match action {
        MenuAction::Clear => {
            state.projects.clear();
            state.briefing.clear();
        }
        MenuAction::Select(id) => spawn_select(state, SelectJob::Open(id.clone())),
        MenuAction::ChooseFolder => {
            state.projects.dismiss_notice();
            state.want_folder_dialog = true;
        }
    }
}

/// Everything dropped on the window this frame, in event order. winit 0.30
/// reports one `DroppedFile` per path and no pointer position, so akar queues
/// them as unpositioned window-level drops.
fn drain_dropped(input: &mut InputState) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::mem::take(&mut input.unpositioned_file_drops)
        .into_iter()
        .flatten()
        .collect();
    for event in std::mem::take(&mut input.file_drop_events) {
        paths.extend(event.paths);
    }
    paths
}

/// Register the first dropped folder; tell the user when only files arrived.
fn handle_drops(state: &mut GuiState) {
    let paths = drain_dropped(&mut state.core.input);
    match classify_drop(&paths, Path::is_dir) {
        DropAction::Ignore => {}
        DropAction::Rejected(message) => {
            state.projects.set_info(message);
            state.window.request_redraw();
        }
        DropAction::Register(path) => {
            state.project_menu_open = false;
            spawn_select(
                state,
                SelectJob::Register(path.to_string_lossy().into_owned()),
            );
        }
    }
}

/// Open the native folder dialog without blocking rendering.
///
/// On macOS `AsyncFileDialog` must be created on the main thread (this runs
/// from the event loop): it attaches an `NSOpenPanel` sheet to the window,
/// which the running `NSApplication` services. Only the returned future is
/// moved to a worker thread to wait for the answer, as rfd recommends. On
/// other platforms the same call works from any thread.
fn start_folder_dialog(state: &mut GuiState) {
    if state.folder_dialog_open {
        return;
    }
    state.folder_dialog_open = true;
    state.projects.job_started();
    let dialog = rfd::AsyncFileDialog::new()
        .set_title("Choose a project folder")
        .set_parent(state.window.as_ref())
        .pick_folder();
    let tx = state.project_tx.clone();
    thread::spawn(move || {
        let path = pollster::block_on(dialog).map(|handle| handle.path().to_path_buf());
        let _ = tx.send(ProjectMessage::Folder(path));
    });
}

/// Fetch the registry on a worker thread. At startup the API may still be
/// coming up, so refused connections are retried for a few seconds.
fn spawn_list(state: &mut GuiState, retry: bool) {
    state.projects.job_started();
    let api = state.api.clone();
    let tx = state.project_tx.clone();
    thread::spawn(move || {
        let attempts = if retry { STARTUP_LIST_ATTEMPTS } else { 1 };
        let mut outcome = api.list_projects();
        for _ in 1..attempts {
            if outcome
                .as_ref()
                .is_err_and(|error| error.kind == "connection")
            {
                thread::sleep(STARTUP_LIST_RETRY);
                outcome = api.list_projects();
            } else {
                break;
            }
        }
        let _ = tx.send(ProjectMessage::Listed(outcome));
    });
}

/// Select a project through the API; the reply makes it current.
fn spawn_select(state: &mut GuiState, job: SelectJob) {
    let token = state.projects.begin_select();
    state.projects.job_started();
    let api = state.api.clone();
    let tx = state.project_tx.clone();
    thread::spawn(move || {
        let outcome = match job {
            SelectJob::Open(id) => api.open_project(&id),
            SelectJob::Register(path) => api
                .create_project(&path)
                .and_then(|project| api.open_project(&project.id)),
        };
        let _ = tx.send(ProjectMessage::Selected { token, outcome });
    });
}

/// Serves the clipboard request recorded during the frame that just ended.
///
/// Paste is queued with `push_paste` for the textarea's widget id and a redraw
/// is requested, so the focused textarea applies it on the next frame. Doing
/// this after `end_frame` matters: it clears one-frame input, including pastes.
fn serve_clipboard(state: &mut GuiState) {
    let request = std::mem::take(&mut state.clipboard_request);
    if request.is_empty() {
        return;
    }
    let outcome = clipboard::service(&mut state.clipboard, request);
    if let Some(text) = outcome.paste {
        state.core.input.push_paste(state.clipboard_target, text);
    }
    state.clipboard_note = outcome.error;
    state.window.request_redraw();
}

fn draw_parse_tab(state: &mut GuiState, theme: &AkarTheme, show_body: bool) {
    let (title, meta, notice) = match state.parse_view.phase() {
        Phase::Idle => {
            let mut text = "Paste ordinary text in the left column — a chat reply, a plan, a code \
                 review, or an email quotation.\n\nScribe marks questions, answer links, \
                 and explicit @assignees. Unlabelled text stays as it is. Each Parse \
                 replaces the blocks shown here."
                .to_owned();
            let mut color = theme.muted_content;
            if let Some(note) = &state.api_note {
                text = format!("API problem: {note}\n\n{text}");
                color = theme.error;
            }
            (
                "Source blocks",
                "No result yet. Parsing starts only when you choose Parse text.".to_owned(),
                Some((text, color)),
            )
        }
        Phase::Loading => (
            "Parsing…",
            "Waiting for the API on a background thread. Rendering stays responsive.".to_owned(),
            Some((
                "Scribe is reading the text. This preview polls the result channel on a \
                 short wake deadline, so the window remains interactive."
                    .to_owned(),
                theme.muted_content,
            )),
        ),
        Phase::Success => {
            let summary = state
                .parse_view
                .result()
                .map(result_summary)
                .unwrap_or_else(|| "No result".to_owned());
            ("Source blocks", summary, None)
        }
        Phase::Error => {
            let error = state.parse_view.error();
            let mut detail = error
                .map(ToString::to_string)
                .unwrap_or_else(|| "The parse failed without a message.".to_owned());
            if error.is_some_and(|error| error.kind == "connection") {
                if let Some(note) = &state.api_note {
                    detail.push_str(&format!("\n\nAPI process: {note}"));
                }
                detail.push_str(
                    "\n\nThe GUI starts a built daftprompt-api itself (cargo build -p \
                     daftprompt-api); otherwise start it with cargo run -p daftprompt-api, \
                     or set DAFTPROMPT_API_URL to its /parse endpoint.",
                );
            }
            (
                "Parse failed",
                "The previous result was replaced by this error.".to_owned(),
                Some((detail, theme.error)),
            )
        }
    };

    akar_heading(
        &mut state.core,
        &state.layout,
        state.main_title,
        title,
        HeadingLevel::H4,
        Some(ui_style(FontWeight::Semibold)),
        theme,
    );

    let meta_style = TextStyle {
        color: Some(theme.muted_content),
        font_size: Some(theme.font_size_sm),
        line_height: Some(theme.font_size_sm * 1.5),
        ..ui_style(FontWeight::Normal)
    };
    akar_paragraph(
        &mut state.core,
        &state.layout,
        state.main_meta,
        &meta,
        Some(meta_style),
        theme,
    );

    let body_rect = state.layout.rect(state.main_body);
    let mut clicked_span = None;
    if !show_body {
        // The details area covers the body while open.
    } else if state.parse_view.phase() == Phase::Success {
        if let Some(content) = &mut state.content {
            let marks = state.branches.marks();
            clicked_span = draw_content(
                &mut state.core,
                content,
                body_rect,
                &mut state.scroll_y,
                theme,
                &marks,
                state.open_branch,
            );
        } else {
            draw_notice(
                &mut state.core,
                &state.layout,
                state.main_body,
                "No text blocks to show. The payload was empty after trimming.",
                theme.muted_content,
                theme,
            );
        }
    } else if let Some((text, color)) = notice {
        draw_notice(
            &mut state.core,
            &state.layout,
            state.main_body,
            &text,
            color,
            theme,
        );
    }

    if let Some(display_id) = clicked_span {
        open_branch(state, display_id);
    }
}

/// The main column: tab bar, the active tab, the details area when open, the
/// clarify panel (Parse tab only), and the command footer.
fn draw_main(state: &mut GuiState, theme: &AkarTheme) {
    draw_tabs(state, theme);
    close_details_on_outside_input(state);
    let details_open = state.briefing.details().is_some();
    match state.briefing.tab() {
        Tab::Parse => draw_parse_tab(state, theme, !details_open),
        Tab::Project => draw_project_header(state, theme),
    }
    if details_open {
        draw_details(state, theme);
    } else if state.briefing.tab() == Tab::Project {
        draw_project_body(state, theme);
    }
    draw_clarify_panel(state, theme);
    draw_command_footer(state, theme);
}

fn draw_tabs(state: &mut GuiState, theme: &AkarTheme) {
    for (node, tab, label) in [
        (state.tab_project, Tab::Project, "Project"),
        (state.tab_parse, Tab::Parse, "Parse"),
    ] {
        let active = state.briefing.tab() == tab;
        let mut style = ButtonStyle::empty();
        style.text_style = Some(ui_style(FontWeight::Medium));
        if active {
            style.fill = Some(theme.base_300);
            style.hover_fill = Some(theme.base_300);
            style.pressed_fill = Some(theme.base_300);
            style.border_color = Some(theme.primary);
        } else {
            style.border_color = Some(theme.base_300);
            style.content_color = Some(theme.muted_content);
        }
        let button = akar_button_styled(
            &mut state.core,
            &state.layout,
            node,
            label,
            ButtonVariant::Outline,
            &style,
            theme,
        );
        if button.clicked && !active {
            state.briefing.set_tab(tab);
            state.window.request_redraw();
        } else if button.clicked {
            state.briefing.close_details();
        }
    }
}

/// Escape, or a press outside the details area, the footer and the tab bar,
/// closes the details. The press's release is swallowed so it does not also
/// click whatever lies beneath.
fn close_details_on_outside_input(state: &mut GuiState) {
    if state.briefing.details().is_none() {
        return;
    }
    let input = &state.core.input;
    let inside = [
        state.layout.rect(state.main_row),
        state.layout.rect(state.footer),
        state.layout.rect(state.tab_project),
        state.layout.rect(state.tab_parse),
    ]
    .iter()
    .any(|rect| input.is_hovering(*rect));
    let escape = input.keys_pressed.contains(&Key::Escape);
    let outside = input.mouse_buttons_pressed[0] && !inside;
    if escape || outside {
        state.briefing.close_details();
        state.swallow_release |= outside;
        state.window.request_redraw();
    }
}

fn draw_project_header(state: &mut GuiState, theme: &AkarTheme) {
    let selected_root = state.projects.current_path_id().and_then(|id| {
        state
            .projects
            .current_paths()
            .iter()
            .find(|path| path.id == id)
            .map(|path| path.root_path.as_str())
    });
    let (title, meta) = match state.projects.current() {
        Some(project) => (
            truncate_for_display(&project.name, 48),
            projects::shorten_path(selected_root.unwrap_or(&project.root_path), 90),
        ),
        None => ("Project".to_owned(), "No project selected.".to_owned()),
    };
    akar_heading(
        &mut state.core,
        &state.layout,
        state.main_title,
        &title,
        HeadingLevel::H4,
        Some(ui_style(FontWeight::Semibold)),
        theme,
    );
    let meta_style = TextStyle {
        color: Some(theme.muted_content),
        font_size: Some(theme.font_size_sm),
        line_height: Some(theme.font_size_sm * 1.5),
        ..ui_style(FontWeight::Normal)
    };
    // With several registered paths the meta line becomes the path selector;
    // registering paths stays API-only.
    let chips = state.projects.path_chips();
    if chips.is_empty() {
        akar_paragraph(
            &mut state.core,
            &state.layout,
            state.main_meta,
            &meta,
            Some(meta_style),
            theme,
        );
    } else {
        let rect = state.layout.rect(state.main_meta);
        if let Some(index) = briefing_ui::draw_path_selector(&mut state.core, rect, &chips, theme)
            && state.projects.select_path(&chips[index].path_id)
        {
            state.window.request_redraw();
        }
    }
}

/// Rebuild a cached block layout when its key changed.
fn ensure_blocks(
    slot: &mut Option<(BlockKey, BlockLayout)>,
    key: BlockKey,
    width: f32,
    blocks: impl FnOnce() -> Vec<ViewBlock>,
    theme: &AkarTheme,
) {
    if slot.as_ref().is_none_or(|(have, _)| *have != key) {
        *slot = Some((key, briefing_ui::build_layout(&blocks(), width, theme)));
    }
}

fn draw_project_body(state: &mut GuiState, theme: &AkarTheme) {
    let paths = state.projects.current_paths();
    let Some(briefing) = state.briefing.briefing() else {
        draw_notice(
            &mut state.core,
            &state.layout,
            state.main_body,
            "Choose a project in the left column, or drop a folder on the window. daftprompt \
             then summarizes its recent work and layout, and lists the commands it supports.",
            theme.muted_content,
            theme,
        );
        return;
    };
    let rect = state.layout.rect(state.main_body);
    if rect[2] <= 8.0 || rect[3] <= 0.0 {
        return;
    }
    let key = (state.briefing.revision(), rect[2].round() as i32);
    ensure_blocks(
        &mut state.project_layout,
        key,
        rect[2],
        || briefing_view::project_blocks(briefing, paths),
        theme,
    );
    if let Some((_, layout)) = &state.project_layout {
        let action = briefing_ui::draw_blocks(
            &mut state.core,
            layout,
            rect,
            &mut state.project_scroll,
            theme,
        );
        if let Some(action) = action {
            handle_action(state, action);
        }
    }
}

/// The details area replaces the body of the main column while open. akar
/// paints all text above all ordinary quads, so a floating popover would show
/// the summary or the source blocks through its card; here nothing else draws
/// in this rectangle, and the popover's own text sits on its card.
fn draw_details(state: &mut GuiState, theme: &AkarTheme) {
    let Some(details) = state.briefing.details().cloned() else {
        return;
    };
    let Some(blocks) = briefing_view::details_view(
        state.briefing.briefing(),
        &details,
        state.projects.current_paths(),
    ) else {
        // The command is gone (re-run or project change).
        state.briefing.close_details();
        state.window.request_redraw();
        return;
    };
    let card = state.layout.rect(state.main_row);
    let inner = [
        card[0] + DETAILS_PADDING,
        card[1] + DETAILS_PADDING - 2.0,
        (card[2] - 2.0 * DETAILS_PADDING).max(0.0),
        (card[3] - 2.0 * DETAILS_PADDING + 4.0).max(0.0),
    ];
    if inner[2] <= 8.0 || inner[3] <= 0.0 {
        return;
    }
    briefing_ui::draw_card(&mut state.core, card, theme);
    let key = (state.briefing.revision(), inner[2].round() as i32);
    ensure_blocks(&mut state.details_layout, key, inner[2], || blocks, theme);
    if let Some((_, layout)) = &state.details_layout {
        let action = briefing_ui::draw_blocks(
            &mut state.core,
            layout,
            inner,
            &mut state.details_scroll,
            theme,
        );
        if let Some(action) = action {
            handle_action(state, action);
        }
    }
}

fn draw_command_footer(state: &mut GuiState, theme: &AkarTheme) {
    let rect = state.layout.rect(state.footer);
    let status = briefing_view::footer_status(state.briefing.briefing());
    let action = briefing_ui::draw_footer(
        &mut state.core,
        rect,
        &status,
        state.briefing.details(),
        theme,
    );
    drop(status);
    if let Some(action) = action {
        handle_action(state, action);
    }
}

/// Apply a click from the Project tab, the details area or the footer.
fn handle_action(state: &mut GuiState, action: Action) {
    match action {
        Action::Rerun(section) => {
            if let Some(request) = state.briefing.rerun(section) {
                spawn_briefing(state, request);
            }
        }
        Action::OpenCommand(id) => toggle_details(state, Details::Command(id)),
        Action::OpenList => toggle_details(state, Details::List),
        Action::CloseDetails => state.briefing.close_details(),
    }
    state.window.request_redraw();
}

/// Open `details`, or close the area when it already shows exactly that.
fn toggle_details(state: &mut GuiState, details: Details) {
    if state.briefing.details() == Some(&details) {
        state.briefing.close_details();
    } else {
        state.briefing.open_details(details);
        state.details_scroll = 0.0;
    }
}

/// Send one briefing request on a worker thread; the render loop only polls
/// the channel. Commands are discovered by the API, never run.
fn spawn_briefing(state: &GuiState, request: Request) {
    let api = state.api.clone();
    let tx = state.briefing_tx.clone();
    thread::spawn(move || {
        let path = request.path_id.as_deref();
        let outcome = match request.section {
            Section::History => api
                .history(&request.project_id, path, request.refresh)
                .map(Reply::Summary),
            Section::Overview => api
                .overview(&request.project_id, path, request.refresh)
                .map(Reply::Summary),
            Section::Commands => api
                .commands(&request.project_id, path, request.refresh)
                .map(Reply::Commands),
        };
        let _ = tx.send(BriefingMessage { request, outcome });
    });
}

fn spawn_briefings(state: &mut GuiState, requests: Vec<Request>) {
    if requests.is_empty() {
        return;
    }
    state.project_scroll = 0.0;
    state.project_layout = None;
    for request in requests {
        spawn_briefing(state, request);
    }
}

/// Show `display_id`'s branch, keeping the draft of the one it replaces.
fn open_branch(state: &mut GuiState, display_id: u32) {
    if state.open_branch == Some(display_id) {
        return;
    }
    if let Some(previous) = state.open_branch {
        let draft = std::mem::take(&mut state.followup_text);
        state.branches.set_draft(previous, draft);
    }
    state.followup_text = state.branches.draft(display_id);
    state.followup_edit = TextEditState::default();
    state.followup_scroll = 0.0;
    state.open_branch = Some(display_id);
    state.panel_key = None;
}

fn close_branch(state: &mut GuiState) {
    if let Some(previous) = state.open_branch.take() {
        let draft = std::mem::take(&mut state.followup_text);
        state.branches.set_draft(previous, draft);
    }
    state.panel = None;
    state.panel_key = None;
}

/// A new parse (or a failed one) discards every branch and the panel.
fn reset_branches(state: &mut GuiState) {
    state.branches.clear();
    state.open_branch = None;
    state.followup_text.clear();
    state.followup_edit = TextEditState::default();
    state.panel = None;
    state.panel_key = None;
    state.panel_scroll = 0.0;
}

fn draw_clarify_panel(state: &mut GuiState, theme: &AkarTheme) {
    // The panel column collapses when no branch is open.
    // The panel shows only on the Parse tab, and not under the details area.
    let open = state.open_branch.is_some()
        && state.parse_view.phase() == Phase::Success
        && state.briefing.tab() == Tab::Parse
        && state.briefing.details().is_none();
    if open != state.panel_open_applied {
        state.layout.set_style(state.panel_col, panel_style(open));
        state.panel_open_applied = open;
        // The source view changes width; rebuild it on the next frame.
        state.content = None;
        state.content_width = 0.0;
        state.window.request_redraw();
        return;
    }
    let (true, Some(display_id)) = (open, state.open_branch) else {
        return;
    };

    akar_heading(
        &mut state.core,
        &state.layout,
        state.panel_title,
        &format!("Clarify #{display_id}"),
        HeadingLevel::H4,
        Some(ui_style(FontWeight::Semibold)),
        theme,
    );
    let close = akar_button_styled(
        &mut state.core,
        &state.layout,
        state.panel_close,
        "Close",
        ButtonVariant::Outline,
        &ButtonStyle {
            text_style: Some(ui_style(FontWeight::Medium)),
            ..ButtonStyle::empty()
        },
        theme,
    );
    if close.clicked {
        close_branch(state);
        return;
    }

    ensure_panel(state, display_id, theme);
    let body_rect = state.layout.rect(state.panel_body);
    if let Some(panel) = &mut state.panel {
        draw_panel_content(
            &mut state.core,
            panel,
            body_rect,
            &mut state.panel_scroll,
            theme,
        );
    }

    let pending = state
        .branches
        .get(display_id)
        .is_some_and(clarify::Branch::is_pending);
    let has_turns = state
        .branches
        .get(display_id)
        .is_some_and(|branch| !branch.turns().is_empty());
    let input_id = state.layout.widget_id(state.panel_input);
    let focused = state.core.input.focused_id == Some(input_id);
    let elapsed_ms = Instant::now().duration_since(state.started).as_millis();
    let cursor_visible = !focused || (elapsed_ms / CURSOR_BLINK.as_millis()).is_multiple_of(2);
    let response = akar_textarea_styled(
        &mut state.core,
        &state.layout,
        state.panel_input,
        &mut state.followup_text,
        &mut state.followup_edit,
        &mut state.followup_scroll,
        if has_turns {
            "Ask a follow-up…"
        } else {
            "Ask about this span…"
        },
        cursor_visible,
        Some(&text_style()),
        theme,
    );
    let request = ClipboardRequest::from_response(&response);
    if !request.is_empty() {
        state.clipboard_request = request;
        state.clipboard_target = input_id;
    }

    let mut button_style = ButtonStyle::empty();
    button_style.text_style = Some(ui_style(FontWeight::Medium));
    if pending {
        button_style.fill = Some(theme.base_300);
        button_style.hover_fill = Some(theme.base_300);
        button_style.pressed_fill = Some(theme.base_300);
        button_style.border_color = Some(theme.base_300);
        button_style.content_color = Some(theme.muted_content);
    }
    let send = akar_button_styled(
        &mut state.core,
        &state.layout,
        state.panel_send,
        if pending {
            "Explaining…"
        } else if has_turns {
            "Ask follow-up"
        } else {
            "Ask"
        },
        ButtonVariant::Solid,
        &button_style,
        theme,
    );
    if send.clicked && !pending {
        submit_clarify(state);
    }
}

/// Rebuild the panel layout when the branch, its content, or the width changed.
fn ensure_panel(state: &mut GuiState, display_id: u32, theme: &AkarTheme) {
    let width = state.layout.rect(state.panel_body)[2];
    let revision = state
        .branches
        .get(display_id)
        .map_or(0, clarify::Branch::revision);
    let key = (display_id, revision, width.round() as i32);
    if state.panel_key == Some(key) {
        return;
    }
    let Some(parse) = state.parse_view.result() else {
        return;
    };
    let branch = state.branches.get(display_id);
    let view = build_branch_view(
        display_id,
        branch.map(clarify::Branch::turns).unwrap_or(&[]),
        parse,
    );
    let blocks = panel_blocks(&view, branch);
    state.panel = build_panel(&blocks, width, theme);
    if state
        .panel_key
        .is_none_or(|(id, rev, _)| (id, rev) != (key.0, key.1))
    {
        // New content: show the latest turn.
        state.panel_scroll = f32::MAX;
    }
    state.panel_key = Some(key);
}

fn submit_clarify(state: &mut GuiState) {
    let Some(display_id) = state.open_branch else {
        return;
    };
    let Some(parse) = state.parse_view.result() else {
        return;
    };
    let question = state.followup_text.clone();
    // The selection is read now; a reply that arrives after it changed is
    // dropped.
    let stamp = state.projects.stamp();
    let Some((token, request)) = state.branches.begin(
        parse,
        display_id,
        &question,
        stamp.as_ref().map(|s| s.project_id.as_str()),
        stamp.as_ref().and_then(|s| s.path_id.as_deref()),
    ) else {
        return;
    };
    spawn_clarify(state, display_id, token, stamp, request);
}

/// Send the request on a worker thread; the render loop only polls the channel.
fn spawn_clarify(
    state: &GuiState,
    display_id: u32,
    token: u64,
    stamp: Option<Stamp>,
    request: ClarifyRequest,
) {
    let api = state.api.clone();
    let tx = state.clarify_tx.clone();
    thread::spawn(move || {
        let outcome = api.clarify(&request);
        let _ = tx.send(ClarifyMessage {
            display_id,
            token,
            stamp,
            outcome,
        });
    });
}

fn ensure_content(state: &mut GuiState) {
    if state.parse_view.phase() != Phase::Success {
        state.content = None;
        state.content_width = 0.0;
        return;
    }

    let width = state.layout.rect(state.main_body)[2];
    if width <= 8.0 {
        return;
    }
    if state.content.is_none() || (state.content_width - width).abs() > 0.5 {
        state.content = render::build_content(&state.blocks, width, &DAFTPROMPT_THEME);
        state.content_width = width;
    }
}

fn submit(state: &mut GuiState) {
    if state.parse_view.is_loading() {
        return;
    }
    state.briefing.parse_submitted();
    if state.text.trim().is_empty() {
        state
            .parse_view
            .fail_local(ApiError::local("Enter some text before parsing."));
        state.blocks.clear();
        state.content = None;
        state.content_width = 0.0;
        state.scroll_y = 0.0;
        reset_branches(state);
        return;
    }

    let Some(submission) = state.parse_view.begin() else {
        return;
    };
    state.blocks.clear();
    state.content = None;
    state.content_width = 0.0;
    state.scroll_y = 0.0;
    reset_branches(state);

    let stamp = state.projects.stamp();
    let request = build_parse_request(
        &state.text,
        stamp.as_ref().map(|s| s.project_id.as_str()),
        stamp.as_ref().and_then(|s| s.path_id.as_deref()),
    );
    let api = state.api.clone();
    let tx = state.tx.clone();
    thread::spawn(move || {
        let outcome = api.parse(&request);
        let _ = tx.send(WorkerMessage {
            submission,
            stamp,
            outcome,
        });
    });
}

fn poll_worker(state: &mut GuiState) {
    let mut refresh_projects = false;
    while let Ok(message) = state.project_rx.try_recv() {
        match message {
            ProjectMessage::Listed(outcome) => {
                state.projects.job_finished();
                state.projects.apply_list(outcome);
            }
            ProjectMessage::Selected { token, outcome } => {
                state.projects.job_finished();
                if state.projects.finish_select(token, outcome)
                    && let Some(stamp) = state.projects.stamp()
                {
                    // Choosing a project always requests its briefing again,
                    // even when it was already current. These are ordinary
                    // opens, so the API may reuse a verified stored response.
                    let requests = state.briefing.open(&stamp);
                    spawn_briefings(state, requests);
                }
            }
            ProjectMessage::Folder(path) => {
                state.projects.job_finished();
                state.folder_dialog_open = false;
                if let Some(path) = path {
                    spawn_select(
                        state,
                        SelectJob::Register(path.to_string_lossy().into_owned()),
                    );
                }
            }
        }
    }
    // Launch preselection, a vanished project and "No project" all arrive as a
    // change of the current project; follow it.
    let requests = state.briefing.sync(state.projects.stamp().as_ref());
    spawn_briefings(state, requests);
    while let Ok(message) = state.briefing_rx.try_recv() {
        state.briefing.apply(message);
    }
    while let Ok(message) = state.rx.try_recv() {
        if message.stamp != state.projects.stamp() {
            // Made for another project or path: never show it.
            if state.parse_view.cancel(message.submission) {
                state.window.request_redraw();
            }
            continue;
        }
        refresh_projects |= message
            .outcome
            .as_ref()
            .err()
            .is_some_and(Projects::error_means_stale);
        if state.parse_view.finish(message.submission, message.outcome) {
            let blocks = state
                .parse_view
                .result()
                .map(build_blocks)
                .unwrap_or_default();
            state.blocks = blocks;
            state.content = None;
            state.content_width = 0.0;
            state.scroll_y = 0.0;
        }
    }
    while let Ok(message) = state.clarify_rx.try_recv() {
        let ClarifyMessage {
            display_id,
            token,
            stamp,
            outcome,
        } = message;
        if stamp != state.projects.stamp() {
            // Made for another project or path: never show it.
            state.branches.cancel(display_id, token);
            continue;
        }
        refresh_projects |= outcome
            .as_ref()
            .err()
            .is_some_and(Projects::error_means_stale);
        if state.branches.finish(display_id, token, outcome)
            && state.open_branch == Some(display_id)
            && let Some(branch) = state.branches.get(display_id)
            && branch.error().is_none()
            && branch.turns().last().map(|turn| turn.question.as_str())
                == Some(state.followup_text.as_str())
        {
            // The question was answered; clear the input for the next one.
            state.followup_text.clear();
            state.followup_edit = TextEditState::default();
        }
    }
    if refresh_projects {
        // The error itself is already shown; refresh so the list is current.
        spawn_list(state, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_columns_put_the_composer_below_a_flexible_spacer() {
        let (mut layout, page, nodes) = build_layout();
        layout.compute(page.root, (Some(1180.0), Some(760.0)), |_, _, _, _, _| {
            Size::ZERO
        });

        let sidebar = layout.rect(page.sidebar_left.expect("left column"));
        let main = layout.rect(page.main);
        assert_eq!(sidebar[2], SIDEBAR_WIDTH);
        assert!(
            main[2] > sidebar[2],
            "the parsed-text column should be wider"
        );

        let input = layout.rect(nodes.input_node);
        let button = layout.rect(nodes.parse_node);
        assert!(
            button[1] >= input[1] + input[3],
            "Parse sits below the textarea"
        );
        assert!(input[1] > 300.0, "textarea is anchored near the bottom");
        assert!(button[1] + button[3] <= 760.0, "button stays on screen");

        let body = layout.rect(nodes.main_body);
        assert!(body[2] > 400.0 && body[3] > 500.0);
    }

    #[test]
    fn the_clarify_panel_opens_beside_the_source_view() {
        let (mut layout, page, nodes) = build_layout();
        layout.compute(page.root, (Some(1180.0), Some(760.0)), |_, _, _, _, _| {
            Size::ZERO
        });
        let closed_body = layout.rect(nodes.main_body);
        assert_eq!(layout.rect(nodes.panel_col)[2], 0.0);

        layout.set_style(nodes.panel_col, panel_style(true));
        layout.compute(page.root, (Some(1180.0), Some(760.0)), |_, _, _, _, _| {
            Size::ZERO
        });
        let body = layout.rect(nodes.main_body);
        let panel = layout.rect(nodes.panel_col);
        assert!(panel[2] > 250.0, "the panel is readable: {panel:?}");
        assert!(body[2] > 350.0 && body[2] < closed_body[2]);
        assert!(panel[0] >= body[0] + body[2], "the panel sits to the right");
        let input = layout.rect(nodes.panel_input);
        let send = layout.rect(nodes.panel_send);
        assert!(send[1] >= input[1] + input[3]);
        assert!(send[1] + send[3] <= 760.0);
        assert!(layout.rect(nodes.panel_body)[3] > 150.0);
    }

    #[test]
    fn a_narrow_window_keeps_both_columns_positive() {
        let (mut layout, page, nodes) = build_layout();
        layout.compute(page.root, (Some(760.0), Some(520.0)), |_, _, _, _, _| {
            Size::ZERO
        });
        assert!(layout.rect(nodes.main_body)[2] > 0.0);
        assert!(layout.rect(nodes.input_node)[2] > 0.0);
    }
}
