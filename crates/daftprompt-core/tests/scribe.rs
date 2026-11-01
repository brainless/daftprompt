//! Deterministic end-to-end tests for Scribe.
//!
//! Every test supplies a stub [`ExtractionModel`]; no credentials, network, or
//! real model is involved. The tests cover graph construction, validation,
//! limits, retries, and explicit failure modes.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use daftprompt_core::{
    BoxFuture, ExtractionModel, ModelError, ModelInput, Parser, Scribe, ScribeConfig,
};
use daftprompt_types::{
    AnnotationKind, ContextTarget, ExtractedAnnotation, ExtractedSpan, Extraction, Label,
    LabelKind, LabelStatus, Node, NodeContext, ParseError, ParseRequest, ParseResult, RunStatus,
    SourceKind, SourceMetadata, SourceRange, SpanInfo,
};

// ---------------------------------------------------------------------------
// Stub model
// ---------------------------------------------------------------------------

struct StubModel {
    script: Mutex<VecDeque<Result<Extraction, ModelError>>>,
    calls: AtomicUsize,
    inputs: Mutex<Vec<String>>,
    pending: bool,
}

impl StubModel {
    fn new(script: Vec<Result<Extraction, ModelError>>) -> Self {
        Self {
            script: Mutex::new(script.into()),
            calls: AtomicUsize::new(0),
            inputs: Mutex::new(Vec::new()),
            pending: false,
        }
    }

    fn valid(extraction: Extraction) -> Self {
        Self::new(vec![Ok(extraction)])
    }

