use glam::Vec2;

use crate::git_log::CommitInfo;
use crate::state::{CardData, DocumentData};
use sugacode_indexer::{CodeSearchResult, SearchResult};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContainerType {
    DocumentGrid,
    GitLogColumn,
    SearchResults,
    CodeSearchResults,
}

pub struct Container {
    pub id: usize,
    pub position: Vec2,
    pub size: Vec2,
    pub content_height: f32,
    pub scroll_offset: f32,
    pub container_type: ContainerType,
    pub cards: Vec<CardData>,
    pub documents: Vec<DocumentData>,
}

impl Container {
    pub fn new_document_grid(
        id: usize,
        position: Vec2,
        size: Vec2,
        cards: Vec<CardData>,
        documents: Vec<DocumentData>,
    ) -> Self {
        let content_height = cards
            .iter()
            .map(|c| c.position.y - position.y + c.size.y)
            .fold(0.0f32, f32::max);

        Self {
            id,
            position,
            size,
            content_height: content_height.max(size.y),
            scroll_offset: 0.0,
            container_type: ContainerType::DocumentGrid,
            cards,
            documents,
        }
    }

    pub fn new_git_log(
        id: usize,
        position: Vec2,
        width: f32,
        viewport_height: f32,
        commits: Vec<CommitInfo>,
    ) -> Self {
        let card_min_height = 80.0;
        let card_max_height = 200.0;
        let card_padding = 8.0;
        let card_width = width - 16.0; // 8px padding each side

        let mut cards = Vec::new();
        let mut documents = Vec::new();
        let mut y_offset = 0.0;

        for (i, commit) in commits.iter().enumerate() {
            let height =
                calculate_card_height(commit, card_width, card_min_height, card_max_height);

            cards.push(CardData {
                id: i,
                position: Vec2::new(8.0, y_offset),
                size: Vec2::new(card_width, height),
                document_id: i,
                is_selected: false,
                is_hovered: false,
            });

            documents.push(DocumentData {
                title: commit.message_title.clone(),
                content: format!(
                    "{}\nAuthor: {}\nDate: {}",
                    commit.short_hash, commit.author_name, commit.time
                ),
                file_type: crate::state::IconType::Code,
                folder_id: 0,
            });

            y_offset += height + card_padding;
        }

        let content_height = y_offset.max(viewport_height);

        Self {
            id,
            position,
            size: Vec2::new(width, viewport_height),
            content_height,
            scroll_offset: 0.0,
            container_type: ContainerType::GitLogColumn,
            cards,
            documents,
        }
    }

    pub fn new_search_results(
        id: usize,
        position: Vec2,
        width: f32,
        viewport_height: f32,
        results: Vec<SearchResult>,
    ) -> Self {
        let card_min_height = 80.0;
        let card_max_height = 200.0;
        let card_padding = 8.0;
        let card_width = width - 16.0;

        let mut cards = Vec::new();
        let mut documents = Vec::new();
        let mut y_offset = 0.0;

        for (i, result) in results.iter().enumerate() {
            let title = result.text.lines().next().unwrap_or("").to_string();
            let author = result.author.as_deref().unwrap_or("");
            let content = format!("{}\nAuthor: {}\nDate: ", result.short_hash, author);

            let height = calculate_search_card_height(&title, card_width, card_min_height, card_max_height);

            cards.push(CardData {
                id: i,
                position: Vec2::new(8.0, y_offset),
                size: Vec2::new(card_width, height),
                document_id: i,
                is_selected: false,
                is_hovered: false,
            });

            documents.push(DocumentData {
                title,
                content,
                file_type: crate::state::IconType::Code,
                folder_id: 0,
            });

            y_offset += height + card_padding;
        }

        let content_height = y_offset.max(viewport_height);

        Self {
            id,
            position,
            size: Vec2::new(width, viewport_height),
            content_height,
            scroll_offset: 0.0,
            container_type: ContainerType::SearchResults,
            cards,
            documents,
        }
    }

