//! Deterministic view logic: submission state and source-ordered blocks.
//!
//! This module has no akar or HTTP dependency, so the rules that matter for the
//! experiment can be tested without a GPU or a model: one submission at a time,
//! each result replacing the previous one, and blocks derived from the original
//! payload and API byte ranges only. Text is never paraphrased in the GUI.

use std::collections::HashMap;

use daftprompt_types::{Label, LabelKind, LabelStatus, ParseResult, SourceKind};

use crate::api::ApiError;

/// Which state the parse pane is in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Loading,
    Success,
    Error,
}

/// The single-submission state machine.
///
/// The API is stateless and each parse replaces the previous result, so the
/// GUI keeps at most one result and never appends a chat transcript.
#[derive(Debug, Default)]
pub struct ParseView {
    phase: Phase,
    result: Option<ParseResult>,
    error: Option<ApiError>,
    next_submission: u64,
    active_submission: Option<u64>,
}

impl ParseView {
    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn result(&self) -> Option<&ParseResult> {
        self.result.as_ref()
    }

    pub fn error(&self) -> Option<&ApiError> {
        self.error.as_ref()
    }

    pub fn is_loading(&self) -> bool {
        self.phase == Phase::Loading
    }

    /// Start one submission and return its id, or `None` while another is in
    /// flight. Overlapping submissions are deliberately refused.
    pub fn begin(&mut self) -> Option<u64> {
        if self.is_loading() {
            return None;
        }
        self.next_submission = self.next_submission.wrapping_add(1);
        let submission = self.next_submission;
        self.phase = Phase::Loading;
        self.result = None;
        self.error = None;
        self.active_submission = Some(submission);
        Some(submission)
    }

    /// Apply the outcome of `submission`. Returns whether it was current;
    /// stale results from an earlier request are ignored.
    pub fn finish(&mut self, submission: u64, outcome: Result<ParseResult, ApiError>) -> bool {
        if self.active_submission != Some(submission) {
            return false;
        }
        self.active_submission = None;
        match outcome {
            Ok(result) => {
                self.phase = Phase::Success;
                self.result = Some(result);
                self.error = None;
            }
            Err(error) => {
                self.phase = Phase::Error;
                self.result = None;
                self.error = Some(error);
            }
        }
        true
    }

    /// Show a client-side failure, such as empty input. Never interrupts an
    /// in-flight submission.
    pub fn fail_local(&mut self, error: ApiError) -> bool {
        if self.is_loading() {
            return false;
        }
        self.phase = Phase::Error;
        self.result = None;
        self.error = Some(error);
        true
    }
}

/// One label as the source view needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelView {
    pub kind: LabelKind,
    /// `None` for non-blocking kinds; questions carry `open` or `resolved`.
    pub status: Option<LabelStatus>,
    pub assignee: Option<String>,
    /// The question a decoded `answer` points at, shown on the answer block.
    pub target: Option<String>,
}

/// One validated span, rendered from its exact source range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanView {
    pub display_id: u32,
    pub node_id: String,
    pub start_byte: usize,
    pub end_byte: usize,
    /// The exact payload slice `[start_byte, end_byte)`, never a paraphrase.
    pub text: String,
    pub labels: Vec<LabelView>,
    /// Answer spans that point at this question, if any.
    pub answered_by: Vec<String>,
    pub recommended_by: Vec<String>,
    pub alternatives: Vec<String>,
}

/// A source-ordered piece of the original text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Text outside every validated span. Unlabelled text is valid.
    Text { text: String },
    /// A validated span with its labels.
    Span(SpanView),
}

const TARGET_PREVIEW_CHARS: usize = 96;