    fn pending() -> Self {
        Self {
            script: Mutex::new(VecDeque::new()),
            calls: AtomicUsize::new(0),
            inputs: Mutex::new(Vec::new()),
            pending: true,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn inputs(&self) -> Vec<String> {
        self.inputs.lock().expect("inputs lock").clone()
    }
}

impl ExtractionModel for StubModel {
    fn extract(&self, input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inputs
            .lock()
            .expect("inputs lock")
            .push(input.segments);

        if self.pending {
            return Box::pin(std::future::pending());
        }

        let next = self
            .script
            .lock()
            .expect("script lock")
            .pop_front()
            .unwrap_or_else(|| Err(ModelError::Invalid("stub script exhausted".to_owned())));
        Box::pin(std::future::ready(next))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn config() -> ScribeConfig {
    ScribeConfig::new()
        .with_provider("test-provider")
        .with_model("stub-model")
}

fn span(id: &str, start: u32, end: u32) -> ExtractedSpan {
    ExtractedSpan {
        id: id.to_owned(),
        start_segment: start,
        end_segment: end,
        quote: None,
    }
}

fn quoted(id: &str, start: u32, end: u32, quote: &str) -> ExtractedSpan {
    ExtractedSpan {
        id: id.to_owned(),
        start_segment: start,
        end_segment: end,
        quote: Some(quote.to_owned()),
    }
}

fn question(span: &str) -> ExtractedAnnotation {
    ExtractedAnnotation {
        kind: AnnotationKind::Question,
        span: span.to_owned(),
        questions: Vec::new(),
        alternatives: Vec::new(),
        answers: Vec::new(),
        assignee: None,
    }
}

fn assigned(span: &str, assignee: &str) -> ExtractedAnnotation {
    ExtractedAnnotation {
        assignee: Some(assignee.to_owned()),
        ..question(span)
    }
}

fn answer(span: &str, targets: &[&str]) -> ExtractedAnnotation {
    ExtractedAnnotation {
        kind: AnnotationKind::Answer,
        span: span.to_owned(),
        questions: Vec::new(),
        alternatives: Vec::new(),
        answers: targets.iter().map(|target| (*target).to_owned()).collect(),
        assignee: None,
    }
}

fn extraction(spans: Vec<ExtractedSpan>, annotations: Vec<ExtractedAnnotation>) -> Extraction {
    Extraction { spans, annotations }
}

fn root(result: &ParseResult) -> &Node {
    result
        .nodes
        .iter()
        .find(|node| node.parent_id.is_none())
        .expect("a root node")
}

fn context_for<'a>(result: &'a ParseResult, node_id: &str) -> &'a NodeContext {
    result
        .node_contexts
        .iter()
        .find(|context| context.node_id == node_id)
        .expect("context exists")
}

fn labels_of(result: &ParseResult, kind: LabelKind) -> Vec<&Label> {
    result
        .labels
        .iter()
        .filter(|label| label.kind == kind)
        .collect()
}

fn span_node(result: &ParseResult) -> &Node {
    result
        .nodes
        .iter()
        .find(|node| node.parent_id.is_some())
        .expect("a span node")
}

fn payload_hash_of(context: &NodeContext) -> &str {
    match &context.target {
        ContextTarget::Payload { payload_hash } => payload_hash,
        ContextTarget::File { .. } => panic!("span contexts target the root payload"),
    }
}

// ---------------------------------------------------------------------------
// Graph construction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_extraction_builds_payload_root_and_run_metadata() {
    let scribe = Scribe::new(config(), StubModel::valid(Extraction::default()));
    let result = scribe
        .parse(ParseRequest::new("Just some notes.\nNo questions."))
        .await
        .expect("empty extraction is valid");

    assert_eq!(result.payload.content, "Just some notes.\nNo questions.");
    assert_eq!(result.payload.hash.len(), 64);
    assert!(result.payload.hash.chars().all(|c| c.is_ascii_hexdigit()));

    assert_eq!(result.nodes.len(), 1);
    let root = root(&result);
    assert_eq!(root.parent_id, None);
    assert_eq!(root.author, "unknown");
    assert_eq!(root.run_id, None);

    assert_eq!(result.node_contexts.len(), 1);
    let context = &result.node_contexts[0];
    assert_eq!(context.node_id, root.id);
    assert_eq!(context.seq, 0);
    assert_eq!(context.range, None);
    assert_eq!(payload_hash_of(context), result.payload.hash);

    assert!(result.labels.is_empty());
    assert!(result.spans.is_empty());
    assert_eq!(result.source, None);

    assert_eq!(result.run.agent, "scribe");
    assert_eq!(result.run.provider.as_deref(), Some("test-provider"));
    assert_eq!(result.run.model.as_deref(), Some("stub-model"));
    assert_eq!(result.run.prompt_version.as_deref(), Some("scribe-v3"));
    assert_eq!(result.run.status, RunStatus::Done);
    assert_eq!(result.run.attempts, 1);
    assert!(result.run.ended_at.is_some());

    assert_eq!(result.run_inputs.len(), 1);
    assert_eq!(result.run_inputs[0].node_id, root.id);
    assert_eq!(result.run_inputs[0].run_id, result.run.id);

    assert_eq!(result.tool_calls.len(), 1);
    let tool_call = &result.tool_calls[0];
    assert_eq!(tool_call.tool, "submit");
    assert_eq!(tool_call.seq, 0);
    assert_eq!(tool_call.run_id, result.run.id);
    assert_eq!(tool_call.output_hash, None);
    let submitted: Extraction =
        serde_json::from_str(&tool_call.input).expect("tool call input is extraction JSON");
    assert_eq!(submitted, Extraction::default());
}

#[tokio::test]
async fn source_metadata_supplies_the_root_author_and_is_preserved() {
    let source = SourceMetadata {
        kind: Some(SourceKind::Email),
        reference: Some("thread-42".to_owned()),
        author: Some("ada".to_owned()),
    };
    let scribe = Scribe::new(config(), StubModel::valid(Extraction::default()));
    let result = scribe
        .parse(ParseRequest::with_source(
            "Quoted email body.",
            source.clone(),
        ))
        .await
        .expect("parse succeeds");

    assert_eq!(root(&result).author, "ada");
    assert_eq!(result.source, Some(source));
}

#[tokio::test]
async fn blank_source_author_falls_back_to_unknown() {
    let source = SourceMetadata {
        author: Some("   ".to_owned()),
        ..SourceMetadata::default()
    };
    let scribe = Scribe::new(config(), StubModel::valid(Extraction::default()));
    let result = scribe
        .parse(ParseRequest::with_source("Text.", source))
        .await
        .expect("parse succeeds");

    assert_eq!(root(&result).author, "unknown");
}

#[tokio::test]
async fn question_span_becomes_a_child_node_with_a_range_context() {
    let text = "Plan\n\nShould we cache tokens?\n";
    let proposed = extraction(
        vec![quoted("s1", 3, 3, "Should we cache tokens?")],
        vec![assigned("s1", "@infra-team")],
    );

    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(text))
        .await
        .expect("parse succeeds");

