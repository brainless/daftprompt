use crate::ui::container::Container;
use akar_components::{CanvasState, TextEditState};
use daftprompt_acp::PermissionRequestId;
use glam::Vec2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SystemTheme {
    #[allow(dead_code)]
    Light,
    Dark,
}

pub struct AppState {
    // Window state
    pub window_size: Vec2,
    pub scale_factor: f32,

    // Canvas state (Task 3: replaced legacy fields with akar's CanvasState)
    pub canvas_state: CanvasState,
    // Tracks Cmd+Left-drag pan. akar's PanButton enum is only Middle/Right,
    // and canvas_begin resets CanvasState::is_panning every frame the
    // configured button isn't pressed, so we need a separate flag for the
    // manual Cmd+Left-pan flow (see main.rs::handle_redraw).
    pub cmd_panning: bool,

    // Modifier state (akar_winit doesn't expose Cmd/Ctrl; we track it ourselves)
    pub cmd_or_ctrl: bool,
    pub shift_pressed: bool,

    // UI state
    pub system_theme: SystemTheme,
    pub drawer_open: bool,
    // Animated drawer progress in [0.0, 1.0]. 0.0 = fully collapsed (60px),
    // 1.0 = fully expanded (250px). Updated by main.rs each frame from
    // `drawer_open` and a delta-time; render_drawer reads it to compute
    // `panel_width` and decide whether to show the folder names.
    pub drawer_animation: f32,
    pub selected_folder: Option<usize>,
    // Index of the folder row the mouse is currently hovering. Set by
    // render_drawer (Task 4) on each frame; consumers can read it for
    // tooltip / preview purposes later.
    pub hover_index: Option<usize>,
    pub search_query: String,
    pub search_active: bool,
    pub search_just_opened: bool,

    // Text selection/cursor state (read/written by the search box).
    pub search_edit_state: TextEditState,
    pub cursor_visible: bool,
    pub cursor_timer: f32,

    // Data state
    pub folders: Vec<FolderData>,
    #[allow(dead_code)]
    pub containers: Vec<Container>,

    // Indexer state
    pub indexer: Option<daftprompt_indexer::Indexer>,
    pub search_results: Vec<daftprompt_indexer::SearchResult>,
    pub code_search_results: Vec<daftprompt_indexer::CodeSearchResult>,
    pub document_search_results: Vec<daftprompt_indexer::DocumentSearchResult>,

    // Conversation state (ACP conversation surface, toggled via Tab)
    pub conversation: ConversationState,
}

#[derive(Debug, Clone)]
pub struct FolderData {
    pub name: String,
    pub icon: IconType,
    #[allow(dead_code)]
    pub path: String,
    #[allow(dead_code)]
    pub is_git_repo: bool,
    pub document_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconType {
    Folder,
    GitRepo,
    #[allow(dead_code)]
    Document,
    Code,
    Markdown,
    #[allow(dead_code)]
    Search,
    #[allow(dead_code)]
    Settings,
}

#[derive(Debug, Clone)]
pub struct DocumentData {
    pub title: String,
    pub content: String,
    #[allow(dead_code)]
    pub file_type: IconType,
    #[allow(dead_code)]
    pub folder_id: usize,
}

#[derive(Debug, Clone)]
pub struct CardData {
    pub document_id: usize,
    pub stable_key: u64,
    pub is_selected: bool,
}

impl AppState {
    pub fn new(window_size: (u32, u32)) -> Self {
        let window_size = Vec2::new(window_size.0 as f32, window_size.1 as f32);

        Self {
            window_size,
            scale_factor: 1.0,

            canvas_state: CanvasState::new(),
            cmd_panning: false,

            cmd_or_ctrl: false,
            shift_pressed: false,

            system_theme: SystemTheme::Dark,
            drawer_open: true,
            drawer_animation: 1.0,
            selected_folder: None,
            hover_index: None,
            search_query: String::new(),
            search_active: false,
            search_just_opened: false,

            search_edit_state: TextEditState::default(),
            cursor_visible: true,
            cursor_timer: 0.0,

            folders: Self::create_sample_folders(),
            containers: Vec::new(),

            indexer: None,
            search_results: Vec::new(),
            code_search_results: Vec::new(),
            document_search_results: Vec::new(),

            conversation: ConversationState::default(),
        }
    }

