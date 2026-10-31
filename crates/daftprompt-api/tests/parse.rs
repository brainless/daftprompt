//! API contract tests.
//!
//! Every test uses a stub [`Parser`]; none needs credentials, a network, or a
//! real model. Success returns a graph built by the real Scribe pipeline over
//! a stub extraction, so the wire shape is the one a client actually sees.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use daftprompt_api::{ApiConfig, AppState, router};
use daftprompt_core::{
    BoxFuture, ExtractionModel, ModelError, ModelInput, Parser, Scribe, ScribeConfig,
};
use daftprompt_types::{
    AnnotationKind, ExtractedAnnotation, ExtractedSpan, Extraction, LabelKind, LabelStatus,
    ParseError, ParseRequest, ParseResult, SourceKind, SourceMetadata,
};
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Stub parser and model
// ---------------------------------------------------------------------------

struct StubParser {
    outcome: Result<ParseResult, ParseError>,
    calls: AtomicUsize,
    requests: Mutex<Vec<ParseRequest>>,
}

impl StubParser {
    fn returning(outcome: Result<ParseResult, ParseError>) -> Arc<Self> {
        Arc::new(Self {
            outcome,
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn requests(&self) -> Vec<ParseRequest> {
        self.requests.lock().expect("requests lock").clone()
    }
}

impl Parser for StubParser {
    fn parse(&self, request: ParseRequest) -> BoxFuture<'_, Result<ParseResult, ParseError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().expect("requests lock").push(request);
        let outcome = self.outcome.clone();
        Box::pin(async move { outcome })
    }
}

struct StubModel {
    extraction: Extraction,
}

impl ExtractionModel for StubModel {
    fn extract(&self, _input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        let extraction = self.extraction.clone();
        Box::pin(async move { Ok(extraction) })
    }
}

struct CountingModel {
    calls: Arc<AtomicUsize>,
}

impl ExtractionModel for CountingModel {
    fn extract(&self, _input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(Extraction::default()) })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const TEXT: &str = "Plan\nShould we cache tokens? @infra-team\nYes, cache them.\n";
const QUESTION: &str = "Should we cache tokens?";
const ANSWER: &str = "Yes, cache them.";

fn source() -> SourceMetadata {
    SourceMetadata {
        kind: Some(SourceKind::Chat),
        reference: Some("thread-1".to_owned()),
        author: Some("ada".to_owned()),
    }
}

/// A complete result produced through the real Scribe pipeline with a stub
/// model: one assigned question resolved by a local answer.
async fn successful_result() -> ParseResult {
    let extraction = Extraction {
        spans: vec![
            ExtractedSpan {
                id: "s1".to_owned(),
                start_segment: 2,
                end_segment: 2,
                quote: Some(QUESTION.to_owned()),
            },
            ExtractedSpan {
                id: "s2".to_owned(),
                start_segment: 3,
                end_segment: 3,
                quote: Some(ANSWER.to_owned()),
            },
        ],
        annotations: vec![
            ExtractedAnnotation {
                kind: AnnotationKind::Question,
                span: "s1".to_owned(),
                answers: Vec::new(),
                assignee: Some("@infra-team".to_owned()),
            },
            ExtractedAnnotation {
                kind: AnnotationKind::Answer,
                span: "s2".to_owned(),
                answers: vec!["s1".to_owned()],
                assignee: None,
            },
        ],
    };

    let scribe = Scribe::new(
        ScribeConfig::new()
            .with_provider("test-provider")
            .with_model("stub-model"),
        StubModel { extraction },
    );
    scribe
        .parse(ParseRequest::with_source(TEXT, source()))
        .await
        .expect("the stub extraction is valid")
}

fn app(parser: Arc<StubParser>, config: ApiConfig) -> Router {
    router(AppState::new(parser, config))
}

async fn post(app: Router, body: impl Into<Body>) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/parse")
                .header("content-type", "application/json")
                .body(body.into())
                .expect("request builds"),
        )
        .await
        .expect("the router answers");

    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let value = serde_json::from_slice(&bytes).expect("the response body is JSON");
    (status, value)
}

