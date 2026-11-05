//! Deterministic format detection and source-preserving block construction.

use daftprompt_types::{ContentType, ContentTypeSource, ParseRequest};
use pulldown_cmark::{Event, Options, Parser, Tag};

use super::{Segment, SegmentRange, segments};

/// Reader-visible units offered to Scribe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Section,
    Item,
    Paragraph,
    Code,
    Quote,
    Table,
}

impl BlockKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Section => "section",
            Self::Item => "item",
            Self::Paragraph => "paragraph",
            Self::Code => "code",
            Self::Quote => "quote",
            Self::Table => "table",
        }
    }
}

/// A block's local identity, parent, inclusive segments and trimmed byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: String,
    pub kind: BlockKind,
    pub parent: Option<String>,
    pub depth: usize,
    pub segments: SegmentRange,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// A preorder block tree; parents precede their descendants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structure {
    pub content_type: ContentType,
    pub blocks: Vec<Block>,
}

/// Resolve a declaration or detect a format from text alone. File names are
/// never sufficient evidence and cannot override a declaration.
pub fn resolve_content_type(request: &ParseRequest) -> (ContentType, ContentTypeSource) {
    match request.content_type {
        Some(format) => (format, ContentTypeSource::Declared),
        None => (
            detect_content_type(&request.text),
            ContentTypeSource::Detected,
        ),
    }
}

/// Deterministic signals, deliberately independent of provenance.
pub fn detect_content_type(source: &str) -> ContentType {
    let mut items = 0;
    for line in source.lines() {
        let line = line.trim_start();
        if atx_level(line).is_some()
            || line.starts_with("```")
            || line.starts_with("~~~")
            || table_delimiter(line)
        {
            return ContentType::Markdown;
        }
        let ordered = line.split_once(". ").is_some_and(|(number, _)| {
            !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
        });
        if ["- ", "* ", "+ "]
            .iter()
            .any(|prefix| line.starts_with(prefix))
            || ordered
        {
            items += 1;
        }
    }
    if items >= 2 {
        ContentType::Markdown
    } else {
        ContentType::Plain
    }
}

fn atx_level(line: &str) -> Option<usize> {
    let count = line.bytes().take_while(|b| *b == b'#').count();
    ((1..=6).contains(&count) && line.as_bytes().get(count) == Some(&b' ')).then_some(count)
}

fn table_delimiter(line: &str) -> bool {
    let has_pipe = line.contains('|');
    let line = line.trim().trim_matches('|');
    has_pipe
        && line.split('|').all(|cell| {
            let cell = cell.trim().trim_matches(':');
            cell.len() >= 3 && cell.bytes().all(|b| b == b'-')
        })
}

/// Build blocks without changing the original text or segment numbering.
pub fn structure(source: &str, content_type: ContentType) -> Structure {
    let lines = segments(source);
    let mut candidates = match content_type {
        ContentType::Plain => plain(source, &lines),
        ContentType::Markdown => markdown(source, &lines),
    };
    candidates.sort_by_key(|block| (block.segments.start, std::cmp::Reverse(block.segments.end)));
    // Mapping parser offsets to lines can collapse same-line nested constructs.
    // Keep the outermost unit rather than offering duplicate identical spans.
    candidates.dedup_by_key(|block| (block.segments.start, block.segments.end));
    let mut stack: Vec<usize> = Vec::new();
    for index in 0..candidates.len() {
        while stack.last().is_some_and(|&parent| {
            candidates[parent].segments.end < candidates[index].segments.end
                || candidates[parent].segments.end < candidates[index].segments.start
        }) {
            stack.pop();
        }
        candidates[index].id = format!("b{}", index + 1);
        candidates[index].parent = stack.last().map(|&parent| candidates[parent].id.clone());
        candidates[index].depth = stack.len();
        stack.push(index);
    }
    Structure {
        content_type,
        blocks: candidates,
    }
}

