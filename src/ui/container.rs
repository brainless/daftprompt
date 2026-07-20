use glam::Vec2;

use crate::git_log::CommitInfo;
use crate::state::{CardData, DocumentData};
use crate::ui::adapter;
use sugacode_indexer::{CodeSearchResult, DocumentSearchResult, SearchResult};

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
    GitLogColumn,
    SearchResults,
    CodeSearchResults,
    DocumentSearchResults,
}

pub struct Container {
    pub id: usize,
    pub position: Vec2,
    pub size: Vec2,
    pub scroll_offset: f32,
    pub container_type: ContainerType,
    pub cards: Vec<CardData>,
    pub documents: Vec<DocumentData>,
}

impl Container {
    pub fn new_git_log(
        id: usize,
        position: Vec2,
        width: f32,
        viewport_height: f32,
        commits: Vec<CommitInfo>,
    ) -> Self {
        let mut cards = Vec::new();
        let mut documents = Vec::new();

        for (i, commit) in commits.iter().enumerate() {
            cards.push(CardData {
                document_id: i,
                stable_key: adapter::stable_item_key_commit(commit),
                is_selected: false,
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
        }

        Self {
            id,
            position,
            size: Vec2::new(width, viewport_height),
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
        let mut cards = Vec::new();
        let mut documents = Vec::new();

        for (i, result) in results.iter().enumerate() {
            let title = result.text.lines().next().unwrap_or("").to_string();
            let author = result.author.as_deref().unwrap_or("");
            let content = format!("{}\nAuthor: {}\nDate: ", result.short_hash, author);

            cards.push(CardData {
                document_id: i,
                stable_key: adapter::stable_item_key_search(result),
                is_selected: false,
            });

            documents.push(DocumentData {
                title,
                content,
                file_type: crate::state::IconType::Code,
                folder_id: 0,
            });
        }

        Self {
            id,
            position,
            size: Vec2::new(width, viewport_height),
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
        let mut cards = Vec::new();
        let mut documents = Vec::new();

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
                document_id: i,
                stable_key: adapter::stable_item_key_code_search(result),
                is_selected: false,
            });

            documents.push(DocumentData {
                title,
                content,
                file_type: crate::state::IconType::Code,
                folder_id: 0,
            });
        }

        Self {
            id,
            position,
            size: Vec2::new(width, viewport_height),
            scroll_offset: 0.0,
            container_type: ContainerType::CodeSearchResults,
            cards,
            documents,
        }
    }

    pub fn new_document_search_results(
        id: usize,
        position: Vec2,
        width: f32,
        viewport_height: f32,
        results: Vec<DocumentSearchResult>,
    ) -> Self {
        let mut cards = Vec::new();
        let mut documents = Vec::new();

        for (i, result) in results.iter().enumerate() {
            let title = result.file_path.clone();
            let preview = result.text.lines().next().unwrap_or("").to_string();

            cards.push(CardData {
                document_id: i,
                stable_key: adapter::stable_item_key_document_search(result),
                is_selected: false,
            });

            documents.push(DocumentData {
                title,
                content: preview,
                file_type: crate::state::IconType::Document,
                folder_id: 0,
            });
        }

        Self {
            id,
            position,
            size: Vec2::new(width, viewport_height),
            scroll_offset: 0.0,
            container_type: ContainerType::DocumentSearchResults,
            cards,
            documents,
        }
    }
}
