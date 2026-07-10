use glam::Vec2;
use crate::ui::container::Container;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SystemTheme {
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

    // Canvas state
    #[allow(dead_code)]
    pub zoom: f32,
    #[allow(dead_code)]
    pub pan_offset: Vec2,
    #[allow(dead_code)]
    pub is_panning: bool,
    #[allow(dead_code)]
    pub last_mouse_pos: Option<Vec2>,

    // Modifier state (akar_winit doesn't expose Cmd/Ctrl; we track it ourselves)
    pub cmd_or_ctrl: bool,
    pub shift_pressed: bool,

    // UI state
    pub system_theme: SystemTheme,
    pub drawer_open: bool,
    pub selected_folder: Option<usize>,
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
    pub code_indexing_in_progress: bool,
}

#[derive(Debug, Clone)]
pub struct FolderData {
    pub name: String,
    pub icon: IconType,
    pub path: String,
    pub is_git_repo: bool,
    pub document_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconType {
    Folder,
    GitRepo,
    Document,
    Code,
    Markdown,
    Search,
    Settings,
}

#[derive(Debug, Clone)]
pub struct DocumentData {
    pub title: String,
    pub content: String,
    pub file_type: IconType,
    pub folder_id: usize,
}

#[derive(Debug, Clone)]
pub struct CardData {
    pub id: usize,
    pub position: Vec2,
    pub size: Vec2,
    pub document_id: usize,
    pub is_selected: bool,
    pub is_hovered: bool,
}

impl AppState {
    pub fn new(window_size: (u32, u32)) -> Self {
        let window_size = Vec2::new(window_size.0 as f32, window_size.1 as f32);

        Self {
            window_size,
            scale_factor: 1.0,

            zoom: 1.0,
            pan_offset: Vec2::ZERO,
            is_panning: false,
            last_mouse_pos: None,

            cmd_or_ctrl: false,
            shift_pressed: false,

            system_theme: SystemTheme::Dark,
            drawer_open: true,
            selected_folder: None,
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
    pub fn screen_to_canvas(&self, screen_pos: Vec2) -> Vec2 {
        (screen_pos - self.window_size * 0.5) / self.zoom + self.pan_offset
    }

    #[allow(dead_code)]
    pub fn canvas_to_screen(&self, canvas_pos: Vec2) -> Vec2 {
        (canvas_pos - self.pan_offset) * self.zoom + self.window_size * 0.5
    }

    #[allow(dead_code)]
    pub fn zoom_at_point(&mut self, screen_pos: Vec2, zoom_delta: f32) {
        let canvas_pos = self.screen_to_canvas(screen_pos);
        let new_zoom = (self.zoom * zoom_delta).clamp(0.1, 5.0);
        let zoom_ratio = new_zoom / self.zoom;

        self.pan_offset = canvas_pos - (canvas_pos - self.pan_offset) * zoom_ratio;
        self.zoom = new_zoom;
    }

    #[allow(dead_code)]
    pub fn start_panning(&mut self, screen_pos: Vec2) {
        self.is_panning = true;
        self.last_mouse_pos = Some(screen_pos);
    }

    #[allow(dead_code)]
    pub fn update_panning(&mut self, screen_pos: Vec2) {
        if let Some(last_pos) = self.last_mouse_pos {
            let delta = (screen_pos - last_pos) / self.zoom;
            self.pan_offset -= delta;
            self.last_mouse_pos = Some(screen_pos);
        }
    }

    #[allow(dead_code)]
    pub fn stop_panning(&mut self) {
        self.is_panning = false;
        self.last_mouse_pos = None;
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