    let root = root(&result);
    let span_node = span_node(&result);
    assert_eq!(span_node.parent_id.as_deref(), Some(root.id.as_str()));
    assert_eq!(span_node.author, "agent");
    assert_eq!(span_node.run_id.as_deref(), Some(result.run.id.as_str()));

    let context = context_for(&result, &span_node.id);
    assert_eq!(
        context.range,
        Some(SourceRange {
            start_line: 3,
            start_col: 1,
            end_line: 3,
            end_col: 24,
        })
    );
    assert_eq!(payload_hash_of(context), result.payload.hash);

    assert_eq!(
        result.spans,
        vec![SpanInfo {
            display_id: 1,
            node_id: span_node.id.clone(),
            start_byte: 6,
            end_byte: 29,
        }]
    );

    assert_eq!(result.labels.len(), 1);
    let label = &result.labels[0];
    assert_eq!(label.kind, LabelKind::Question);
    assert_eq!(label.src_node_id, span_node.id);
    assert_eq!(label.dst_node_id, None);
    assert_eq!(label.status, Some(LabelStatus::Open));
    assert_eq!(label.assignee.as_deref(), Some("@infra-team"));
    assert_eq!(label.note_node_id, None);
    assert_eq!(label.created_by, "agent");
    assert_eq!(label.run_id.as_deref(), Some(result.run.id.as_str()));
}

#[tokio::test]
async fn answer_links_resolve_question_status() {
    let text = "Q: Should we cache tokens?\nA: Yes, cache them.\n";
    let proposed = extraction(
        vec![
            quoted("s1", 1, 1, "Should we cache tokens?"),
            span("s2", 2, 2),
        ],
        vec![question("s1"), answer("s2", &["s1"])],
    );

    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(text))
        .await
        .expect("parse succeeds");

    let question_label = labels_of(&result, LabelKind::Question)[0];
    assert_eq!(question_label.status, Some(LabelStatus::Resolved));

    let answer_label = labels_of(&result, LabelKind::Answer)[0];
    assert_eq!(answer_label.status, None);
    assert_eq!(answer_label.assignee, None);
    assert_eq!(answer_label.src_node_id, result.spans[1].node_id);
    assert_eq!(
        answer_label.dst_node_id.as_deref(),
        Some(result.spans[0].node_id.as_str())
    );

    // The answer span's context covers its trimmed segment.
    let answer_context = context_for(&result, &result.spans[1].node_id);
    assert_eq!(
        answer_context.range,
        Some(SourceRange {
            start_line: 2,
            start_col: 1,
            end_line: 2,
            end_col: 20,
        })
    );
}

#[tokio::test]
async fn one_answer_can_resolve_several_questions() {
    let text = "Q1?\nQ2?\nA: yes.\n";
    let proposed = extraction(
        vec![span("s1", 1, 1), span("s2", 2, 2), span("s3", 3, 3)],
        vec![question("s1"), question("s2"), answer("s3", &["s1", "s2"])],
    );

    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(text))
        .await
        .expect("parse succeeds");

    let questions = labels_of(&result, LabelKind::Question);
    assert_eq!(questions.len(), 2);
    assert!(
        questions
            .iter()
            .all(|label| label.status == Some(LabelStatus::Resolved))
    );
    assert_eq!(labels_of(&result, LabelKind::Answer).len(), 2);
}

#[tokio::test]
async fn only_answered_questions_are_resolved() {
    let text = "Q1?\nQ2?\nA: yes.\n";
    let proposed = extraction(
        vec![span("s1", 1, 1), span("s2", 2, 2), span("s3", 3, 3)],
        vec![question("s1"), question("s2"), answer("s3", &["s1"])],
    );

    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(text))
        .await
        .expect("parse succeeds");

    let questions = labels_of(&result, LabelKind::Question);
    let resolved = questions
        .iter()
        .find(|label| label.status == Some(LabelStatus::Resolved))
        .expect("one resolved question");
    let open = questions
        .iter()
        .find(|label| label.status == Some(LabelStatus::Open))
        .expect("one open question");
    assert_eq!(resolved.src_node_id, result.spans[0].node_id);
    assert_eq!(open.src_node_id, result.spans[1].node_id);
}

