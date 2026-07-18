use crate::ui::container::Container;
use akar_components::CanvasState;
use glam::Vec2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SystemTheme {
    #[allow(dead_code)]
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchMode {
    Commits,
    Code,
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
    pub search_mode: SearchMode,
    pub search_just_opened: bool,

    // Text-input cursor (Task 6: read/written by search box)
    pub cursor_pos: usize,
    pub cursor_visible: bool,
    pub cursor_timer: f32,

    // Data state
    pub folders: Vec<FolderData>,
    #[allow(dead_code)]
    pub containers: Vec<Container>,

    // Indexer state
    pub indexer: Option<sugacode_indexer::Indexer>,
    pub search_results: Vec<sugacode_indexer::SearchResult>,

    // Code search state
    pub code_search_active: bool,
    pub code_search_just_opened: bool,
    pub code_search_query: String,
    pub code_search_results: Vec<sugacode_indexer::CodeSearchResult>,
    #[allow(dead_code)]
    pub code_indexing_in_progress: bool,
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
            search_mode: SearchMode::Commits,
            search_just_opened: false,

            cursor_pos: 0,
            cursor_visible: true,
            cursor_timer: 0.0,

            folders: Self::create_sample_folders(),
            containers: Vec::new(),

            indexer: None,
            search_results: Vec::new(),

            code_search_active: false,
            code_search_just_opened: false,
            code_search_query: String::new(),
            code_search_results: Vec::new(),
            code_indexing_in_progress: false,
        }
    }

    #[allow(dead_code)]
    pub fn resize(&mut self, new_size: (u32, u32)) {
        self.window_size = Vec2::new(new_size.0 as f32, new_size.1 as f32);
    }

    #[allow(dead_code)]
    fn create_sample_folders() -> Vec<FolderData> {
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