fn block(
    source: &str,
    lines: &[Segment],
    kind: BlockKind,
    start: usize,
    end: usize,
) -> Option<Block> {
    if start >= end {
        return None;
    }
    let first = lines.iter().position(|line| line.end_byte >= start)?;
    let last = lines.iter().rposition(|line| line.start_byte < end)?;
    let region = &source[lines[first].start_byte..lines[last].end_byte];
    let trimmed = region.trim();
    if trimmed.is_empty() {
        return None;
    }
    let start_byte = lines[first].start_byte + region.len() - region.trim_start().len();
    Some(Block {
        id: String::new(),
        kind,
        parent: None,
        depth: 0,
        segments: SegmentRange {
            start: lines[first].number,
            end: lines[last].number,
        },
        start_byte,
        end_byte: start_byte + trimmed.len(),
    })
}

fn plain(source: &str, lines: &[Segment]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut first = None;
    for line in lines {
        if line.text(source).trim().is_empty() {
            if let Some(start) = first.take()
                && let Some(block) =
                    block(source, lines, BlockKind::Paragraph, start, line.start_byte)
            {
                blocks.push(block);
            }
        } else {
            first.get_or_insert(line.start_byte);
        }
    }
    if let Some(start) = first
        && let Some(block) = block(source, lines, BlockKind::Paragraph, start, source.len())
    {
        blocks.push(block);
    }
    blocks
}

