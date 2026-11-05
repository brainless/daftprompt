//! `POST /clarify` contract tests.
//!
//! Every test uses a stub model or a stub [`Clarifier`]; none needs
//! credentials, a network, or a real provider. Success cases run the real
//! Explainer pipeline (validation and graph construction) over a stub model.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use daftprompt_api::{ApiConfig, AppState, router};
use daftprompt_core::explainer::{BoxFuture, ExplainerInput, ExplainerModel, ModelError};
use daftprompt_core::project_tools::InMemoryProject;
use daftprompt_core::{Clarifier, Explainer, ExplainerConfig, Parser};
use daftprompt_types::{
    ClarifyRequest, ClarifyResult, ExplainerCitation, ExplainerExtraction, ParseError,
    ParseRequest, ParseResult,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const SOURCE: &str = "We use a token bucket.\nIt refills slowly.\nThe limit is per user.";
const LIMITER: &str = "// rate limiter\nstruct Bucket {\n    capacity: u32,\n}\nfn refill() {}\n";

fn sha256(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

// ---------------------------------------------------------------------------
// Stubs
// ---------------------------------------------------------------------------

struct NoParser;

impl Parser for NoParser {
    fn parse(&self, _request: ParseRequest) -> BoxFuture<'_, Result<ParseResult, ParseError>> {
        Box::pin(async {
            Err(ParseError::Internal {
                detail: "unused".to_owned(),
            })
        })
    }
}

enum Behavior {
    Submit(Vec<(&'static str, Value)>, ExplainerExtraction),
    Fail(ModelError),
    Hang,
}

struct StubModel {
    behavior: Behavior,
    prompts: Arc<Mutex<Vec<String>>>,
}

impl ExplainerModel for StubModel {
    fn explain(
        &self,
        input: ExplainerInput,
    ) -> BoxFuture<'_, Result<ExplainerExtraction, ModelError>> {
        self.prompts.lock().unwrap().push(input.prompt.clone());
        Box::pin(async move {
            match &self.behavior {
                Behavior::Submit(calls, extraction) => {
                    if let Some(toolbox) = &input.toolbox {
                        for (name, args) in calls {
                            toolbox.call(name, args.clone());
                        }
                    }
                    Ok(extraction.clone())
                }
                Behavior::Fail(error) => Err(match error {
                    ModelError::Provider(d) => ModelError::Provider(d.clone()),
                    ModelError::Invalid(d) => ModelError::Invalid(d.clone()),
                    ModelError::Internal(d) => ModelError::Internal(d.clone()),
                }),
                Behavior::Hang => {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    unreachable!()
                }
            }
        })
    }
}

/// A clarifier that always fails with a fixed error.
struct FailingClarifier(ParseError);

impl Clarifier for FailingClarifier {
    fn clarify(
        &self,
        _request: ClarifyRequest,
    ) -> BoxFuture<'_, Result<ClarifyResult, ParseError>> {
        let error = self.0.clone();
        Box::pin(async move { Err(error) })
    }
}

fn answer(explanation: &str, citations: Vec<ExplainerCitation>) -> ExplainerExtraction {
    ExplainerExtraction {
        explanation: explanation.to_owned(),
        citations,
    }
}

fn app_with(clarifier: Arc<dyn Clarifier>) -> Router {
    router(AppState::new(Arc::new(NoParser), ApiConfig::default()).with_clarifier(clarifier))
}

fn explainer_app(
    behavior: Behavior,
    project: bool,
    config: ExplainerConfig,
) -> (Router, Arc<Mutex<Vec<String>>>) {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let model = StubModel {
        behavior,
        prompts: prompts.clone(),
    };
    let mut explainer = Explainer::new(config, model);
    if project {
        explainer = explainer.with_project(Arc::new(
            InMemoryProject::new().with_file("src/limit.rs", LIMITER),
        ));
    }
    (app_with(Arc::new(explainer)), prompts)
}

fn body() -> Value {
    let start = SOURCE.find("token bucket").unwrap();
    json!({
        "source": { "text": SOURCE, "payload_hash": sha256(SOURCE) },
        "target": {
            "node_id": "node-target",
            "display_id": 3,
            "start_byte": start,
            "end_byte": start + "token bucket".len()
        },
        "question": "What does the bucket mean here?",
        "author": "ada"
    })
}

async fn post(app: Router, body: impl Into<Body>) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/clarify")
                .header("content-type", "application/json")
                .body(body.into())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn post_json(app: Router, value: Value) -> (StatusCode, Value) {
    post(app, value.to_string()).await
}

// ---------------------------------------------------------------------------
// Success
// ---------------------------------------------------------------------------

#[tokio::test]
async fn first_turn_in_text_only_mode() {
    let (app, _) = explainer_app(
        Behavior::Submit(
            vec![],
            answer(
                "A token bucket meters requests [1].",
                vec![ExplainerCitation::Source {
                    start_segment: 1,
                    end_segment: 1,
                }],
            ),
        ),
        false,
        ExplainerConfig::default(),
    );
    let (status, json) = post_json(app, body()).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["grounding"], "text_only");
    assert_eq!(
        json["question_payload"]["content"],
        "What does the bucket mean here?"
    );
    assert_eq!(
        json["explanation_payload"]["content"],
        "A token bucket meters requests [1]."
    );
    assert_eq!(json["nodes"][0]["parent_id"], "node-target");
    assert_eq!(json["nodes"][0]["author"], "ada");
    assert_eq!(json["labels"][0]["kind"], "clarification");
    assert_eq!(json["citations"][0]["index"], 1);
    assert_eq!(json["citations"][0]["location"]["kind"], "source");
    assert_eq!(json["source_window"]["start_segment"], 1);
    assert_eq!(json["run"]["agent"], "explainer");
    // The result is the shared type.
    serde_json::from_value::<ClarifyResult>(json).expect("a ClarifyResult");
}

