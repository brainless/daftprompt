use glam::Vec2;

use crate::git_log::CommitInfo;
use crate::state::{CardData, DocumentData};
use sugacode_indexer::{CodeSearchResult, SearchResult};

/// Fixed card height policy for Epic 017 compatibility.
///
/// `data_list_begin` requires a single uniform `item_height` for all cards
/// (akar deliberately deferred variable-height virtualization, see ADR-017).
/// 120px accommodates a header line (~24px), separator (~8px), gap (~4px),
/// ~3 lines of wrapped text at 18px each (~54px), and vertical padding
/// (~24px), totaling ~114px rounded to 120px for comfortable spacing.
/// Content exceeding this height is truncated by the label component.
pub const CARD_HEIGHT: f32 = 120.0;

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
        let card_padding = 8.0;
        let card_width = width - 16.0; // 8px padding each side

        let mut cards = Vec::new();
        let mut documents = Vec::new();
        let mut y_offset = 0.0;

        for (i, commit) in commits.iter().enumerate() {
            cards.push(CardData {
                id: i,
                position: Vec2::new(8.0, y_offset),
                size: Vec2::new(card_width, CARD_HEIGHT),
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

            y_offset += CARD_HEIGHT + card_padding;
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
        let card_padding = 8.0;
        let card_width = width - 16.0;

        let mut cards = Vec::new();
        let mut documents = Vec::new();
        let mut y_offset = 0.0;

        for (i, result) in results.iter().enumerate() {
            let title = result.text.lines().next().unwrap_or("").to_string();
            let author = result.author.as_deref().unwrap_or("");
            let content = format!("{}\nAuthor: {}\nDate: ", result.short_hash, author);

            cards.push(CardData {
                id: i,
                position: Vec2::new(8.0, y_offset),
                size: Vec2::new(card_width, CARD_HEIGHT),
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

            y_offset += CARD_HEIGHT + card_padding;
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

            cards.push(CardData {
                id: i,
                position: Vec2::new(8.0, y_offset),
                size: Vec2::new(card_width, CARD_HEIGHT),
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

            y_offset += CARD_HEIGHT + card_padding;
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