/// Build display blocks from the API's payload and half-open byte ranges.
///
/// The API already validated ranges, but the GUI still checks bounds and char
/// boundaries so a malformed response cannot panic the renderer. Overlapping
/// spans are skipped defensively; core rejects them before they reach here.
pub fn build_blocks(result: &ParseResult) -> Vec<Block> {
    let content = &result.payload.content;
    let mut spans: Vec<_> = result
        .spans
        .iter()
        .filter(|span| {
            span.start_byte < span.end_byte
                && span.end_byte <= content.len()
                && content.is_char_boundary(span.start_byte)
                && content.is_char_boundary(span.end_byte)
        })
        .collect();
    spans.sort_by_key(|span| (span.start_byte, span.end_byte));

    // Reject ambiguous identities without inventing replacement numbers.
    let mut ids = HashMap::new();
    let mut nodes = HashMap::new();
    for span in &spans {
        *ids.entry(span.display_id).or_insert(0) += 1;
        *nodes.entry(span.node_id.as_str()).or_insert(0) += 1;
    }
    spans.retain(|span| {
        span.display_id > 0 && ids[&span.display_id] == 1 && nodes[span.node_id.as_str()] == 1
    });
    let mut end = 0;
    spans.retain(|span| {
        if span.start_byte < end {
            return false;
        }
        end = span.end_byte;
        true
    });
    let node_text: HashMap<&str, String> = spans
        .iter()
        .map(|span| {
            (
                span.node_id.as_str(),
                format!(
                    "#{} · {}",
                    span.display_id,
                    truncate_for_display(
                        &content[span.start_byte..span.end_byte],
                        TARGET_PREVIEW_CHARS
                    )
                ),
            )
        })
        .collect();

    let labels_by_node = labels_by_source_node(&result.labels);

    let mut blocks = Vec::new();
    let mut cursor = 0;

    for span in spans {
        if span.start_byte < cursor {
            // Overlap: keep the earlier block and ignore the malformed one.
            continue;
        }

        let gap = &content[cursor..span.start_byte];
        if !gap.is_empty() {
            blocks.push(Block::Text {
                text: gap.to_owned(),
            });
        }

        let text = &content[span.start_byte..span.end_byte];
        let labels = labels_by_node
            .get(span.node_id.as_str())
            .map(|labels| {
                labels
                    .iter()
                    .map(|label| label_view(label, &node_text))
                    .collect()
            })
            .unwrap_or_default();

        let incoming = |kind| {
            result
                .labels
                .iter()
                .filter(|label| {
                    label.kind == kind
                        && label.dst_node_id.as_deref() == Some(span.node_id.as_str())
                })
                .filter_map(|label| node_text.get(label.src_node_id.as_str()).cloned())
                .collect()
        };
        let answered_by = incoming(LabelKind::Answer);
        let recommended_by = incoming(LabelKind::RecommendationFor);
        let alternatives = result
            .labels
            .iter()
            .filter(|label| label.kind == LabelKind::AlternativeTo)
            .filter_map(|label| {
                if label.src_node_id == span.node_id {
                    label.dst_node_id.as_deref()
                } else if label.dst_node_id.as_deref() == Some(span.node_id.as_str()) {
                    Some(label.src_node_id.as_str())
                } else {
                    None
                }
            })
            .filter_map(|id| node_text.get(id).cloned())
            .collect();

        blocks.push(Block::Span(SpanView {
            display_id: span.display_id,
            node_id: span.node_id.clone(),
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            text: text.to_owned(),
            labels,
            answered_by,
            recommended_by,
            alternatives,
        }));
        cursor = span.end_byte;
    }

    let rest = &content[cursor..];
    if !rest.is_empty() {
        blocks.push(Block::Text {
            text: rest.to_owned(),
        });
    }

    blocks
}

fn labels_by_source_node(labels: &[Label]) -> HashMap<&str, Vec<&Label>> {
    let mut grouped: HashMap<&str, Vec<&Label>> = HashMap::new();
    for label in labels {
        grouped
            .entry(label.src_node_id.as_str())
            .or_default()
            .push(label);
    }
    grouped
}

