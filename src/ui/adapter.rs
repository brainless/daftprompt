use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use akar_components::{AkarTheme, CanvasDataItemDescriptor, DataItemStyle};

use crate::git_log::CommitInfo;
use sugacode_indexer::{CodeSearchResult, SearchResult};

fn color_u32_to_f32(c: u32) -> [f32; 4] {
    [
        ((c >> 24) & 0xFF) as f32 / 255.0,
        ((c >> 16) & 0xFF) as f32 / 255.0,
        ((c >> 8) & 0xFF) as f32 / 255.0,
        (c & 0xFF) as f32 / 255.0,
    ]
}

pub fn commit_to_item_descriptor<'a>(
    commit: &'a CommitInfo,
    style: &'a DataItemStyle,
) -> CanvasDataItemDescriptor<'a> {
    CanvasDataItemDescriptor {
        title: Some(&commit.message_title),
        supporting_text: Some(&commit.short_hash),
        metadata: Some(&commit.time),
        style,
    }
}

pub fn search_result_to_item_descriptor<'a>(
    result: &'a SearchResult,
    style: &'a DataItemStyle,
) -> CanvasDataItemDescriptor<'a> {
    let title = result.text.lines().next().unwrap_or("");
    CanvasDataItemDescriptor {
        title: Some(title),
        supporting_text: Some(&result.short_hash),
        metadata: None,
        style,
    }
}

pub fn code_search_result_to_item_descriptor<'a>(
    result: &'a CodeSearchResult,
    style: &'a DataItemStyle,
) -> CanvasDataItemDescriptor<'a> {
    let title = result
        .identifier
        .split("::")
        .last()
        .unwrap_or(&result.identifier);
    CanvasDataItemDescriptor {
        title: Some(title),
        supporting_text: Some(&result.file_path),
        metadata: None,
        style,
    }
}

pub fn stable_item_key_commit(commit: &CommitInfo) -> u64 {
    let mut hasher = DefaultHasher::new();
    commit.sha.hash(&mut hasher);
    hasher.finish()
}

pub fn stable_item_key_search(result: &SearchResult) -> u64 {
    let mut hasher = DefaultHasher::new();
    result.identifier.hash(&mut hasher);
    hasher.finish()
}

pub fn stable_item_key_code_search(result: &CodeSearchResult) -> u64 {
    let mut hasher = DefaultHasher::new();
    result.identifier.hash(&mut hasher);
    hasher.finish()
}