    pub fn new_code_search_results(
        id: usize,
        position: Vec2,
        width: f32,
        viewport_height: f32,
        results: Vec<CodeSearchResult>,
    ) -> Self {
        let card_min_height = 80.0;
        let card_max_height = 200.0;
        let card_padding = 8.0;
        let card_width = width - 16.0;

        let mut cards = Vec::new();
        let mut documents = Vec::new();
        let mut y_offset = 0.0;

        for (i, result) in results.iter().enumerate() {
            let title = result
                .identifier
                .split("::")
                .last()
                .unwrap_or(&result.identifier)
                .to_string();
            let content = format!(
                "{}\n{}:{}",
                result.file_path, result.line_start, result.line_end
            );

            let height = calculate_search_card_height(
                &title,
                card_width,
                card_min_height,
                card_max_height,
            );

            cards.push(CardData {
                id: i,
                position: Vec2::new(8.0, y_offset),
                size: Vec2::new(card_width, height),
                document_id: i,
                is_selected: false,
                is_hovered: false,
            });

            documents.push(DocumentData {
                title,
                content,
                file_type: crate::state::IconType::Code,
                folder_id: 0,
            });

            y_offset += height + card_padding;
        }

        let content_height = y_offset.max(viewport_height);

        Self {
            id,
            position,
            size: Vec2::new(width, viewport_height),
            content_height,
            scroll_offset: 0.0,
            container_type: ContainerType::CodeSearchResults,
            cards,
            documents,
        }
    }

    pub fn scroll(&mut self, delta: f32) {
        let max_scroll = (self.content_height - self.size.y).max(0.0);
        self.scroll_offset = (self.scroll_offset + delta).clamp(0.0, max_scroll);
    }

    pub fn is_mouse_over(&self, mouse_pos: Vec2) -> bool {
        mouse_pos.x >= self.position.x
            && mouse_pos.x <= self.position.x + self.size.x
            && mouse_pos.y >= self.position.y
            && mouse_pos.y <= self.position.y + self.size.y
    }

    pub fn visible_cards(&self) -> impl Iterator<Item = (&CardData, &DocumentData, Vec2)> {
        let container_top = self.position.y;
        let container_bottom = self.position.y + self.size.y;

        self.cards.iter().zip(self.documents.iter()).filter_map(
            move |(card, doc)| {
                let card_abs_y = self.position.y + card.position.y - self.scroll_offset;

                // Cull cards outside visible area
                if card_abs_y + card.size.y < container_top || card_abs_y > container_bottom {
                    return None;
                }

                let abs_pos = Vec2::new(self.position.x + card.position.x, card_abs_y);
                Some((card, doc, abs_pos))
            },
        )
    }
}

fn calculate_card_height(
    commit: &CommitInfo,
    card_width: f32,
    min: f32,
    max: f32,
) -> f32 {
    // Header line: hash + author + date
    let header_height = 24.0;
    // Separator
    let separator_height = 16.0;
    // Message title (may wrap)
    let chars_per_line = ((card_width - 20.0) / 7.5).max(1.0); // approx chars at 12px font
    let message_lines = (commit.message_title.len() as f32 / chars_per_line).ceil().max(1.0);
    let message_height = message_lines * 18.0;
    // Padding
    let padding = 24.0;

    let total = header_height + separator_height + message_height + padding;
    total.clamp(min, max)
}

fn calculate_search_card_height(
    title: &str,
    card_width: f32,
    min: f32,
    max: f32,
) -> f32 {
    let header_height = 24.0;
    let separator_height = 16.0;
    let chars_per_line = ((card_width - 20.0) / 7.5).max(1.0);
    let message_lines = (title.len() as f32 / chars_per_line).ceil().max(1.0);
    let message_height = message_lines * 18.0;
    let padding = 24.0;

    let total = header_height + separator_height + message_height + padding;
    total.clamp(min, max)
}