fn label_view(label: &Label, node_text: &HashMap<&str, String>) -> LabelView {
    let target = if matches!(label.kind, LabelKind::Answer | LabelKind::RecommendationFor) {
        label
            .dst_node_id
            .as_deref()
            .and_then(|node_id| node_text.get(node_id))
            .cloned()
            .or_else(|| Some("unavailable target".to_owned()))
    } else {
        None
    };

    LabelView {
        kind: label.kind,
        status: label.status,
        assignee: label.assignee.clone(),
        target,
    }
}

/// Collapse whitespace and shorten a source excerpt for a badge target line.
pub fn truncate_for_display(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let mut shortened: String = collapsed.chars().take(max_chars - 1).collect();
    shortened.push('…');
    shortened
}

/// A compact run/provenance line for the main pane header.
pub fn result_summary(result: &ParseResult) -> String {
    let provider = result.run.provider.as_deref().unwrap_or("?");
    let model = result.run.model.as_deref().unwrap_or("?");
    let attempts = result.run.attempts;
    let mut parts = vec![
        result.run.agent.clone(),
        format!("{provider}/{model}"),
        format!(
            "{} span(s), {} label(s)",
            result.spans.len(),
            result.labels.len()
        ),
        format!("{attempts} attempt(s)"),
    ];
    if let Some(source) = &result.source {
        let mut description = source
            .kind
            .map(source_kind_name)
            .unwrap_or("unspecified source")
            .to_owned();
        if let Some(reference) = &source.reference {
            description.push_str(" (");
            description.push_str(reference);
            description.push(')');
        }
        parts.push(description);
    }
    parts.join(" · ")
}