#[tokio::test]
async fn one_span_may_carry_a_question_and_an_answer() {
    let text = "Q: Is it fast?\nA: Yes. Should we measure more?\n";
    let proposed = extraction(
        vec![quoted("s1", 1, 1, "Is it fast?"), span("s2", 2, 2)],
        vec![question("s1"), answer("s2", &["s1"]), question("s2")],
    );

    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(text))
        .await
        .expect("parse succeeds");

    assert_eq!(labels_of(&result, LabelKind::Question).len(), 2);
    assert_eq!(labels_of(&result, LabelKind::Answer).len(), 1);

    let answer_label = labels_of(&result, LabelKind::Answer)[0];
    assert_eq!(answer_label.src_node_id, result.spans[1].node_id);
    assert_eq!(
        answer_label.dst_node_id.as_deref(),
        Some(result.spans[0].node_id.as_str())
    );

    let s2_question = labels_of(&result, LabelKind::Question)
        .into_iter()
        .find(|label| label.src_node_id == result.spans[1].node_id)
        .expect("the answer span is also a question");
    assert_eq!(s2_question.status, Some(LabelStatus::Open));
}

#[tokio::test]
async fn span_nodes_are_emitted_in_source_order() {
    let text = "one\ntwo\nthree\n";
    let proposed = extraction(
        vec![span("late", 3, 3), span("early", 1, 1)],
        vec![question("late"), question("early")],
    );

    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(text))
        .await
        .expect("parse succeeds");

    assert_eq!(result.spans.len(), 2);
    assert!(result.spans[0].start_byte < result.spans[1].start_byte);
    assert_eq!(result.spans[0].start_byte, 0);
    assert_eq!(result.spans[1].start_byte, 8);

    // Root, then the span starting at byte 0, then the one starting at byte 8.
    assert_eq!(result.nodes[1].id, result.spans[0].node_id);
    assert_eq!(result.nodes[2].id, result.spans[1].node_id);

    let first_context = context_for(&result, &result.spans[0].node_id);
    let second_context = context_for(&result, &result.spans[1].node_id);
    assert_eq!(first_context.range.unwrap().start_line, 1);
    assert_eq!(second_context.range.unwrap().start_line, 3);
}

#[tokio::test]
async fn text_is_stored_once_in_the_payload() {
    let text = "Question?\nAnswer.\n";
    let proposed = extraction(
        vec![span("s1", 1, 1), span("s2", 2, 2)],
        vec![question("s1"), answer("s2", &["s1"])],
    );

    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(text))
        .await
        .expect("parse succeeds");

    assert_eq!(result.payload.content, text);
    assert_eq!(result.nodes.len(), 3);
    assert_eq!(result.node_contexts.len(), 3);
    for context in &result.node_contexts {
        assert_eq!(payload_hash_of(context), result.payload.hash);
    }
}

#[tokio::test]
async fn accepted_submit_arguments_are_recorded_verbatim() {
    let proposed = extraction(
        vec![quoted("s1", 1, 1, "Question?")],
        vec![assigned("s1", "@legal-team")],
    );
    let scribe = Scribe::new(config(), StubModel::valid(proposed.clone()));
    let result = scribe
        .parse(ParseRequest::new("Question?"))
        .await
        .expect("parse succeeds");

    let submitted: Extraction =
        serde_json::from_str(&result.tool_calls[0].input).expect("valid JSON");
    assert_eq!(submitted, proposed);
}

// ---------------------------------------------------------------------------
// Coordinates and text shapes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unicode_spans_use_character_columns_and_char_boundaries() {
    let text = "héllo 😀\ncombining e\u{301} question?\n";
    let proposed = extraction(
        vec![
            quoted("emoji", 1, 1, "😀"),
            quoted("combining", 2, 2, "e\u{301}"),
        ],
        vec![question("emoji"), question("combining")],
    );

    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(text))
        .await
        .expect("parse succeeds");

    let emoji = result
        .spans
        .iter()
        .find(|span| span.start_byte == 7)
        .expect("emoji span starts after six one-column characters");
    assert_eq!((emoji.start_byte, emoji.end_byte), (7, 11));
    assert_eq!(
        context_for(&result, &emoji.node_id).range,
        Some(SourceRange {
            start_line: 1,
            start_col: 7,
            end_line: 1,
            end_col: 8,
        })
    );

    let combining = result
        .spans
        .iter()
        .find(|span| span.start_byte == 22)
        .expect("combining span");
    assert_eq!((combining.start_byte, combining.end_byte), (22, 25));
    assert_eq!(
        context_for(&result, &combining.node_id).range,
        Some(SourceRange {
            start_line: 2,
            start_col: 11,
            end_line: 2,
            end_col: 13,
        })
    );
}