pub fn default_data_item_style(theme: &AkarTheme) -> DataItemStyle {
    DataItemStyle {
        surface: color_u32_to_f32(theme.base_100),
        padding_x: theme.padding_x,
        padding_y: theme.padding_y,
        spacing: 8.0,
        color_normal: color_u32_to_f32(theme.base_100),
        color_hover: color_u32_to_f32(theme.base_300),
        color_pressed: color_u32_to_f32(theme.base_300),
        color_selected: color_u32_to_f32(theme.primary),
        corner_radius: theme.radius_box,
        border_width: theme.border_width,
        border_color: color_u32_to_f32(theme.base_300),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_log::CommitInfo;
    use crate::state::CardData;
    use akar_components::{AKAR_THEME_DARK, AKAR_THEME_LIGHT};
    use sugacode_indexer::{CodeSearchResult, MatchType, SearchResult, SymbolKind};

    // --- stable_item_key_commit ---

    #[test]
    fn stable_item_key_commit_is_deterministic() {
        let commit = CommitInfo {
            sha: "abc123def456".to_string(),
            short_hash: "abc123".to_string(),
            author_name: "Test Author".to_string(),
            time: "2024-01-15".to_string(),
            message_title: "Fix critical bug".to_string(),
            message_body: "".to_string(),
        };
        let key1 = stable_item_key_commit(&commit);
        let key2 = stable_item_key_commit(&commit);
        assert_eq!(key1, key2);
        assert_ne!(key1, 0);
    }

    #[test]
    fn stable_item_key_commit_different_inputs() {
        let a = CommitInfo {
            sha: "abc123def456".to_string(),
            short_hash: "abc123".to_string(),
            author_name: "Alice".to_string(),
            time: "2024-01-15".to_string(),
            message_title: "Fix bug".to_string(),
            message_body: "".to_string(),
        };
        let b = CommitInfo {
            sha: "789012345678".to_string(),
            short_hash: "789012".to_string(),
            author_name: "Bob".to_string(),
            time: "2024-01-16".to_string(),
            message_title: "Add feature".to_string(),
            message_body: "".to_string(),
        };
        assert_ne!(stable_item_key_commit(&a), stable_item_key_commit(&b));
    }

    // --- stable_item_key_search ---

    #[test]
    fn stable_item_key_search_is_deterministic() {
        let r = SearchResult {
            identifier: "abc123def456".to_string(),
            short_hash: "abc123".to_string(),
            text: "Fix critical bug\n\nDetailed description".to_string(),
            author: Some("Test Author".to_string()),
            score: 0.95,
            match_type: MatchType::Hybrid,
        };
        let key1 = stable_item_key_search(&r);
        let key2 = stable_item_key_search(&r);
        assert_eq!(key1, key2);
        assert_ne!(key1, 0);
    }

    #[test]
    fn stable_item_key_search_different_inputs() {
        let a = SearchResult {
            identifier: "abc123".to_string(),
            short_hash: String::new(),
            text: String::new(),
            author: None,
            score: 0.0,
            match_type: MatchType::Fts,
        };
        let b = SearchResult {
            identifier: "def456".to_string(),
            short_hash: String::new(),
            text: String::new(),
            author: None,
            score: 0.0,
            match_type: MatchType::Fts,
        };
        assert_ne!(stable_item_key_search(&a), stable_item_key_search(&b));
    }

    // --- stable_item_key_code_search ---

    #[test]
    fn stable_item_key_code_search_is_deterministic() {
        let r = CodeSearchResult {
            identifier: "my_module::MyStruct::my_method".to_string(),
            symbol_kind: SymbolKind::Function,
            file_path: "src/main.rs".to_string(),
            line_start: 42,
            line_end: 56,
            text: "fn my_method(&self) { ... }".to_string(),
            score: 0.85,
            match_type: MatchType::Hybrid,
        };
        let key1 = stable_item_key_code_search(&r);
        let key2 = stable_item_key_code_search(&r);
        assert_eq!(key1, key2);
        assert_ne!(key1, 0);
    }

    #[test]
    fn stable_item_key_code_search_different_inputs() {
        let a = CodeSearchResult {
            identifier: "foo::bar".to_string(),
            symbol_kind: SymbolKind::Function,
            file_path: String::new(),
            line_start: 0,
            line_end: 0,
            text: String::new(),
            score: 0.0,
            match_type: MatchType::Fts,
        };
        let b = CodeSearchResult {
            identifier: "baz::qux".to_string(),
            symbol_kind: SymbolKind::Struct,
            file_path: String::new(),
            line_start: 0,
            line_end: 0,
            text: String::new(),
            score: 0.0,
            match_type: MatchType::Fts,
        };
        assert_ne!(
            stable_item_key_code_search(&a),
            stable_item_key_code_search(&b)
        );
    }

    // --- commit_to_item_descriptor ---

    #[test]
    fn commit_to_item_descriptor_fields() {
        let commit = CommitInfo {
            sha: "abc123def456".to_string(),
            short_hash: "abc123".to_string(),
            author_name: "Test Author".to_string(),
            time: "2024-01-15".to_string(),
            message_title: "Fix critical bug".to_string(),
            message_body: "".to_string(),
        };
        let style = default_data_item_style(&AKAR_THEME_DARK);
        let desc = commit_to_item_descriptor(&commit, &style);

        assert_eq!(desc.title, Some("Fix critical bug"));
        assert_eq!(desc.supporting_text, Some("abc123"));
        assert_eq!(desc.metadata, Some("2024-01-15"));
        assert!(std::ptr::eq(desc.style, &style));
    }

    // --- search_result_to_item_descriptor ---

    #[test]
    fn search_result_to_item_descriptor_fields() {
        let result = SearchResult {
            identifier: "abc123def456".to_string(),
            short_hash: "abc123".to_string(),
            text: "Fix critical bug\n\nDetailed description".to_string(),
            author: Some("Test Author".to_string()),
            score: 0.95,
            match_type: MatchType::Hybrid,
        };
        let style = default_data_item_style(&AKAR_THEME_DARK);
        let desc = search_result_to_item_descriptor(&result, &style);

        assert_eq!(desc.title, Some("Fix critical bug"));
        assert_eq!(desc.supporting_text, Some("abc123"));
        assert!(desc.metadata.is_none());
        assert!(std::ptr::eq(desc.style, &style));
    }

    #[test]
    fn search_result_to_item_descriptor_empty_text() {
        let result = SearchResult {
            identifier: "empty".to_string(),
            short_hash: "emp".to_string(),
            text: "".to_string(),
            author: None,
            score: 0.0,
            match_type: MatchType::Fts,
        };
        let style = default_data_item_style(&AKAR_THEME_DARK);
        let desc = search_result_to_item_descriptor(&result, &style);
        assert_eq!(desc.title, Some(""));
    }

    // --- code_search_result_to_item_descriptor ---

    #[test]
    fn code_search_result_to_item_descriptor_fields() {
        let result = CodeSearchResult {
            identifier: "my_module::MyStruct::my_method".to_string(),
            symbol_kind: SymbolKind::Function,
            file_path: "src/main.rs".to_string(),
            line_start: 42,
            line_end: 56,
            text: "fn my_method(&self) { ... }".to_string(),
            score: 0.85,
            match_type: MatchType::Hybrid,
        };
        let style = default_data_item_style(&AKAR_THEME_DARK);
        let desc = code_search_result_to_item_descriptor(&result, &style);

        assert_eq!(desc.title, Some("my_method"));
        assert_eq!(desc.supporting_text, Some("src/main.rs"));
        assert!(desc.metadata.is_none());
        assert!(std::ptr::eq(desc.style, &style));
    }

    #[test]
    fn code_search_result_to_item_descriptor_single_segment() {
        let result = CodeSearchResult {
            identifier: "simple_function".to_string(),
            symbol_kind: SymbolKind::Function,
            file_path: "lib.rs".to_string(),
            line_start: 1,
            line_end: 5,
            text: "fn simple_function() {}".to_string(),
            score: 0.5,
            match_type: MatchType::Fts,
        };
        let style = default_data_item_style(&AKAR_THEME_DARK);
        let desc = code_search_result_to_item_descriptor(&result, &style);
        assert_eq!(desc.title, Some("simple_function"));
    }

    // --- default_data_item_style ---

    #[test]
    fn default_data_item_style_has_non_zero_fields() {
        let style = default_data_item_style(&AKAR_THEME_DARK);

        assert!(
            style.surface[0] > 0.0 || style.surface[1] > 0.0 || style.surface[2] > 0.0,
            "surface RGB should be non-zero for dark theme"
        );
        assert_eq!(style.surface[3], 1.0, "surface alpha should be 1.0");
        assert!(style.padding_x > 0.0);
        assert!(style.padding_y > 0.0);
        assert_eq!(style.spacing, 8.0);
    }

    #[test]
    fn default_data_item_style_matches_theme_fields() {
        let style = default_data_item_style(&AKAR_THEME_DARK);

        assert_eq!(style.padding_x, AKAR_THEME_DARK.padding_x);
        assert_eq!(style.padding_y, AKAR_THEME_DARK.padding_y);
        assert_eq!(style.corner_radius, AKAR_THEME_DARK.radius_box);
        assert_eq!(style.border_width, AKAR_THEME_DARK.border_width);
    }

    #[test]
    fn default_data_item_style_differs_between_themes() {
        let dark = default_data_item_style(&AKAR_THEME_DARK);
        let light = default_data_item_style(&AKAR_THEME_LIGHT);

        assert_ne!(dark.surface, light.surface);
    }

    // --- Selection logic ---

    fn apply_selection(cards: &mut [CardData], clicked_index: usize) {
        for (j, card) in cards.iter_mut().enumerate() {
            card.is_selected = j == clicked_index;
        }
    }

    #[test]
    fn select_clicked_card() {
        let mut cards = vec![
            CardData {
                id: 0,
                position: glam::Vec2::ZERO,
                size: glam::Vec2::new(100.0, 50.0),
                document_id: 0,
                stable_key: 1,
                is_selected: false,
            },
            CardData {
                id: 1,
                position: glam::Vec2::ZERO,
                size: glam::Vec2::new(100.0, 50.0),
                document_id: 1,
                stable_key: 2,
                is_selected: false,
            },
            CardData {
                id: 2,
                position: glam::Vec2::ZERO,
                size: glam::Vec2::new(100.0, 50.0),
                document_id: 2,
                stable_key: 3,
                is_selected: false,
            },
        ];

        apply_selection(&mut cards, 1);

        assert!(!cards[0].is_selected);
        assert!(cards[1].is_selected);
        assert!(!cards[2].is_selected);
    }

    #[test]
    fn select_deselects_previous() {
        let mut cards = vec![
            CardData {
                id: 0,
                position: glam::Vec2::ZERO,
                size: glam::Vec2::new(100.0, 50.0),
                document_id: 0,
                stable_key: 1,
                is_selected: true,
            },
            CardData {
                id: 1,
                position: glam::Vec2::ZERO,
                size: glam::Vec2::new(100.0, 50.0),
                document_id: 1,
                stable_key: 2,
                is_selected: false,
            },
        ];

        apply_selection(&mut cards, 1);

        assert!(!cards[0].is_selected);
        assert!(cards[1].is_selected);
    }

    #[test]
    fn click_already_selected_card_stays_selected() {
        let mut cards = vec![
            CardData {
                id: 0,
                position: glam::Vec2::ZERO,
                size: glam::Vec2::new(100.0, 50.0),
                document_id: 0,
                stable_key: 1,
                is_selected: true,
            },
            CardData {
                id: 1,
                position: glam::Vec2::ZERO,
                size: glam::Vec2::new(100.0, 50.0),
                document_id: 1,
                stable_key: 2,
                is_selected: false,
            },
        ];

        apply_selection(&mut cards, 0);

        assert!(cards[0].is_selected);
        assert!(!cards[1].is_selected);
    }
}
