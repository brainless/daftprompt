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