#[tokio::test]
async fn crlf_is_preserved_and_carriage_returns_count_as_columns() {
    let text = "first\r\nsecond\r\n";
    let proposed = extraction(
        vec![quoted("s1", 1, 2, "first\r\nsecond")],
        vec![question("s1")],
    );

    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(text))
        .await
        .expect("parse succeeds");

    assert_eq!(result.payload.content, text);
    assert_eq!(
        result.spans,
        vec![SpanInfo {
            display_id: 1,
            node_id: result.spans[0].node_id.clone(),
            start_byte: 0,
            end_byte: 13,
        }]
    );
    assert_eq!(
        context_for(&result, &result.spans[0].node_id).range,
        Some(SourceRange {
            start_line: 1,
            start_col: 1,
            end_line: 2,
            end_col: 7,
        })
    );
}

#[tokio::test]
async fn multiline_spans_cover_the_segment_range() {
    let text = "Start\n  continue here\nend\n";
    let proposed = extraction(vec![span("s1", 1, 2)], vec![question("s1")]);

    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(text))
        .await
        .expect("parse succeeds");

    assert_eq!(
        result.spans,
        vec![SpanInfo {
            display_id: 1,
            node_id: result.spans[0].node_id.clone(),
            start_byte: 0,
            end_byte: 21,
        }]
    );
    assert_eq!(
        context_for(&result, &result.spans[0].node_id).range,
        Some(SourceRange {
            start_line: 1,
            start_col: 1,
            end_line: 2,
            end_col: 16,
        })
    );
}

#[tokio::test]
async fn repeated_text_anchors_only_with_an_unambiguous_quote() {
    // "foo" occurs twice in segments 1..=3, so the first attempt is invalid.
    let ambiguous = extraction(vec![quoted("s1", 1, 3, "foo")], vec![question("s1")]);
    // Restricting the range to segment 1 makes the same quote unique.
    let accepted = extraction(vec![quoted("s1", 1, 1, "foo")], vec![question("s1")]);

    let model = StubModel::new(vec![Ok(ambiguous), Ok(accepted)]);
    let scribe = Scribe::new(config(), model);
    let result = scribe
        .parse(ParseRequest::new("foo\nfoo\nbar\n"))
        .await
        .expect("the second attempt is valid");

    assert_eq!(result.run.attempts, 2);
    assert_eq!(scribe.model().calls(), 2);
    assert_eq!(result.spans[0].start_byte, 0);
    assert_eq!(result.spans[0].end_byte, 3);

    // Retries are fresh: the model sees exactly the same input, no feedback.
    let inputs = scribe.model().inputs();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0], inputs[1]);
}

#[tokio::test]
async fn the_model_sees_numbered_segments_without_crlf() {
    let scribe = Scribe::new(config(), StubModel::valid(Extraction::default()));
    scribe
        .parse(ParseRequest::new("a\r\nb"))
        .await
        .expect("parse succeeds");

    assert_eq!(scribe.model().inputs(), vec!["[1] a\n[2] b"]);
}

// ---------------------------------------------------------------------------
// Validation failures
// ---------------------------------------------------------------------------

async fn invalid_after(proposed: Extraction, max_attempts: u32) -> ParseError {
    let script = (0..max_attempts).map(|_| Ok(proposed.clone())).collect();
    let model = StubModel::new(script);
    let scribe = Scribe::new(config().with_max_attempts(max_attempts), model);
    scribe
        .parse(ParseRequest::new("a\nb\nc\n"))
        .await
        .expect_err("output should be rejected")
}