#[tokio::test]
async fn follow_up_chains_under_the_previous_explanation() {
    let (app, prompts) = explainer_app(
        Behavior::Submit(vec![], answer("Because bursts.", vec![])),
        false,
        ExplainerConfig::default(),
    );
    let mut request = body();
    request["question"] = json!("Why not a fixed window?");
    request["turns"] = json!([{
        "question_node_id": "q-old",
        "question": "What is it?",
        "explanation_node_id": "e-old",
        "explanation": "A rate limiter."
    }]);
    let (status, json) = post_json(app, request).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["nodes"][0]["parent_id"], "e-old");
    assert_eq!(json["labels"], json!([]));
    assert!(prompts.lock().unwrap()[0].contains("A rate limiter."));
}

#[tokio::test]
async fn project_mode_returns_file_citations() {
    let (app, _) = explainer_app(
        Behavior::Submit(
            vec![(
                "read_file",
                json!({"path": "src/limit.rs", "start_line": 1, "end_line": 4}),
            )],
            answer(
                "The bucket holds capacity [1].",
                vec![ExplainerCitation::File {
                    path: "src/limit.rs".to_owned(),
                    start_line: 2,
                    end_line: 4,
                }],
            ),
        ),
        true,
        ExplainerConfig::default(),
    );
    let (status, json) = post_json(app, body()).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["grounding"], "project");
    assert_eq!(
        json["citations"][0]["location"],
        json!({"kind": "file", "path": "src/limit.rs", "start_line": 2, "end_line": 4})
    );
    assert!(json["tool_calls"].as_array().unwrap().len() >= 2);
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn malformed_json_is_400() {
    let (app, _) = explainer_app(
        Behavior::Submit(vec![], answer("x", vec![])),
        false,
        ExplainerConfig::default(),
    );
    let (status, json) = post(app, "{ not json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["kind"], "invalid_request");
}

#[tokio::test]
async fn missing_fields_are_400() {
    let (app, _) = explainer_app(
        Behavior::Submit(vec![], answer("x", vec![])),
        false,
        ExplainerConfig::default(),
    );
    let (status, json) = post_json(app, json!({"question": "hi"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["kind"], "invalid_request");
}

#[tokio::test]
async fn hash_mismatch_is_400() {
    let (app, _) = explainer_app(
        Behavior::Submit(vec![], answer("x", vec![])),
        false,
        ExplainerConfig::default(),
    );
    let mut request = body();
    request["source"]["payload_hash"] = json!(sha256("other"));
    let (status, json) = post_json(app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["kind"], "payload_mismatch");
}

#[tokio::test]
async fn invalid_target_is_400() {
    let (app, _) = explainer_app(
        Behavior::Submit(vec![], answer("x", vec![])),
        false,
        ExplainerConfig::default(),
    );
    let mut request = body();
    request["target"]["end_byte"] = json!(SOURCE.len() + 10);
    let (status, json) = post_json(app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["kind"], "invalid_target");
}

#[tokio::test]
async fn oversized_question_is_413() {
    let (app, _) = explainer_app(
        Behavior::Submit(vec![], answer("x", vec![])),
        false,
        ExplainerConfig::default(),
    );
    let mut request = body();
    request["question"] = json!("q".repeat(3000));
    let (status, json) = post_json(app, request).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json["kind"], "oversized_input");
}

#[tokio::test]
async fn oversized_envelope_is_413() {
    let (app, _) = explainer_app(
        Behavior::Submit(vec![], answer("x", vec![])),
        false,
        ExplainerConfig::default(),
    );
    let (status, json) = post(app, "x".repeat(2 * ApiConfig::default().max_body_bytes + 1)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json["kind"], "oversized_input");
}

#[tokio::test]
async fn invalid_output_after_attempts_is_422() {
    let (app, prompts) = explainer_app(
        Behavior::Fail(ModelError::Invalid("no submission".to_owned())),
        false,
        ExplainerConfig::default(),
    );
    let (status, json) = post_json(app, body()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["kind"], "invalid_output");
    assert_eq!(json["attempts"], 3);
    assert_eq!(prompts.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn provider_failure_is_502_without_retry() {
    let (app, prompts) = explainer_app(
        Behavior::Fail(ModelError::Provider("upstream down".to_owned())),
        false,
        ExplainerConfig::default(),
    );
    let (status, json) = post_json(app, body()).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(json["kind"], "provider_error");
    assert_eq!(prompts.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn timeout_is_504() {
    let (app, _) = explainer_app(
        Behavior::Hang,
        false,
        ExplainerConfig::default().with_timeout(Duration::from_millis(20)),
    );
    let (status, json) = post_json(app, body()).await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(json["kind"], "timeout");
}

#[tokio::test]
async fn stub_clarifier_errors_map_to_status() {
    let cases = [
        (
            ParseError::InvalidRequest {
                detail: String::new(),
            },
            StatusCode::BAD_REQUEST,
        ),
        (
            ParseError::PayloadMismatch {
                detail: String::new(),
            },
            StatusCode::BAD_REQUEST,
        ),
        (
            ParseError::InvalidTarget {
                detail: String::new(),
            },
            StatusCode::BAD_REQUEST,
        ),
        (
            ParseError::OversizedInput {
                detail: String::new(),
            },
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            ParseError::InvalidOutput {
                detail: String::new(),
                attempts: 3,
            },
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            ParseError::Provider {
                detail: String::new(),
            },
            StatusCode::BAD_GATEWAY,
        ),
        (
            ParseError::Timeout {
                detail: String::new(),
            },
            StatusCode::GATEWAY_TIMEOUT,
        ),
        (
            ParseError::Internal {
                detail: String::new(),
            },
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ];
    for (error, expected) in cases {
        let (status, _) =
            post_json(app_with(Arc::new(FailingClarifier(error.clone()))), body()).await;
        assert_eq!(status, expected, "{error:?}");
    }
}

#[tokio::test]
async fn unconfigured_clarifier_is_500() {
    let app = router(AppState::new(Arc::new(NoParser), ApiConfig::default()));
    let (status, json) = post_json(app, body()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json["kind"], "internal_error");
}

#[test]
fn request_type_matches_the_documented_json() {
    serde_json::from_value::<ClarifyRequest>(body()).expect("a ClarifyRequest");
}
