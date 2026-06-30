use glam::Vec2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SystemTheme {
    Light,
    Dark,
}

pub struct AppState {
    // Window state
    pub window_size: Vec2,
    pub scale_factor: f32,
    
    // Canvas state
    pub zoom: f32,
    pub pan_offset: Vec2,
    pub is_panning: bool,
    pub last_mouse_pos: Option<Vec2>,
    
    // UI state
    pub system_theme: SystemTheme,
    pub drawer_open: bool,
    pub selected_folder: Option<usize>,
    pub search_query: String,
    pub search_active: bool,
    
    // Data state
    pub folders: Vec<FolderData>,
    pub documents: Vec<DocumentData>,
    pub cards: Vec<CardData>,
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
            
            system_theme: SystemTheme::Dark,
            drawer_open: true,
            selected_folder: None,
            search_query: String::new(),
            search_active: false,
            
            folders: Self::create_sample_folders(),
            documents: Self::create_sample_documents(),
            cards: Self::create_sample_cards(),
        }
    }
    
    pub fn resize(&mut self, new_size: (u32, u32)) {
        self.window_size = Vec2::new(new_size.0 as f32, new_size.1 as f32);
    }
    
    pub fn screen_to_canvas(&self, screen_pos: Vec2) -> Vec2 {
        (screen_pos - self.window_size * 0.5) / self.zoom + self.pan_offset
    }
    
    pub fn canvas_to_screen(&self, canvas_pos: Vec2) -> Vec2 {
        (canvas_pos - self.pan_offset) * self.zoom + self.window_size * 0.5
    }
    
    pub fn zoom_at_point(&mut self, screen_pos: Vec2, zoom_delta: f32) {
        let canvas_pos = self.screen_to_canvas(screen_pos);
        let new_zoom = (self.zoom * zoom_delta).clamp(0.1, 5.0);
        let zoom_ratio = new_zoom / self.zoom;
        
        self.pan_offset = canvas_pos - (canvas_pos - self.pan_offset) * zoom_ratio;
        self.zoom = new_zoom;
    }
    
    pub fn start_panning(&mut self, screen_pos: Vec2) {
        self.is_panning = true;
        self.last_mouse_pos = Some(screen_pos);
    }
    
    pub fn update_panning(&mut self, screen_pos: Vec2) {
        if let Some(last_pos) = self.last_mouse_pos {
            let delta = (screen_pos - last_pos) / self.zoom;
            self.pan_offset -= delta;
            self.last_mouse_pos = Some(screen_pos);
        }
    }
    
    pub fn stop_panning(&mut self) {
        self.is_panning = false;
        self.last_mouse_pos = None;
    }
    
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
    
    fn create_sample_documents() -> Vec<DocumentData> {
        vec![
            DocumentData {
                title: "README.md".to_string(),
                content: "# Project Title\n\nThis is a sample README file for the project.\n\n## Features\n- Feature 1\n- Feature 2\n- Feature 3\n\n## Installation\n```bash\nnpm install\n```".to_string(),
                file_type: IconType::Markdown,
                folder_id: 0,
            },
            DocumentData {
                title: "main.rs".to_string(),
                content: "fn main() {\n    println!(\"Hello, world!\");\n    \n    let x = 42;\n    let y = x * 2;\n    \n    println!(\"Result: {}\", y);\n}".to_string(),
                file_type: IconType::Code,
                folder_id: 0,
            },
            DocumentData {
                title: "config.json".to_string(),
                content: "{\n  \"name\": \"text-explorer\",\n  \"version\": \"1.0.0\",\n  \"description\": \"A text repository explorer\",\n  \"main\": \"index.js\"\n}".to_string(),
                file_type: IconType::Document,
                folder_id: 0,
            },
            DocumentData {
                title: "meeting-notes.md".to_string(),
                content: "# Meeting Notes - 2024-01-15\n\n## Attendees\n- Alice\n- Bob\n- Charlie\n\n## Agenda\n1. Project status update\n2. Timeline review\n3. Action items".to_string(),
                file_type: IconType::Markdown,
                folder_id: 1,
            },
            DocumentData {
                title: "todo.txt".to_string(),
                content: "TODO List:\n- [ ] Complete UI prototype\n- [ ] Add zoom functionality\n- [ ] Implement search\n- [x] Set up project structure\n- [x] Add dependencies".to_string(),
                file_type: IconType::Document,
                folder_id: 1,
            },
            DocumentData {
                title: "algorithms.rs".to_string(),
                content: "fn binary_search(arr: &[i32], target: i32) -> Option<usize> {\n    let mut left = 0;\n    let mut right = arr.len();\n    \n    while left < right {\n        let mid = left + (right - left) / 2;\n        if arr[mid] == target {\n            return Some(mid);\n        } else if arr[mid] < target {\n            left = mid + 1;\n        } else {\n            right = mid;\n        }\n    }\n    None\n}".to_string(),
                file_type: IconType::Code,
                folder_id: 3,
            },
            DocumentData {
                title: "design.md".to_string(),
                content: "# Design Document\n\n## Overview\nThis document outlines the design decisions for the text explorer.\n\n## Architecture\nThe application uses a canvas-based approach with infinite scrolling.\n\n## Components\n- Canvas with zoom/pan\n- Left drawer\n- Document cards\n- Search box".to_string(),
                file_type: IconType::Markdown,
                folder_id: 2,
            },
            DocumentData {
                title: "api-docs.txt".to_string(),
                content: "API Documentation\n\nGET /api/documents\n- Returns list of all documents\n- Query params: search, folder_id\n\nPOST /api/documents\n- Create new document\n- Body: { title, content, folder_id }\n\nGET /api/folders\n- Returns list of all folders".to_string(),
                file_type: IconType::Document,
                folder_id: 1,
            },
        ]
    }
    
    fn create_sample_cards() -> Vec<CardData> {
        let documents = Self::create_sample_documents();
        let mut cards = Vec::new();
        
        for (i, _doc) in documents.iter().enumerate() {
            let row = (i / 3) as f32;
            let col = (i % 3) as f32;
            
            cards.push(CardData {
                id: i,
                position: Vec2::new(
                    100.0 + col * 350.0,
                    100.0 + row * 250.0,
                ),
                size: Vec2::new(300.0, 200.0),
                document_id: i,
                is_selected: false,
                is_hovered: false,
            });
        }
        
        cards
    }
}
