//! A comparable form of a Scribe result.
//!
//! Scoring does not depend on graph ids, hashes, or timestamps. A
//! [`Prediction`] keeps only what the fixture annotations describe: byte
//! ranges, label kinds, answer links, and assignee handles. It is built either
//! from a validated [`ParseResult`] or directly in deterministic tests.

use daftprompt_types::{AnnotationKind, LabelKind, ParseResult};

/// A half-open byte range into a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

impl ByteRange {
    /// A range of `start..end`.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The range shortened to its first and last non-whitespace characters.
    ///
    /// Returns `None` when the range is not on `char` boundaries of `text` or
    /// lies outside it. An all-whitespace range trims to an empty range, which
    /// can never match a valid span.
    pub fn trimmed(self, text: &str) -> Option<Self> {
        let slice = text.get(self.start..self.end)?;
        let leading = slice.len() - slice.trim_start().len();
        let trailing = slice.trim_end().len();
        Some(Self {
            start: self.start + leading,
            end: self.start + trailing,
        })
    }

    /// The text this range covers, when the range is valid.
    pub fn text(self, text: &str) -> Option<&str> {
        text.get(self.start..self.end)
    }

    /// The number of bytes in the range.
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the range contains no bytes.
    pub fn is_empty(self) -> bool {
        self.start >= self.end
    }

    /// The number of bytes two ranges share.
    pub fn intersection(self, other: Self) -> usize {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        end.saturating_sub(start)
    }

    /// The number of bytes covered by either range.
    pub fn union(self, other: Self) -> usize {
        let start = self.start.min(other.start);
        let end = self.end.max(other.end);
        end.saturating_sub(start)
    }

    /// Overlap divided by union, in `0.0..=1.0`.
    pub fn iou(self, other: Self) -> f64 {
        let union = self.union(other);
        if union == 0 {
            0.0
        } else {
            self.intersection(other) as f64 / union as f64
        }
    }
}

/// One span predicted by the parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PredictedSpan {
    pub range: ByteRange,
}

/// One label predicted on a span, by span index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredictedLabel {
    pub kind: AnnotationKind,
    /// Index into [`Prediction::spans`].
    pub span: usize,
    /// Only set for `question` or `task` labels.
    pub assignee: Option<String>,
}

/// One predicted `answer` relationship, by span index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PredictedAnswerLink {
    /// The answering span.
    pub from: usize,
    /// The question span.
    pub to: usize,
}

/// The scoreable part of one parse result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prediction {
    pub spans: Vec<PredictedSpan>,
    pub labels: Vec<PredictedLabel>,
    pub answer_links: Vec<PredictedAnswerLink>,
    pub recommendation_links: Vec<PredictedAnswerLink>,
    pub alternative_links: Vec<PredictedAnswerLink>,
}

impl Prediction {
    /// Extract byte ranges, labels, and links from a validated parse result.
    ///
    /// Labels whose nodes are not span nodes are ignored; Scribe never emits
    /// them, so that only keeps artificially constructed results scoreable.
    pub fn from_parse_result(result: &ParseResult) -> Self {
        let span_index =
            |node_id: &str| result.spans.iter().position(|span| span.node_id == node_id);

        let spans = result
            .spans
            .iter()
            .map(|span| PredictedSpan {
                range: ByteRange::new(span.start_byte, span.end_byte),
            })
            .collect();

        let mut labels = Vec::new();
        let mut answer_links = Vec::new();
        let mut recommendation_links = Vec::new();
        let mut alternative_links = Vec::new();
        for label in &result.labels {
            let Some(source) = span_index(&label.src_node_id) else {
                continue;
            };
            match label.kind {
                LabelKind::Question => labels.push(PredictedLabel {
                    kind: AnnotationKind::Question,
                    span: source,
                    assignee: label.assignee.clone(),
                }),
                LabelKind::Answer => {
                    let Some(target) = label.dst_node_id.as_deref().and_then(span_index) else {
                        continue;
                    };
                    labels.push(PredictedLabel {
                        kind: AnnotationKind::Answer,
                        span: source,
                        assignee: None,
                    });
                    answer_links.push(PredictedAnswerLink {
                        from: source,
                        to: target,
                    });
                }
                LabelKind::Recommendation | LabelKind::Task => labels.push(PredictedLabel {
                    kind: if label.kind == LabelKind::Task {
                        AnnotationKind::Task
                    } else {
                        AnnotationKind::Recommendation
                    },
                    span: source,
                    assignee: label.assignee.clone(),
                }),
                LabelKind::RecommendationFor | LabelKind::AlternativeTo => {
                    if let Some(target) = label.dst_node_id.as_deref().and_then(span_index) {
                        let links = if label.kind == LabelKind::AlternativeTo {
                            &mut alternative_links
                        } else {
                            &mut recommendation_links
                        };
                        links.push(PredictedAnswerLink {
                            from: source,
                            to: target,
                        });
                    }
                }
                LabelKind::Conflict | LabelKind::Supersedes | LabelKind::Stale => {}
            }
        }

        Self {
            spans,
            labels,
            answer_links,
            recommendation_links,
            alternative_links,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming_strips_whitespace_on_both_sides() {
        let text = "  Question?  ";
        assert_eq!(
            ByteRange::new(0, 13).trimmed(text),
            Some(ByteRange::new(2, 11))
        );
        assert_eq!(
            ByteRange::new(2, 11).trimmed(text),
            Some(ByteRange::new(2, 11))
        );
    }

    #[test]
    fn trimming_rejects_out_of_bounds_and_split_characters() {
        let text = "aé";
        assert_eq!(ByteRange::new(0, 4).trimmed(text), None);
        assert_eq!(ByteRange::new(2, 3).trimmed(text), None);
    }

    #[test]
    fn iou_uses_overlap_over_union() {
        let left = ByteRange::new(0, 10);
        let right = ByteRange::new(5, 15);
        assert!((left.iou(right) - 1.0 / 3.0).abs() < f64::EPSILON);
        assert_eq!(left.iou(left), 1.0);
        assert_eq!(left.iou(ByteRange::new(20, 30)), 0.0);
    }
}