fn markdown(source: &str, lines: &[Segment]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut headings = Vec::new();
    let mut ancestors = Vec::new();
    // Front matter is one ordinary paragraph, never headings or code.
    let prefix = if lines.first().is_some_and(|line| line.text(source) == "---") {
        lines
            .iter()
            .skip(1)
            .find(|line| matches!(line.text(source), "---" | "..."))
            .map_or(0, |line| {
                source[line.end_byte..]
                    .find('\n')
                    .map_or(line.end_byte, |n| line.end_byte + n + 1)
            })
    } else {
        0
    };
    if prefix > 0 {
        blocks.extend(block(source, lines, BlockKind::Paragraph, 0, prefix));
    }
    for (event, range) in
        Parser::new_ext(&source[prefix..], Options::ENABLE_TABLES).into_offset_iter()
    {
        match event {
            Event::Start(tag) => {
                let start = prefix + range.start;
                let end = prefix + range.end;
                let inside_item = ancestors.iter().any(|kind| {
                    matches!(
                        kind,
                        Some(
                            BlockKind::Item | BlockKind::Quote | BlockKind::Table | BlockKind::Code
                        )
                    )
                });
                let kind = match &tag {
                    Tag::Heading { level, .. }
                        if !inside_item && atx_level(source[start..end].trim_start()).is_some() =>
                    {
                        headings.push((start, *level as usize));
                        None
                    }
                    Tag::Heading { .. } | Tag::Paragraph if !inside_item => {
                        Some(BlockKind::Paragraph)
                    }
                    Tag::Item => Some(BlockKind::Item),
                    Tag::CodeBlock(_) => Some(BlockKind::Code),
                    Tag::BlockQuote(_) => Some(BlockKind::Quote),
                    Tag::Table(_) => Some(BlockKind::Table),
                    _ => None,
                };
                if let Some(kind) = kind {
                    blocks.extend(block(source, lines, kind, start, end));
                }
                ancestors.push(kind);
            }
            Event::End(_) => {
                ancestors.pop();
            }
            _ => {}
        }
    }
    for (index, &(start, level)) in headings.iter().enumerate() {
        let end = headings[index + 1..]
            .iter()
            .find(|(_, next_level)| *next_level <= level)
            .map_or(source.len(), |(start, _)| *start);
        // Sections must sort before children sharing their first segment.
        blocks.insert(
            0,
            block(source, lines, BlockKind::Section, start, end).expect("heading has text"),
        );
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_signal_and_rejects_plain_lookalikes() {
        for source in [
            "# Title",
            "###### Title",
            "```rust",
            "~~~",
            "- one\n- two",
            "* one\n+ two",
            "1. one\n2. two",
            "| --- | :---: |",
        ] {
            assert_eq!(
                detect_content_type(source),
                ContentType::Markdown,
                "{source}"
            );
        }
        for source in [
            "",
            "ordinary prose",
            "#hashtag",
            "####### title",
            "- only one",
            "1. just one",
            "a | b",
        ] {
            assert_eq!(detect_content_type(source), ContentType::Plain, "{source}");
        }
    }

    #[test]
    fn declaration_wins_and_file_extension_is_not_proof() {
        let mut request = ParseRequest::with_source(
            "plain words",
            daftprompt_types::SourceMetadata {
                kind: Some(daftprompt_types::SourceKind::File),
                reference: Some("file.md".into()),
                author: None,
            },
        );
        assert_eq!(
            resolve_content_type(&request),
            (ContentType::Plain, ContentTypeSource::Detected)
        );
        request.text = "# heading".into();
        request.content_type = Some(ContentType::Plain);
        assert_eq!(
            resolve_content_type(&request),
            (ContentType::Plain, ContentTypeSource::Declared)
        );
    }

    #[test]
    fn plain_paragraphs_preserve_unicode_crlf_and_marker_text() {
        let source = "  café\r\ne\u{301} 😀  \r\n\r\n- item\r\n> quote";
        let tree = structure(source, ContentType::Plain);
        assert_eq!(tree.blocks.len(), 2);
        assert_eq!(tree.blocks[0].segments, SegmentRange { start: 1, end: 2 });
        assert_eq!(
            &source[tree.blocks[0].start_byte..tree.blocks[0].end_byte],
            "café\r\ne\u{301} 😀"
        );
        assert_eq!(
            &source[tree.blocks[1].start_byte..tree.blocks[1].end_byte],
            "- item\r\n> quote"
        );
        for source in ["", "\r\n \n"] {
            assert!(structure(source, ContentType::Plain).blocks.is_empty());
        }
    }

    #[test]
    fn markdown_sections_and_items_form_a_preorder_tree() {
        let source = "# Plan\n\nIntro\n\n## Step\n\n- parent\n  continuation\n  - nested\n\n## Next\n\nFinish";
        let tree = structure(source, ContentType::Markdown);
        let kinds: Vec<_> = tree.blocks.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            [
                BlockKind::Section,
                BlockKind::Paragraph,
                BlockKind::Section,
                BlockKind::Item,
                BlockKind::Item,
                BlockKind::Section,
                BlockKind::Paragraph
            ]
        );
        assert_eq!(tree.blocks[2].parent.as_deref(), Some("b1"));
        assert_eq!(tree.blocks[4].parent.as_deref(), Some("b4"));
        assert_eq!(tree.blocks[3].segments, SegmentRange { start: 7, end: 10 });
        assert_eq!(tree.blocks[2].segments, SegmentRange { start: 5, end: 10 });
        assert_eq!(tree.blocks[5].segments.start, 11);
    }

    #[test]
    fn fences_quotes_tables_indented_code_and_setext_are_whole_blocks() {
        let source = "```md\n# fake\n- fake\n```\n\n> quote\n> more\n\n| A | B |\n| --- | --- |\n| x | y |\n\n    code\n    more\n\nTitle\n=====";
        let tree = structure(source, ContentType::Markdown);
        assert_eq!(
            tree.blocks.iter().map(|b| b.kind).collect::<Vec<_>>(),
            [
                BlockKind::Code,
                BlockKind::Quote,
                BlockKind::Table,
                BlockKind::Code,
                BlockKind::Paragraph
            ]
        );
        assert_eq!(tree.blocks[0].segments, SegmentRange { start: 1, end: 4 });
        assert_eq!(tree.blocks[4].segments, SegmentRange { start: 16, end: 17 });
    }

    #[test]
    fn markdown_frontmatter_and_empty_input() {
        let source = "---\n# metadata\nkey: value\n---\n\n# Real\r\n\r\ncafé e\u{301} 😀";
        let tree = structure(source, ContentType::Markdown);
        assert_eq!(tree.blocks.len(), 3);
        assert_eq!(tree.blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(tree.blocks[0].segments, SegmentRange { start: 1, end: 4 });
        assert_eq!(
            &source[tree.blocks[2].start_byte..tree.blocks[2].end_byte],
            "café e\u{301} 😀"
        );
        assert!(structure("", ContentType::Markdown).blocks.is_empty());
    }
}