#[tokio::test]
async fn overlapping_spans_are_rejected() {
    let error = invalid_after(
        extraction(
            vec![span("s1", 1, 2), span("s2", 2, 3)],
            vec![question("s1"), question("s2")],
        ),
        2,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, attempts } => {
            assert_eq!(attempts, 2);
            assert!(detail.contains("overlap"), "detail was {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn nested_spans_are_rejected() {
    let error = invalid_after(
        extraction(
            vec![span("outer", 1, 3), span("inner", 2, 2)],
            vec![question("outer"), question("inner")],
        ),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("overlap") || detail.contains("nest"));
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn unreferenced_spans_are_rejected() {
    let error = invalid_after(
        extraction(
            vec![span("s1", 1, 1), span("unused", 3, 3)],
            vec![question("s1")],
        ),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("not referenced"), "detail was {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn unknown_annotation_references_are_rejected() {
    let error = invalid_after(
        extraction(vec![span("s1", 1, 1)], vec![question("missing")]),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("unknown span"), "detail was {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn unknown_answer_targets_are_rejected() {
    let error = invalid_after(
        extraction(vec![span("s1", 1, 1)], vec![answer("s1", &["missing"])]),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("unknown span"), "detail was {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn an_answer_target_must_be_a_question() {
    let error = invalid_after(
        extraction(
            vec![span("s1", 1, 1), span("s2", 2, 2)],
            vec![answer("s2", &["s1"])],
        ),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("not a question"), "detail was {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn a_span_cannot_answer_itself() {
    let error = invalid_after(
        extraction(
            vec![span("s1", 1, 1)],
            vec![question("s1"), answer("s1", &["s1"])],
        ),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("cannot answer itself"), "detail {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn answers_need_at_least_one_target() {
    let error = invalid_after(
        extraction(
            vec![span("s1", 1, 1)],
            vec![question("s1"), answer("s1", &[])],
        ),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("at least one"), "detail {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn questions_must_not_list_answer_targets() {
    let error = invalid_after(
        extraction(
            vec![span("s1", 1, 1)],
            vec![ExtractedAnnotation {
                questions: Vec::new(),
                alternatives: Vec::new(),
                answers: vec!["s1".to_owned()],
                ..question("s1")
            }],
        ),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("must not list"), "detail {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn assignees_belong_only_on_questions() {
    let error = invalid_after(
        extraction(
            vec![span("s1", 1, 1), span("s2", 2, 2)],
            vec![
                question("s1"),
                ExtractedAnnotation {
                    assignee: Some("@infra-team".to_owned()),
                    ..answer("s2", &["s1"])
                },
            ],
        ),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("only question"), "detail {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn malformed_assignee_handles_are_rejected() {
    for handle in ["infra-team", "@", "@infra team", "@ infra", "@infra\tteam"] {
        let error = invalid_after(
            extraction(vec![span("s1", 1, 1)], vec![assigned("s1", handle)]),
            1,
        )
        .await;

        match error {
            ParseError::InvalidOutput { detail, .. } => {
                assert!(
                    detail.contains("assignee"),
                    "handle {handle:?}: detail was {detail:?}"
                );
            }
            other => panic!("expected invalid output for {handle:?}, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn duplicate_span_ids_are_rejected() {
    let error = invalid_after(
        extraction(
            vec![span("s1", 1, 1), span("s1", 2, 2)],
            vec![question("s1")],
        ),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("more than once"), "detail {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn duplicate_question_annotations_are_rejected() {
    let error = invalid_after(
        extraction(vec![span("s1", 1, 1)], vec![question("s1"), question("s1")]),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(
                detail.contains("more than one question"),
                "detail {detail:?}"
            );
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn duplicate_answer_links_are_rejected() {
    let error = invalid_after(
        extraction(
            vec![span("s1", 1, 1), span("s2", 2, 2)],
            vec![question("s1"), answer("s2", &["s1", "s1"])],
        ),
        1,
    )
    .await;

    match error {
        ParseError::InvalidOutput { detail, .. } => {
            assert!(detail.contains("duplicate"), "detail {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[tokio::test]
async fn invalid_quotes_and_bounds_are_rejected() {
    let cases = [
        (
            extraction(vec![quoted("s1", 1, 1, "missing")], vec![question("s1")]),
            "was not found",
        ),
        (
            extraction(vec![quoted("s1", 1, 1, "")], vec![question("s1")]),
            "must not be empty",
        ),
        (
            extraction(vec![span("s1", 4, 4)], vec![question("s1")]),
            "does not exist",
        ),
        (
            extraction(vec![span("s1", 0, 1)], vec![question("s1")]),
            "invalid",
        ),
        (
            extraction(vec![span("s1", 2, 1)], vec![question("s1")]),
            "invalid",
        ),
        (
            extraction(vec![span("s1", 2, 2)], vec![question("s1")]),
            "no non-whitespace",
        ),
    ];

    for (proposed, expected) in cases {
        let model = StubModel::new(vec![Ok(proposed)]);
        let scribe = Scribe::new(config().with_max_attempts(1), model);
        let error = scribe
            .parse(ParseRequest::new("first\n   \nthird"))
            .await
            .expect_err("proposal should be rejected");
        match error {
            ParseError::InvalidOutput { detail, .. } => {
                assert!(
                    detail.contains(expected),
                    "expected {expected:?} in {detail:?}"
                );
            }
            other => panic!("expected invalid output, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Explicit versus incidental mentions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_incidental_mention_does_not_become_an_assignee() {
    let proposed = extraction(
        vec![quoted("s1", 2, 2, "Should we cache tokens?")],
        vec![question("s1")],
    );
    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(
            "Alex (@alex) raised this.\nShould we cache tokens?\n",
        ))
        .await
        .expect("parse succeeds");

    assert_eq!(labels_of(&result, LabelKind::Question)[0].assignee, None);
}

#[tokio::test]
async fn an_explicit_assignment_is_preserved() {
    let proposed = extraction(
        vec![quoted("s1", 2, 2, "Should we cache tokens?")],
        vec![assigned("s1", "@alex")],
    );
    let scribe = Scribe::new(config(), StubModel::valid(proposed));
    let result = scribe
        .parse(ParseRequest::new(
            "@alex please confirm:\nShould we cache tokens?\n",
        ))
        .await
        .expect("parse succeeds");

    assert_eq!(
        labels_of(&result, LabelKind::Question)[0]
            .assignee
            .as_deref(),
        Some("@alex")
    );
}

// ---------------------------------------------------------------------------
// Limits, attempts, and failures
// ---------------------------------------------------------------------------

#[tokio::test]
async fn oversized_input_is_rejected_before_any_model_call() {
    let model = StubModel::valid(Extraction::default());
    let scribe = Scribe::new(config().with_input_limits(4, 100), model);
    let error = scribe
        .parse(ParseRequest::new("hello"))
        .await
        .expect_err("five bytes exceed a four-byte limit");

    assert!(matches!(error, ParseError::OversizedInput { .. }));
    assert_eq!(scribe.model().calls(), 0);
}

#[tokio::test]
async fn too_many_segments_are_rejected_before_any_model_call() {
    let model = StubModel::valid(Extraction::default());
    let scribe = Scribe::new(config().with_input_limits(1_024, 2), model);
    let error = scribe
        .parse(ParseRequest::new("a\nb\nc"))
        .await
        .expect_err("three segments exceed a two-segment limit");

    assert!(matches!(error, ParseError::OversizedInput { .. }));
    assert_eq!(scribe.model().calls(), 0);
}

#[tokio::test]
async fn empty_input_is_an_invalid_request() {
    let model = StubModel::valid(Extraction::default());
    let scribe = Scribe::new(config(), model);
    let error = scribe
        .parse(ParseRequest::new(""))
        .await
        .expect_err("empty text has nothing to parse");

    match error {
        ParseError::InvalidRequest { detail } => assert!(detail.contains("empty")),
        other => panic!("expected invalid request, got {other:?}"),
    }
    assert_eq!(scribe.model().calls(), 0);
}

#[tokio::test]
async fn invalid_output_retries_fresh_until_success() {
    let bad = extraction(vec![span("s1", 1, 1)], vec![question("missing")]);
    let good = extraction(vec![span("s1", 1, 1)], vec![question("s1")]);

    let model = StubModel::new(vec![Ok(bad.clone()), Ok(bad), Ok(good)]);
    let scribe = Scribe::new(config().with_max_attempts(3), model);
    let result = scribe
        .parse(ParseRequest::new("Question?"))
        .await
        .expect("third attempt succeeds");

    assert_eq!(result.run.attempts, 3);
    assert_eq!(scribe.model().calls(), 3);
    let inputs = scribe.model().inputs();
    assert_eq!(inputs.len(), 3);
    assert!(inputs.windows(2).all(|pair| pair[0] == pair[1]));
}

#[tokio::test]
async fn invalid_model_output_is_retried_and_exhausts_the_budget() {
    let invalid = || {
        Err(ModelError::Invalid(
            "submit arguments did not match the schema".to_owned(),
        ))
    };
    let model = StubModel::new(vec![invalid(), invalid()]);
    let scribe = Scribe::new(config().with_max_attempts(2), model);
    let error = scribe
        .parse(ParseRequest::new("Question?"))
        .await
        .expect_err("both attempts are invalid output");

    match error {
        ParseError::InvalidOutput { attempts, detail } => {
            assert_eq!(attempts, 2);
            assert!(detail.contains("schema"), "detail {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
    assert_eq!(scribe.model().calls(), 2);
}

#[tokio::test]
async fn exhausted_budget_reports_the_attempt_count() {
    let bad = extraction(vec![span("s1", 1, 1)], vec![question("missing")]);
    let model = StubModel::new(vec![Ok(bad.clone()), Ok(bad)]);
    let scribe = Scribe::new(config().with_max_attempts(2), model);
    let error = scribe
        .parse(ParseRequest::new("Question?"))
        .await
        .expect_err("both attempts are invalid");

    match error {
        ParseError::InvalidOutput { attempts, detail } => {
            assert_eq!(attempts, 2);
            assert!(detail.contains("unknown span"), "detail {detail:?}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
    assert_eq!(scribe.model().calls(), 2);
}

#[tokio::test]
async fn a_zero_attempt_budget_still_makes_one_attempt() {
    let model = StubModel::valid(Extraction::default());
    let scribe = Scribe::new(config().with_max_attempts(0), model);
    let result = scribe
        .parse(ParseRequest::new("Text."))
        .await
        .expect("one attempt runs");

    assert_eq!(result.run.attempts, 1);
    assert_eq!(scribe.model().calls(), 1);
}

#[tokio::test]
async fn provider_failures_fail_fast_without_retrying() {
    let model = StubModel::new(vec![
        Err(ModelError::Provider("quota exceeded".to_owned())),
        Ok(Extraction::default()),
    ]);
    let scribe = Scribe::new(config().with_max_attempts(3), model);
    let error = scribe
        .parse(ParseRequest::new("Text."))
        .await
        .expect_err("provider failure");

    match error {
        ParseError::Provider { detail } => assert!(detail.contains("quota")),
        other => panic!("expected provider error, got {other:?}"),
    }
    assert_eq!(scribe.model().calls(), 1);
}

#[tokio::test]
async fn internal_adapter_failures_are_not_retried() {
    let model = StubModel::new(vec![Err(ModelError::Internal("bug".to_owned()))]);
    let scribe = Scribe::new(config().with_max_attempts(3), model);
    let error = scribe
        .parse(ParseRequest::new("Text."))
        .await
        .expect_err("internal failure");

    assert!(matches!(error, ParseError::Internal { .. }));
    assert_eq!(scribe.model().calls(), 1);
}

#[tokio::test(start_paused = true)]
async fn timeouts_fail_fast_without_retrying() {
    let model = StubModel::pending();
    let scribe = Scribe::new(
        config()
            .with_timeout(Duration::from_millis(10))
            .with_max_attempts(3),
        model,
    );
    let error = scribe
        .parse(ParseRequest::new("Text."))
        .await
        .expect_err("the model never answers");

    match error {
        ParseError::Timeout { detail } => assert!(detail.contains("exceeded")),
        other => panic!("expected timeout, got {other:?}"),
    }
    assert_eq!(scribe.model().calls(), 1);
}

// ---------------------------------------------------------------------------
// Object-safe parser interface
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scribe_can_be_used_as_a_dyn_parser() {
    let parser: Arc<dyn Parser> = Arc::new(Scribe::new(
        config(),
        StubModel::valid(Extraction::default()),
    ));
    let result = parser
        .parse(ParseRequest::new("Text."))
        .await
        .expect("parse succeeds");

    assert_eq!(result.payload.content, "Text.");
    assert_eq!(root(&result).author, "unknown");
}