fn source_kind_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Chat => "chat",
        SourceKind::File => "file",
        SourceKind::Clipboard => "clipboard",
        SourceKind::Email => "email",
        SourceKind::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daftprompt_types::{
        ContextTarget, Label, Node, NodeContext, ParseResult, Payload, Run, RunInput, RunStatus,
        SpanInfo, ToolCall,
    };

    fn sample_result() -> ParseResult {
        let text = "Intro\nQ?\nA\n";
        let payload = Payload {
            hash: "hash".to_owned(),
            content: text.to_owned(),
            created_at: "now".to_owned(),
        };
        let root = Node {
            id: "root".to_owned(),
            parent_id: None,
            author: "unknown".to_owned(),
            run_id: None,
            created_at: "now".to_owned(),
        };
        let question_id = "q";
        let answer_id = "a";

        let labels = vec![
            Label {
                id: "label-q".to_owned(),
                kind: LabelKind::Question,
                src_node_id: question_id.to_owned(),
                dst_node_id: None,
                status: Some(LabelStatus::Resolved),
                assignee: Some("@infra-team".to_owned()),
                note_node_id: None,
                created_by: "agent".to_owned(),
                run_id: Some("run".to_owned()),
                created_at: "now".to_owned(),
            },
            Label {
                id: "label-a".to_owned(),
                kind: LabelKind::Answer,
                src_node_id: answer_id.to_owned(),
                dst_node_id: Some(question_id.to_owned()),
                status: None,
                assignee: None,
                note_node_id: None,
                created_by: "agent".to_owned(),
                run_id: Some("run".to_owned()),
                created_at: "now".to_owned(),
            },
        ];

        ParseResult {
            payload,
            source: None,
            nodes: vec![
                root,
                Node {
                    id: question_id.to_owned(),
                    parent_id: Some("root".to_owned()),
                    author: "agent".to_owned(),
                    run_id: Some("run".to_owned()),
                    created_at: "now".to_owned(),
                },
                Node {
                    id: answer_id.to_owned(),
                    parent_id: Some("root".to_owned()),
                    author: "agent".to_owned(),
                    run_id: Some("run".to_owned()),
                    created_at: "now".to_owned(),
                },
            ],
            node_contexts: vec![NodeContext {
                id: "ctx".to_owned(),
                node_id: question_id.to_owned(),
                seq: 0,
                target: ContextTarget::Payload {
                    payload_hash: "hash".to_owned(),
                },
                range: None,
            }],
            labels,
            spans: vec![
                SpanInfo {
                    display_id: 1,
                    node_id: question_id.to_owned(),
                    start_byte: 6,
                    end_byte: 8,
                },
                SpanInfo {
                    display_id: 2,
                    node_id: answer_id.to_owned(),
                    start_byte: 9,
                    end_byte: 10,
                },
            ],
            run: Run {
                id: "run".to_owned(),
                agent: "scribe".to_owned(),
                provider: Some("openai".to_owned()),
                model: Some("gpt-4o-mini".to_owned()),
                prompt_version: Some("scribe-v1".to_owned()),
                started_at: "now".to_owned(),
                ended_at: Some("now".to_owned()),
                status: RunStatus::Done,
                attempts: 1,
            },
            run_inputs: vec![RunInput {
                run_id: "run".to_owned(),
                node_id: "root".to_owned(),
            }],
            tool_calls: vec![ToolCall {
                id: "tool".to_owned(),
                run_id: "run".to_owned(),
                seq: 0,
                tool: "submit".to_owned(),
                input: "{}".to_owned(),
                output_hash: None,
                created_at: "now".to_owned(),
            }],
        }
    }

    #[test]
    fn source_order_keeps_unlabelled_text_between_spans() {
        let blocks = build_blocks(&sample_result());
        assert_eq!(blocks.len(), 5);
        assert_eq!(
            blocks[0],
            Block::Text {
                text: "Intro\n".to_owned()
            }
        );
        let Block::Span(question) = &blocks[1] else {
            panic!("expected the question span");
        };
        assert_eq!(question.text, "Q?");
        assert_eq!(question.node_id, "q");
        assert_eq!(question.labels[0].status, Some(LabelStatus::Resolved));
        assert_eq!(question.labels[0].assignee.as_deref(), Some("@infra-team"));
        assert_eq!(question.answered_by, vec!["#2 · A".to_owned()]);

        let Block::Span(answer) = &blocks[3] else {
            panic!("expected the answer span");
        };
        assert_eq!(answer.text, "A");
        assert_eq!(answer.labels[0].kind, LabelKind::Answer);
        assert_eq!(answer.labels[0].target.as_deref(), Some("#1 · Q?"));
    }

    #[test]
    fn whitespace_only_gaps_preserve_original_text() {
        let result = sample_result();
        let reconstructed: String = build_blocks(&result)
            .iter()
            .map(|block| match block {
                Block::Text { text } => text.as_str(),
                Block::Span(span) => span.text.as_str(),
            })
            .collect();
        assert_eq!(reconstructed, result.payload.content);
    }

    #[test]
    fn out_of_bounds_and_overlapping_ranges_are_ignored() {
        let mut result = sample_result();
        result.spans.push(SpanInfo {
            display_id: 3,
            node_id: "bad-boundary".to_owned(),
            start_byte: 7,
            end_byte: 9,
        });
        result.spans.push(SpanInfo {
            display_id: 1,
            node_id: "out-of-bounds".to_owned(),
            start_byte: 0,
            end_byte: 99,
        });
        let blocks = build_blocks(&result);
        // The valid spans still render, and the malformed ones add no block.
        assert_eq!(blocks.len(), 5);
    }

    #[test]
    fn byte_ranges_must_fall_on_char_boundaries() {
        let mut result = sample_result();
        result.payload.content = "aé\n".to_owned();
        result.labels.clear();
        // "é" occupies bytes 1..3; byte 2 is inside the character.
        result.spans = vec![SpanInfo {
            display_id: 1,
            node_id: "bad".to_owned(),
            start_byte: 1,
            end_byte: 2,
        }];
        let blocks = build_blocks(&result);
        assert!(
            blocks.iter().all(|block| !matches!(block, Block::Span(_))),
            "a range inside a char must not become a span"
        );

        result.spans = vec![SpanInfo {
            display_id: 1,
            node_id: "good".to_owned(),
            start_byte: 1,
            end_byte: 3,
        }];
        let blocks = build_blocks(&result);
        let span = blocks
            .iter()
            .find_map(|block| match block {
                Block::Span(span) => Some(span),
                Block::Text { .. } => None,
            })
            .expect("expected the valid Unicode span");
        assert_eq!(span.text, "é");
    }

    #[test]
    fn unlabelled_only_text_is_kept_verbatim() {
        let mut result = sample_result();
        result.payload.content = "Just some notes.\nNothing to do here.".to_owned();
        result.spans.clear();
        result.labels.clear();
        assert_eq!(
            build_blocks(&result),
            vec![Block::Text {
                text: "Just some notes.\nNothing to do here.".to_owned()
            }]
        );
    }

    #[test]
    fn overlapping_spans_keep_only_the_first_block() {
        let mut result = sample_result();
        result.spans.push(SpanInfo {
            display_id: 3,
            node_id: "overlap".to_owned(),
            start_byte: 6,
            end_byte: 9,
        });
        let blocks = build_blocks(&result);
        let span_count = blocks
            .iter()
            .filter(|block| matches!(block, Block::Span(_)))
            .count();
        assert_eq!(span_count, 2);
    }

    #[test]
    fn whitespace_only_text_is_preserved() {
        let mut result = sample_result();
        result.payload.content = "  \n".to_owned();
        result.spans.clear();
        result.labels.clear();
        assert_eq!(
            build_blocks(&result),
            vec![Block::Text {
                text: "  \n".to_owned()
            }]
        );
    }

    #[test]
    fn new_relationships_use_api_numbers_and_preserve_source() {
        let mut result = sample_result();
        result.payload.content.push_str("R\nT");
        result.spans.extend([
            SpanInfo {
                display_id: 3,
                node_id: "r".into(),
                start_byte: 11,
                end_byte: 12,
            },
            SpanInfo {
                display_id: 4,
                node_id: "t".into(),
                start_byte: 13,
                end_byte: 14,
            },
        ]);
        let base = result.labels[1].clone();
        for (kind, src, dst) in [
            (LabelKind::Recommendation, "a", None),
            (LabelKind::Recommendation, "r", None),
            (LabelKind::RecommendationFor, "r", Some("q")),
            (LabelKind::AlternativeTo, "a", Some("r")),
            (LabelKind::Task, "t", None),
        ] {
            let mut label = base.clone();
            label.kind = kind;
            label.src_node_id = src.into();
            label.dst_node_id = dst.map(str::to_owned);
            if kind == LabelKind::Task {
                label.status = Some(LabelStatus::Open);
                label.assignee = Some("@alex".into());
            }
            result.labels.push(label);
        }
        result.spans.reverse();
        let blocks = build_blocks(&result);
        let spans: Vec<_> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Span(span) => Some(span),
                _ => None,
            })
            .collect();
        assert_eq!(
            spans.iter().map(|span| span.display_id).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(spans[0].answered_by, ["#2 · A"]);
        assert_eq!(spans[0].recommended_by, ["#3 · R"]);
        assert_eq!(spans[1].alternatives, ["#3 · R"]);
        assert_eq!(spans[2].alternatives, ["#2 · A"]);
        assert_eq!(spans[2].labels[1].target.as_deref(), Some("#1 · Q?"));
        assert!(
            spans[1]
                .labels
                .iter()
                .find(|l| l.kind == LabelKind::Recommendation)
                .unwrap()
                .target
                .is_none()
        );
        assert_eq!(spans[3].labels[0].assignee.as_deref(), Some("@alex"));
        let source: String = blocks
            .iter()
            .map(|block| match block {
                Block::Text { text } => text.as_str(),
                Block::Span(span) => span.text.as_str(),
            })
            .collect();
        assert_eq!(source, result.payload.content);
    }

    #[test]
    fn malformed_identifiers_are_never_replaced() {
        let mut result = sample_result();
        result.spans[0].display_id = 0;
        let blocks = build_blocks(&result);
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, Block::Span(span) if span.display_id == 2))
        );
        assert!(
            !blocks
                .iter()
                .any(|block| matches!(block, Block::Span(span) if span.node_id == "q"))
        );
        result.spans[0].display_id = 2;
        assert!(
            build_blocks(&result)
                .iter()
                .all(|block| matches!(block, Block::Text { .. }))
        );
    }

    #[test]
    fn live_submission_starts_idle_and_refuses_overlap() {
        let mut view = ParseView::default();
        assert_eq!(view.phase(), Phase::Idle);
        assert!(view.result().is_none());

        let first = view.begin().expect("first submission");
        assert_eq!(view.phase(), Phase::Loading);
        assert!(view.begin().is_none(), "overlap must be refused");
        assert!(!view.fail_local(ApiError::local("empty")), "loading wins");

        assert!(view.finish(first, Ok(sample_result())));
        assert_eq!(view.phase(), Phase::Success);
        assert!(view.result().is_some());
    }

    #[test]
    fn each_finished_result_replaces_the_previous_one() {
        let mut view = ParseView::default();
        let first = view.begin().unwrap();
        view.finish(first, Ok(sample_result()));
        let second = view.begin().unwrap();
        assert!(view.result().is_none(), "old result is cleared on submit");
        view.finish(second, Err(ApiError::connection("down")));
        assert_eq!(view.phase(), Phase::Error);
        assert!(view.result().is_none());
        assert_eq!(view.error().unwrap().kind, "connection");

        let third = view.begin().unwrap();
        view.finish(third, Ok(sample_result()));
        assert_eq!(view.phase(), Phase::Success);
        assert!(view.error().is_none());
    }

    #[test]
    fn stale_outcomes_are_ignored() {
        let mut view = ParseView::default();
        let first = view.begin().unwrap();
        view.finish(first, Ok(sample_result()));
        let second = view.begin().unwrap();
        assert!(!view.finish(first, Err(ApiError::connection("stale"))));
        assert_eq!(view.phase(), Phase::Loading);
        assert!(view.finish(second, Ok(sample_result())));
    }

    #[test]
    fn local_errors_only_apply_when_idle_or_finished() {
        let mut view = ParseView::default();
        assert!(view.fail_local(ApiError::local("Enter some text")));
        assert_eq!(view.phase(), Phase::Error);
        assert_eq!(view.error().unwrap().kind, "invalid_request");

        let id = view.begin().unwrap();
        view.finish(id, Ok(sample_result()));
        assert!(view.fail_local(ApiError::local("again")));
        assert_eq!(view.phase(), Phase::Error);
    }

    #[test]
    fn display_excerpts_collapse_whitespace_and_shorten() {
        assert_eq!(
            truncate_for_display("  Should we\n cache   tokens? ", 96),
            "Should we cache tokens?"
        );
        assert_eq!(truncate_for_display("abcdef", 4), "abc…");
        assert_eq!(truncate_for_display("abcdef", 0), "");
    }

    #[test]
    fn summary_records_the_run_and_provenance() {
        let mut result = sample_result();
        result.source = Some(daftprompt_types::SourceMetadata {
            kind: Some(SourceKind::Chat),
            reference: Some("thread-1".to_owned()),
            author: Some("ada".to_owned()),
        });
        assert_eq!(
            result_summary(&result),
            "scribe · openai/gpt-4o-mini · 2 span(s), 2 label(s) · 1 attempt(s) · chat (thread-1)"
        );
    }
}