    #[allow(dead_code)]
    pub fn resize(&mut self, new_size: (u32, u32)) {
        self.window_size = Vec2::new(new_size.0 as f32, new_size.1 as f32);
    }

    #[allow(dead_code)]
    pub fn create_sample_folders() -> Vec<FolderData> {
        vec![
            FolderData {
                name: "My Project".to_string(),
                icon: IconType::GitRepo,
                path: "/path/to/project".to_string(),
                is_git_repo: true,
                document_count: 15,
            },
            FolderData {
                name: "Documents".to_string(),
                icon: IconType::Folder,
                path: "/path/to/docs".to_string(),
                is_git_repo: false,
                document_count: 23,
            },
            FolderData {
                name: "Notes".to_string(),
                icon: IconType::Markdown,
                path: "/path/to/notes".to_string(),
                is_git_repo: false,
                document_count: 8,
            },
            FolderData {
                name: "Code Snippets".to_string(),
                icon: IconType::Code,
                path: "/path/to/snippets".to_string(),
                is_git_repo: false,
                document_count: 12,
            },
        ]
    }
}

// ── Conversation state types (Task 5, Epic 014) ──

pub struct ConversationState {
    pub visible: bool,
    pub entries: Vec<TranscriptEntry>,
    pub prompt_input: String,
    pub prompt_edit_state: TextEditState,
    pub adapter_status: AdapterStatus,
    pub active_turn_id: Option<i64>,
    pub permission_dialog: Option<PermissionDialogState>,
    pub enrichment_inspector_visible: bool,
    pub enrichment_inspector: Option<EnrichmentInspectorState>,
    pub transcript_scroll_y: f32,
    // Signal fields: set by render, consumed by main.rs each frame.
    pub prompt_send_requested: bool,
    pub cancel_requested: bool,
    pub permission_response: Option<String>,
    pub permission_cancel_requested: bool,
}

impl Default for ConversationState {
    fn default() -> Self {
        Self {
            visible: false,
            entries: Vec::new(),
            prompt_input: String::new(),
            prompt_edit_state: TextEditState::default(),
            adapter_status: AdapterStatus::default(),
            active_turn_id: None,
            permission_dialog: None,
            enrichment_inspector_visible: false,
            enrichment_inspector: None,
            transcript_scroll_y: 0.0,
            prompt_send_requested: false,
            cancel_requested: false,
            permission_response: None,
            permission_cancel_requested: false,
        }
    }
}

pub struct TranscriptEntry {
    pub kind: TranscriptEntryKind,
    pub text: String,
    #[allow(dead_code)]
    pub timestamp: String,
}

pub enum TranscriptEntryKind {
    UserPrompt,
    AgentText,
    ToolCall,
    ToolCallUpdate,
    Thought,
    Plan,
    SystemMessage,
    Error,
    Unknown(String),
}

pub struct AdapterStatus {
    pub connected: bool,
    pub name: String,
    pub version: String,
    #[allow(dead_code)]
    pub protocol_version: String,
}

impl Default for AdapterStatus {
    fn default() -> Self {
        Self {
            connected: false,
            name: String::new(),
            version: String::new(),
            protocol_version: String::new(),
        }
    }
}

pub struct PermissionDialogState {
    pub request_id: PermissionRequestId,
    pub tool_call_description: String,
    pub options: Vec<PermissionDialogOption>,
}

pub struct PermissionDialogOption {
    pub id: String,
    pub label: String,
    pub kind: String,
}

pub struct EnrichmentInspectorState {
    pub original_prompt: String,
    pub enriched_prompt: String,
    pub included_excerpts: Vec<InspectorExcerpt>,
    pub excluded_candidates: Vec<InspectorExcluded>,
    pub retrieval_status: String,
    #[allow(dead_code)]
    pub formatter_version: u32,
    pub total_char_budget: usize,
    #[allow(dead_code)]
    pub per_excerpt_char_limit: usize,
}

pub struct InspectorExcerpt {
    pub rank: usize,
    pub source: String,
    pub identifier: String,
    #[allow(dead_code)]
    pub match_type: String,
    #[allow(dead_code)]
    pub text: String,
    pub truncated: bool,
    #[allow(dead_code)]
    pub truncation_reason: Option<String>,
}

pub struct InspectorExcluded {
    pub rank: usize,
    pub source: String,
    pub identifier: String,
    pub reason: String,
}

