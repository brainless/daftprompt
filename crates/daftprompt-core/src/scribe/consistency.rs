//! Sibling-consistency detection.
//!
//! Siblings under one heading usually share a classification. After an
//! extraction validates, Rust looks for list items (or sub-sections) where the
//! model labelled one block exactly but left same-kind siblings untouched.
//! Rust only detects the gap; the model is asked again and proposes any labels.
//! Rust never copies a label onto a block itself.

use daftprompt_types::{AnnotationKind, Extraction};

use super::validate::ValidatedExtraction;
use crate::text::structure::{BlockKind, Structure};

/// One group of same-kind siblings where some were labelled and others untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SiblingGap {
    /// The first (in source order) block exactly covered by an annotated span.
    pub(crate) example: String,
    pub(crate) example_kind: AnnotationKind,
    pub(crate) block_kind: BlockKind,
    /// Siblings with no span overlapping them or their descendants.
    pub(crate) untouched: Vec<String>,
}

fn kind_name(kind: AnnotationKind) -> &'static str {
    match kind {
        AnnotationKind::Question => "question",
        AnnotationKind::Answer => "answer",
        AnnotationKind::Recommendation => "recommendation",
        AnnotationKind::Task => "task",
    }
}

/// Find sibling gaps among `item` and `section` blocks.
pub(crate) fn sibling_gaps(
    tree: &Structure,
    extraction: &Extraction,
    validated: &ValidatedExtraction,
) -> Vec<SiblingGap> {
    let mut gaps = Vec::new();
    // Group key: (parent, kind); the virtual root is `None`. Blocks are in
    // preorder, so each group's members are already in source order.
    let mut groups: Vec<(Option<&str>, BlockKind, Vec<usize>)> = Vec::new();
    for (index, block) in tree.blocks.iter().enumerate() {
        if !matches!(block.kind, BlockKind::Item | BlockKind::Section) {
            continue;
        }
        let parent = block.parent.as_deref();
        match groups
            .iter_mut()
            .find(|(p, k, _)| *p == parent && *k == block.kind)
        {
            Some((_, _, members)) => members.push(index),
            None => groups.push((parent, block.kind, vec![index])),
        }
    }

    for (_, block_kind, members) in groups {
        if members.len() < 2 {
            continue;
        }
        let mut example = None;
        let mut untouched = Vec::new();
        for &index in &members {
            let block = &tree.blocks[index];
            let exact = validated
                .spans
                .iter()
                .position(|s| s.start_byte == block.start_byte && s.end_byte == block.end_byte);
            let touched = validated
                .spans
                .iter()
                .any(|s| s.start_byte < block.end_byte && block.start_byte < s.end_byte);
            if let Some(span) = exact
                && example.is_none()
                && let Some(annotation) = extraction
                    .annotations
                    .iter()
                    .find(|a| a.span == validated.spans[span].local_id)
            {
                example = Some((block.id.clone(), annotation.kind));
            }
            if !touched {
                untouched.push(block.id.clone());
            }
        }
        if let Some((example, example_kind)) = example
            && !untouched.is_empty()
        {
            gaps.push(SiblingGap {
                example,
                example_kind,
                block_kind,
                untouched,
            });
        }
    }
    gaps
}

/// Corrective feedback naming every gap, appended to the follow-up prompt.
pub(crate) fn feedback(gaps: &[SiblingGap]) -> String {
    let mut text = String::new();
    for gap in gaps {
        text.push_str(&format!(
            "Your extraction labelled block {} as a {}, but its sibling blocks {} (same parent, same kind: {}) have no span. ",
            gap.example,
            kind_name(gap.example_kind),
            gap.untouched.join(", "),
            gap.block_kind.as_str(),
        ));
    }
    text.push_str(
        "Siblings under one heading usually share a classification. Resubmit the complete \
         extraction, giving those siblings the same kind unless one is clearly different.",
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scribe::validate::validate;
    use crate::text::structure::structure;
    use daftprompt_types::{ContentType, ExtractedAnnotation, ExtractedSpan};

    const MD: &str = "## Good\n\n- one\n- two\n- three\n\n## Other\n\n- x\n";

    fn run(source: &str, ct: ContentType, blocks: &[&str]) -> Vec<SiblingGap> {
        let tree = structure(source, ct);
        let spans = blocks
            .iter()
            .enumerate()
            .map(|(i, b)| ExtractedSpan::Block {
                id: format!("s{i}"),
                block: (*b).to_owned(),
            })
            .collect();
        let annotations = (0..blocks.len())
            .map(|i| ExtractedAnnotation {
                kind: AnnotationKind::Recommendation,
                span: format!("s{i}"),
                answers: vec![],
                questions: vec![],
                alternatives: vec![],
                assignee: None,
            })
            .collect();
        let extraction = Extraction { spans, annotations };
        let validated = validate(source, &extraction, &tree).expect("valid");
        sibling_gaps(&tree, &extraction, &validated)
    }

    fn id_of(source: &str, text: &str) -> String {
        let tree = structure(source, ContentType::Markdown);
        tree.blocks
            .iter()
            .find(|b| &source[b.start_byte..b.end_byte] == text)
            .expect("block")
            .id
            .clone()
    }

    #[test]
    fn labelled_item_with_untouched_siblings_is_a_gap() {
        let one = id_of(MD, "- one");
        let gaps = run(MD, ContentType::Markdown, &[&one]);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].example, one);
        assert_eq!(
            gaps[0].untouched,
            vec![id_of(MD, "- two"), id_of(MD, "- three")]
        );
        assert!(feedback(&gaps).contains(&one));
    }

    #[test]
    fn all_siblings_labelled_is_not_a_gap() {
        let ids: Vec<String> = ["- one", "- two", "- three"].map(|t| id_of(MD, t)).to_vec();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        assert!(run(MD, ContentType::Markdown, &refs).is_empty());
    }

    #[test]
    fn single_item_group_is_not_a_gap() {
        let x = id_of(MD, "- x");
        assert!(run(MD, ContentType::Markdown, &[&x]).is_empty());
    }

    #[test]
    fn sections_and_items_are_compared_separately() {
        let src = "- a\n- b\n\n## Sub\n\ntext\n";
        let a = id_of(src, "- a");
        let gaps = run(src, ContentType::Markdown, &[&a]);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].untouched, vec![id_of(src, "- b")]);
        // One of two top-level sections labelled leaves the other a section gap.
        let src = "## A\n\ntext\n\n## B\n\ntext\n";
        let a = id_of(src, "## A\n\ntext");
        assert_eq!(run(src, ContentType::Markdown, &[&a]).len(), 1);
    }

    #[test]
    fn sibling_with_labelled_descendant_is_touched() {
        let src = "- a\n- b\n  - c\n";
        let a = id_of(src, "- a");
        let c = id_of(src, "- c");
        let gaps = run(src, ContentType::Markdown, &[&a, &c]);
        assert!(gaps.is_empty(), "{gaps:?}");
    }

    #[test]
    fn parent_labelled_whole_has_no_gap() {
        let src = "## Good\n\n- one\n- two\n- three\n";
        let whole = id_of(src, src.trim());
        assert!(run(src, ContentType::Markdown, &[&whole]).is_empty());
    }

    #[test]
    fn plain_text_has_no_gaps() {
        let src = "one\n\ntwo\n\nthree\n";
        let tree = structure(src, ContentType::Plain);
        let b = tree.blocks[0].id.clone();
        assert!(run(src, ContentType::Plain, &[&b]).is_empty());
    }
}
