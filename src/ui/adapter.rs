use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::git_log::CommitInfo;
use daftprompt_indexer::{CodeSearchResult, DocumentSearchResult, SearchResult};

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

pub fn stable_item_key_document_search(result: &DocumentSearchResult) -> u64 {
    let mut hasher = DefaultHasher::new();
    result.identifier.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_log::CommitInfo;
    use crate::state::CardData;
    use daftprompt_indexer::{
        CodeSearchResult, DocumentSearchResult, MatchType, SearchResult, SymbolKind,
    };

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
                document_id: 0,
                stable_key: 1,
                is_selected: false,
            },
            CardData {
                document_id: 1,
                stable_key: 2,
                is_selected: false,
            },
            CardData {
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
                document_id: 0,
                stable_key: 1,
                is_selected: true,
            },
            CardData {
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
                document_id: 0,
                stable_key: 1,
                is_selected: true,
            },
            CardData {
                document_id: 1,
                stable_key: 2,
                is_selected: false,
            },
        ];

        apply_selection(&mut cards, 0);

        assert!(cards[0].is_selected);
        assert!(!cards[1].is_selected);
    }

    // --- stable_item_key_document_search ---

    #[test]
    fn stable_item_key_document_search_is_deterministic() {
        let r = DocumentSearchResult {
            identifier: "docs/readme.md".to_string(),
            file_path: "docs/readme.md".to_string(),
            text: "# Hello World\n\nWelcome.".to_string(),
            score: 0.9,
            match_type: MatchType::Hybrid,
        };
        let key1 = stable_item_key_document_search(&r);
        let key2 = stable_item_key_document_search(&r);
        assert_eq!(key1, key2);
        assert_ne!(key1, 0);
    }

    #[test]
    fn stable_item_key_document_search_different_inputs() {
        let a = DocumentSearchResult {
            identifier: "docs/a.md".to_string(),
            file_path: String::new(),
            text: String::new(),
            score: 0.0,
            match_type: MatchType::Fts,
        };
        let b = DocumentSearchResult {
            identifier: "docs/b.md".to_string(),
            file_path: String::new(),
            text: String::new(),
            score: 0.0,
            match_type: MatchType::Fts,
        };
        assert_ne!(
            stable_item_key_document_search(&a),
            stable_item_key_document_search(&b)
        );
    }
}