async fn post_request(app: Router, request: &ParseRequest) -> (StatusCode, serde_json::Value) {
    post(
        app,
        serde_json::to_string(request).expect("request serializes"),
    )
    .await
}

// ---------------------------------------------------------------------------
// Success
// ---------------------------------------------------------------------------

#[tokio::test]
async fn success_returns_the_graph_payload_text_and_span_byte_ranges() {
    let expected = successful_result().await;
    let parser = StubParser::returning(Ok(expected.clone()));

    let request = ParseRequest::with_source(TEXT, source());
    let (status, body) = post_request(app(parser.clone(), ApiConfig::default()), &request).await;

    assert_eq!(status, StatusCode::OK);

    let result: ParseResult = serde_json::from_value(body).expect("the body is a ParseResult");
    assert_eq!(result, expected);

    // The complete original text is present exactly once in the payload.
    assert_eq!(result.payload.content, TEXT);
    assert_eq!(result.payload.hash.len(), 64);

    // Every span node has a half-open byte range into the payload, on char
    // boundaries, matching the quoted text clients render.
    let question_start = TEXT.find(QUESTION).expect("question text");
    let answer_start = TEXT.find(ANSWER).expect("answer text");
    let ranges: Vec<(usize, usize)> = result
        .spans
        .iter()
        .map(|span| (span.start_byte, span.end_byte))
        .collect();
    assert_eq!(
        ranges,
        vec![
            (question_start, question_start + QUESTION.len()),
            (answer_start, answer_start + ANSWER.len()),
        ]
    );
    assert_eq!(
        &result.payload.content[question_start..question_start + QUESTION.len()],
        QUESTION
    );

    // Each span node also has a ranged context pointing at the root payload.
    for span in &result.spans {
        let context = result
            .node_contexts
            .iter()
            .find(|context| context.node_id == span.node_id)
            .expect("a span context");
        assert!(context.range.is_some());
        match &context.target {
            daftprompt_types::ContextTarget::Payload { payload_hash } => {
                assert_eq!(payload_hash, &result.payload.hash);
            }
            daftprompt_types::ContextTarget::File { .. } => panic!("span targets the payload"),
        }
    }

    // Labels, assignee, and the answer target are ready to render.
    let question = result
        .labels
        .iter()
        .find(|label| label.kind == LabelKind::Question)
        .expect("a question label");
    assert_eq!(question.status, Some(LabelStatus::Resolved));
    assert_eq!(question.assignee.as_deref(), Some("@infra-team"));

    let answer = result
        .labels
        .iter()
        .find(|label| label.kind == LabelKind::Answer)
        .expect("an answer label");
    assert_eq!(
        answer.dst_node_id.as_deref(),
        Some(question.src_node_id.as_str())
    );

    // Provenance crosses the boundary unchanged.
    assert_eq!(result.source, Some(source()));

    // The API hands the parser the request unchanged.
    assert_eq!(parser.calls(), 1);
    assert_eq!(parser.requests(), vec![request]);
}

// ---------------------------------------------------------------------------
// Parse failure mapping
// ---------------------------------------------------------------------------

async fn assert_failure(
    error: ParseError,
    expected_status: StatusCode,
    expected_kind: &str,
    expected_detail: &str,
) {
    let parser = StubParser::returning(Err(error));
    let (status, body) = post_request(
        app(parser.clone(), ApiConfig::default()),
        &ParseRequest::new("Question?"),
    )
    .await;

    assert_eq!(status, expected_status);
    assert_eq!(body["kind"], expected_kind);
    assert_eq!(body["detail"], expected_detail);
    assert_eq!(parser.calls(), 1);
}

#[tokio::test]
async fn invalid_request_maps_to_400() {
    assert_failure(
        ParseError::InvalidRequest {
            detail: "the submitted text is empty".to_owned(),
        },
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "the submitted text is empty",
    )
    .await;
}

