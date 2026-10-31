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
//! (see `spawn`), stopped when the window closes.

mod api;
mod clipboard;
mod render;
mod spawn;
mod view;

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use akar_components::{
    AKAR_THEME_DARK, AkarTheme, ButtonStyle, ButtonVariant, HeadingLevel, TextEditState, TextStyle,
    akar_button_styled, akar_container, akar_heading, akar_paragraph, akar_textarea,
};
use akar_core::AkarCore;
use akar_layout::{
    Dimension, Display, FlexDirection, Layout, NodeId, PageConfig, Rect, Size, Style, length,
};
use akar_winit::{ActivationProgress, StartupActivation, process_window_event};
use api::{ApiError, ParseClient};
use clipboard::{ClipboardRequest, SystemClipboard};
use daftprompt_types::ParseResult;
use render::{ContentLayout, draw_content, draw_notice};
use view::{Block, ParseView, Phase, build_blocks, result_summary};
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
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const CURSOR_BLINK: Duration = Duration::from_millis(500);
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
    outcome: Result<ParseResult, ApiError>,
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

struct GuiState {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    core: AkarCore,
    layout: Layout,
    page: akar_layout::PageLayout,

    left_title: NodeId,
    left_hint: NodeId,
    input_node: NodeId,
    parse_node: NodeId,
    left_footer: NodeId,
    main_title: NodeId,
    main_meta: NodeId,
    main_body: NodeId,

    text: String,
    edit_state: TextEditState,
    input_scroll: f32,
    /// Created on first use; only touched on the main thread.
    clipboard: SystemClipboard,
    /// Clipboard work requested by the textarea this frame, served after the
    /// frame because akar clears one-frame input in `end_frame`.
    clipboard_request: ClipboardRequest,
    /// The last clipboard failure, shown in the composer footer.
    clipboard_note: Option<String>,

    parse_view: ParseView,
    blocks: Vec<Block>,
    content: Option<ContentLayout>,
    content_width: f32,
    scroll_y: f32,

    api: ParseClient,
    api_note: Option<String>,
    tx: Sender<WorkerMessage>,
    rx: Receiver<WorkerMessage>,
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
            .with_min_inner_size(LogicalSize::new(760.0, 520.0));
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

        let core = AkarCore::new(
            &device,
            &queue,
            surface_format,
            akar_core::TextPipelineConfig::default(),
        );

        let (layout, page, nodes) = build_layout();
        let (tx, rx) = mpsc::channel();

        self.state = Some(GuiState {
            window,
            device,
            queue,
            surface,
            surface_config,
            core,
            layout,
            page,
            left_title: nodes.left_title,
            left_hint: nodes.left_hint,
            input_node: nodes.input_node,
            parse_node: nodes.parse_node,
            left_footer: nodes.left_footer,
            main_title: nodes.main_title,
            main_meta: nodes.main_meta,
            main_body: nodes.main_body,
            text: String::new(),
            edit_state: TextEditState::default(),
            input_scroll: 0.0,
            clipboard: SystemClipboard::default(),
            clipboard_request: ClipboardRequest::default(),
            clipboard_note: None,
            parse_view: ParseView::default(),
            blocks: Vec::new(),
            content: None,
            content_width: 0.0,
            scroll_y: 0.0,
            api: self.api.clone(),
            api_note: self.api_note.clone(),
            tx,
            rx,
            started: Instant::now(),
        });
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
            if state.parse_view.is_loading() {
                wake_at = Some(now + POLL_INTERVAL);
                state.window.request_redraw();
            } else if state.core.input.focused_id == Some(state.layout.widget_id(state.input_node))
            {
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

    let theme = &AKAR_THEME_DARK;
    let now = Instant::now();
    state.core.begin_frame(size.width, size.height, scale);

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

    if state.parse_view.is_loading() {
        state.window.request_redraw();
    }
}

struct LayoutNodes {
    left_title: NodeId,
    left_hint: NodeId,
    input_node: NodeId,
    parse_node: NodeId,
    left_footer: NodeId,
    main_title: NodeId,
    main_meta: NodeId,
    main_body: NodeId,
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

    let main_title = layout.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(30.0_f32),
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
        margin: Rect {
            top: length(12.0_f32),
            right: length(0.0_f32),
            bottom: length(0.0_f32),
            left: length(0.0_f32),
        },
        ..Default::default()
    });
    layout.set_children(main, &[main_title, main_meta, main_body]);

    (
        layout,
        page,
        LayoutNodes {
            left_title,
            left_hint,
            input_node,
            parse_node,
            left_footer,
            main_title,
            main_meta,
            main_body,
        },
    )
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

fn draw_sidebar(state: &mut GuiState, theme: &AkarTheme, now: Instant) {
    akar_heading(
        &mut state.core,
        &state.layout,
        state.left_title,
        "daftprompt",
        HeadingLevel::H3,
        None,
        theme,
    );

    let hint_style = TextStyle {
        color: Some(theme.muted_content),
        font_size: Some(theme.font_size_sm),
        line_height: Some(theme.font_size_sm * 1.5),
        ..TextStyle::empty()
    };
    akar_paragraph(
        &mut state.core,
        &state.layout,
        state.left_hint,
        "Paste chat, code, email, or document text. Scribe marks questions, local \
         answer links, statuses, and explicit @assignees. Unlabelled text stays \
         unlabelled.",
        Some(hint_style),
        theme,
    );

    let input_id = state.layout.widget_id(state.input_node);
    let focused = state.core.input.focused_id == Some(input_id);
    let elapsed_ms = now.duration_since(state.started).as_millis();
    let cursor_visible = !focused || (elapsed_ms / CURSOR_BLINK.as_millis()).is_multiple_of(2);
    let response = akar_textarea(
        &mut state.core,
        &state.layout,
        state.input_node,
        &mut state.text,
        &mut state.edit_state,
        &mut state.input_scroll,
        "Paste or type text here…",
        cursor_visible,
        theme,
    );
    let request = ClipboardRequest::from_response(&response);
    if !request.is_empty() {
        state.clipboard_request = request;
    }

    let mut button_style = ButtonStyle::empty();
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
        ..TextStyle::empty()
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
        let target = state.layout.widget_id(state.input_node);
        state.core.input.push_paste(target, text);
    }
    state.clipboard_note = outcome.error;
    state.window.request_redraw();
}

fn draw_main(state: &mut GuiState, theme: &AkarTheme) {
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
        None,
        theme,
    );

    let meta_style = TextStyle {
        color: Some(theme.muted_content),
        font_size: Some(theme.font_size_sm),
        line_height: Some(theme.font_size_sm * 1.5),
        ..TextStyle::empty()
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
    if state.parse_view.phase() == Phase::Success {
        if let Some(content) = &mut state.content {
            draw_content(
                &mut state.core,
                content,
                body_rect,
                &mut state.scroll_y,
                theme,
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
        state.content = render::build_content(&state.blocks, width, &AKAR_THEME_DARK);
        state.content_width = width;
    }
}

fn submit(state: &mut GuiState) {
    if state.parse_view.is_loading() {
        return;
    }
    if state.text.trim().is_empty() {
        state
            .parse_view
            .fail_local(ApiError::local("Enter some text before parsing."));
        state.blocks.clear();
        state.content = None;
        state.content_width = 0.0;
        state.scroll_y = 0.0;
        return;
    }

    let Some(submission) = state.parse_view.begin() else {
        return;
    };
    state.blocks.clear();
    state.content = None;
    state.content_width = 0.0;
    state.scroll_y = 0.0;

    let text = state.text.clone();
    let api = state.api.clone();
    let tx = state.tx.clone();
    thread::spawn(move || {
        let outcome = api.parse(&text);
        let _ = tx.send(WorkerMessage {
            submission,
            outcome,
        });
    });
}

fn poll_worker(state: &mut GuiState) {
    while let Ok(message) = state.rx.try_recv() {
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
    fn a_narrow_window_keeps_both_columns_positive() {
        let (mut layout, page, nodes) = build_layout();
        layout.compute(page.root, (Some(760.0), Some(520.0)), |_, _, _, _, _| {
            Size::ZERO
        });
        assert!(layout.rect(nodes.main_body)[2] > 0.0);
        assert!(layout.rect(nodes.input_node)[2] > 0.0);
    }
}