#[tokio::test]
async fn oversized_input_maps_to_413() {
    assert_failure(
        ParseError::OversizedInput {
            detail: "the submitted text is 40 byte(s); the limit is 32 byte(s)".to_owned(),
        },
        StatusCode::PAYLOAD_TOO_LARGE,
        "oversized_input",
        "the submitted text is 40 byte(s); the limit is 32 byte(s)",
    )
    .await;
}

#[tokio::test]
async fn invalid_output_maps_to_422_with_the_attempt_count() {
    let parser = StubParser::returning(Err(ParseError::InvalidOutput {
        detail: "spans 1 and 2 overlap".to_owned(),
        attempts: 3,
    }));
    let (status, body) = post_request(
        app(parser.clone(), ApiConfig::default()),
        &ParseRequest::new("Question?"),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["kind"], "invalid_output");
    assert_eq!(body["detail"], "spans 1 and 2 overlap");
    assert_eq!(body["attempts"], 3);
    assert_eq!(parser.calls(), 1);
}

#[tokio::test]
async fn provider_failure_maps_to_502() {
    assert_failure(
        ParseError::Provider {
            detail: "quota exceeded".to_owned(),
        },
        StatusCode::BAD_GATEWAY,
        "provider_error",
        "quota exceeded",
    )
    .await;
}

#[tokio::test]
async fn timeout_maps_to_504() {
    assert_failure(
        ParseError::Timeout {
            detail: "the model call exceeded 60s on attempt 2 of 3".to_owned(),
        },
        StatusCode::GATEWAY_TIMEOUT,
        "timeout",
        "the model call exceeded 60s on attempt 2 of 3",
    )
    .await;
}

#[tokio::test]
async fn internal_failure_maps_to_500() {
    assert_failure(
        ParseError::Internal {
            detail: "the extraction adapter failed".to_owned(),
        },
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "the extraction adapter failed",
    )
    .await;
}

// ---------------------------------------------------------------------------
// Request validation before the parser
// ---------------------------------------------------------------------------

#[tokio::test]
async fn malformed_json_maps_to_400_without_calling_the_parser() {
    let parser = StubParser::returning(Ok(successful_result().await));
    let (status, body) = post(app(parser.clone(), ApiConfig::default()), r#"{"text":"#).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["kind"], "invalid_request");
    assert!(
        body["detail"]
            .as_str()
            .expect("detail")
            .contains("valid ParseRequest JSON")
    );
    assert_eq!(parser.calls(), 0);
}

#[tokio::test]
async fn a_missing_text_field_maps_to_400_without_calling_the_parser() {
    let parser = StubParser::returning(Ok(successful_result().await));
    let (status, body) = post(app(parser.clone(), ApiConfig::default()), r#"{}"#).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["kind"], "invalid_request");
    assert_eq!(parser.calls(), 0);
}

#[tokio::test]
async fn an_oversized_json_envelope_maps_to_413_without_calling_the_parser() {
    let parser = StubParser::returning(Ok(successful_result().await));
    let config = ApiConfig::new(32);
    let (status, body) = post(
        app(parser.clone(), config),
        r#"{"text":"this request body is longer than thirty-two bytes"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["kind"], "oversized_input");
    assert_eq!(parser.calls(), 0);
}

#[tokio::test]
async fn empty_text_maps_to_400_before_the_model_runs() {
    let calls = Arc::new(AtomicUsize::new(0));
    let scribe = Scribe::new(
        ScribeConfig::new()
            .with_provider("test-provider")
            .with_model("stub-model"),
        CountingModel {
            calls: calls.clone(),
        },
    );
    let parser: Arc<dyn Parser> = Arc::new(scribe);

    let (status, body) = post(
        router(AppState::new(parser, ApiConfig::default())),
        r#"{"text":""}"#,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["kind"], "invalid_request");
    assert!(
        body["detail"].as_str().expect("detail").contains("empty"),
        "detail was {}",
        body["detail"]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn only_post_is_served_on_parse() {
    let parser = StubParser::returning(Ok(successful_result().await));
    let response = app(parser, ApiConfig::default())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/parse")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}
