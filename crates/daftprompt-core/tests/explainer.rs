//! Deterministic Explainer tests: a stub model drives the per-attempt toolbox
//! over an in-memory project. No credentials or network are needed.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use daftprompt_core::ProjectScope;
use daftprompt_core::explainer::{
    Clarifier, Explainer, ExplainerConfig, ExplainerInput, ExplainerModel, ModelError,
};
use daftprompt_core::project_tools::{InMemoryProject, ProjectFiles};
use daftprompt_types::{
    ClarifyRequest, ClarifySource, ClarifyTarget, ClarifyTurn, ContextTarget, ExplainerCitation,
    ExplainerExtraction, Grounding, LabelKind, ParseError, SourceRange,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const LIMITER: &str =
    "// rate limiter\nstruct Bucket {\n    capacity: u32,\n}\nfn refill() {}\nfn take() {}\n";

type Script = dyn Fn(usize, &ExplainerInput) -> Step + Send + Sync;

enum Step {
    /// Run these `(tool, args)` calls on the toolbox, then submit.
    Submit(Vec<(&'static str, serde_json::Value)>, ExplainerExtraction),
    Fail(ModelError),
    Hang,
}

/// A stub model that records every input it receives.
struct Stub {
    script: Box<Script>,
    inputs: Mutex<Vec<ExplainerInput>>,
}

impl Stub {
    fn new(script: impl Fn(usize, &ExplainerInput) -> Step + Send + Sync + 'static) -> Self {
        Self {
            script: Box::new(script),
            inputs: Mutex::new(Vec::new()),
        }
    }

    fn always(calls: Vec<(&'static str, serde_json::Value)>, e: ExplainerExtraction) -> Self {
        Self::new(move |_, _| Step::Submit(calls.clone(), e.clone()))
    }

    fn calls(&self) -> usize {
        self.inputs.lock().unwrap().len()
    }
}

/// Newtype so the foreign trait can be implemented for a shared stub.
struct Handle(Arc<Stub>);

impl ExplainerModel for Handle {
    fn explain(
        &self,
        input: ExplainerInput,
    ) -> daftprompt_core::explainer::BoxFuture<'_, Result<ExplainerExtraction, ModelError>> {
        Box::pin(async move {
            let attempt = {
                let mut inputs = self.0.inputs.lock().unwrap();
                inputs.push(input.clone());
                inputs.len()
            };
            match (self.0.script)(attempt, &input) {
                Step::Submit(calls, extraction) => {
                    if let Some(toolbox) = &input.toolbox {
                        for (name, args) in calls {
                            toolbox.call(name, args);
                        }
                    }
                    Ok(extraction)
                }
                Step::Fail(error) => Err(error),
                Step::Hang => {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    unreachable!()
                }
            }
        })
    }
}

fn sha256(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn project() -> Arc<dyn ProjectFiles> {
    Arc::new(InMemoryProject::new().with_file("src/limit.rs", LIMITER))
}

fn text(lines: usize) -> String {
    (1..=lines)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A request targeting the byte range of `needle` in `source`.
fn request(source: &str, needle: &str) -> ClarifyRequest {
    let start = source.find(needle).expect("needle in source");
    ClarifyRequest {
        source: ClarifySource {
            text: source.to_owned(),
            payload_hash: sha256(source),
            metadata: None,
        },
        target: ClarifyTarget {
            node_id: "node-target".to_owned(),
            display_id: 3,
            start_byte: start,
            end_byte: start + needle.len(),
        },
        turns: Vec::new(),
        question: "What does the bucket mean here?".to_owned(),
        author: Some("ada".to_owned()),
        project_id: None,
        project_path_id: None,
    }
}

fn file(path: &str, start: u32, end: u32) -> ExplainerCitation {
    ExplainerCitation::File {
        path: path.to_owned(),
        start_line: start,
        end_line: end,
    }
}

fn source(start: u32, end: u32) -> ExplainerCitation {
    ExplainerCitation::Source {
        start_segment: start,
        end_segment: end,
    }
}

fn answer(explanation: &str, citations: Vec<ExplainerCitation>) -> ExplainerExtraction {
    ExplainerExtraction {
        explanation: explanation.to_owned(),
        citations,
    }
}

fn explainer(stub: &Arc<Stub>, with_project: bool) -> Explainer<Handle> {
    let explainer = Explainer::new(ExplainerConfig::default(), Handle(stub.clone()));
    if with_project {
        explainer.with_project(project())
    } else {
        explainer
    }
}

const SOURCE: &str = "We use a token bucket.\nIt refills slowly.\nThe limit is per user.";

type Calls = Vec<(&'static str, serde_json::Value)>;
type Case = (&'static str, Calls, ExplainerExtraction);

fn read_limiter() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("search", json!({"query": "bucket"})),
        (
            "read_file",
            json!({"path": "src/limit.rs", "start_line": 1, "end_line": 4}),
        ),
    ]
}

#[tokio::test]
async fn first_turn_builds_the_branch_with_file_and_source_citations() {
    let stub = Arc::new(Stub::always(
        read_limiter(),
        answer(
            "A bucket holds capacity [1]. Here it throttles [2].",
            vec![file("./src/limit.rs", 2, 4), source(1, 2)],
        ),
    ));
    let request = request(SOURCE, "token bucket");
    let result = explainer(&stub, true)
        .clarify(request.clone())
        .await
        .expect("clarify");

    assert_eq!(result.grounding, Grounding::Project);
    assert_eq!(result.question_payload.content, request.question);
    assert_eq!(result.question_payload.hash, sha256(&request.question));
    assert_eq!(
        result.explanation_payload.content,
        "A bucket holds capacity [1]. Here it throttles [2]."
    );

    // Nodes: Q under the target, E under Q.
    let [q, e] = result.nodes.as_slice() else {
        panic!("two nodes");
    };
    assert_eq!(q.id, result.question_node_id);
    assert_eq!(q.parent_id.as_deref(), Some("node-target"));
    assert_eq!(q.author, "ada");
    assert_eq!(q.run_id, None);
    assert_eq!(e.id, result.explanation_node_id);
    assert_eq!(e.parent_id.as_deref(), Some(q.id.as_str()));
    assert_eq!(e.author, "agent");
    assert_eq!(e.run_id.as_deref(), Some(result.run.id.as_str()));

    // The label exists only here, with no status.
    let [label] = result.labels.as_slice() else {
        panic!("one label");
    };
    assert_eq!(label.kind, LabelKind::Clarification);
    assert_eq!(label.src_node_id, q.id);
    assert_eq!(label.dst_node_id.as_deref(), Some("node-target"));
    assert_eq!(label.status, None);
    assert_eq!(label.created_by, "ada");
    assert_eq!(label.run_id, None);

    // Contexts: Q whole payload; E seq 0 whole payload; E seq 1, 2 citations.
    let contexts = &result.node_contexts;
    assert_eq!(contexts.len(), 4);
    assert_eq!(contexts[0].node_id, q.id);
    assert_eq!(contexts[0].seq, 0);
    assert_eq!(
        contexts[0].target,
        ContextTarget::Payload {
            payload_hash: result.question_payload.hash.clone()
        }
    );
    assert_eq!(contexts[0].range, None);
    assert_eq!(contexts[1].node_id, e.id);
    assert_eq!(contexts[1].seq, 0);
    assert_eq!(contexts[1].range, None);
    assert_eq!(
        contexts[1].target,
        ContextTarget::Payload {
            payload_hash: result.explanation_payload.hash.clone()
        }
    );

    assert_eq!(contexts[2].seq, 1);
    assert_eq!(
        contexts[2].target,
        ContextTarget::File {
            path: "src/limit.rs".to_owned(),
            content_hash: Some(sha256(LIMITER)),
        }
    );
    assert_eq!(
        contexts[2].range,
        Some(SourceRange {
            start_line: 2,
            start_col: 1,
            end_line: 5,
            end_col: 1
        })
    );

    assert_eq!(contexts[3].seq, 2);
    assert_eq!(
        contexts[3].target,
        ContextTarget::Payload {
            payload_hash: sha256(SOURCE)
        }
    );
    // Segments 1-2 cover "We use ... slowly." on lines 1-2.
    assert_eq!(
        contexts[3].range,
        Some(SourceRange {
            start_line: 1,
            start_col: 1,
            end_line: 2,
            end_col: 19
        })
    );

    // Citation list for rendering.
    assert_eq!(result.citations.len(), 2);
    assert_eq!(result.citations[0].index, 1);
    assert_eq!(result.citations[0].context_id, contexts[2].id);
    assert_eq!(
        result.citations[1].location,
        daftprompt_types::CitationLocation::Source {
            start_byte: 0,
            end_byte: SOURCE.find("\nThe").unwrap()
        }
    );

    // Run metadata.
    assert_eq!(result.run.agent, "explainer");
    assert_eq!(result.run.provider.as_deref(), Some("neuralwatt"));
    assert_eq!(result.run.model.as_deref(), Some("glm-5.3-flash"));
    assert_eq!(result.run.prompt_version.as_deref(), Some("explainer-v1"));
    assert_eq!(result.run.attempts, 1);
    let run_inputs: Vec<_> = result
        .run_inputs
        .iter()
        .map(|r| r.node_id.as_str())
        .collect();
    assert_eq!(run_inputs, vec!["node-target", q.id.as_str()]);

    // Tool calls: search, read_file, then submit, each with ordered seq.
    let tools: Vec<_> = result
        .tool_calls
        .iter()
        .map(|c| (c.seq, c.tool.as_str()))
        .collect();
    assert_eq!(tools, vec![(0, "search"), (1, "read_file"), (2, "submit")]);
    let read = &result.tool_calls[1];
    let output = result
        .tool_payloads
        .iter()
        .find(|p| Some(&p.hash) == read.output_hash.as_ref())
        .expect("read_file output payload");
    assert!(output.content.starts_with("src/limit.rs (lines 1-4 of 6)"));
    assert_eq!(output.hash, sha256(&output.content));
    assert_eq!(result.tool_calls[2].output_hash, None);
    assert!(result.tool_calls[2].input.contains("\"explanation\""));

    // The window covers the whole 3-segment source.
    let window = result.source_window.expect("window");
    assert_eq!((window.start_segment, window.end_segment), (1, 3));
    assert_eq!((window.start_byte, window.end_byte), (0, SOURCE.len()));
}

#[tokio::test]
async fn follow_up_chains_under_the_previous_explanation_without_a_second_label() {
    let stub = Arc::new(Stub::always(Vec::new(), answer("Because bursts.", vec![])));
    let mut request = request(SOURCE, "token bucket");
    request.turns = vec![ClarifyTurn {
        question_node_id: "q-old".to_owned(),
        question: "What is it?".to_owned(),
        explanation_node_id: "e-old".to_owned(),
        explanation: "A rate limiter.".to_owned(),
    }];
    request.question = "Why not a fixed window?".to_owned();
    request.author = None;
    let result = explainer(&stub, false).clarify(request).await.unwrap();

    assert_eq!(result.nodes[0].parent_id.as_deref(), Some("e-old"));
    assert_eq!(result.nodes[0].author, "unknown");
    assert!(result.labels.is_empty());
    let ids: Vec<_> = result
        .run_inputs
        .iter()
        .map(|r| r.node_id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![
            "node-target",
            "q-old",
            "e-old",
            result.question_node_id.as_str()
        ]
    );

    let prompt = stub.inputs.lock().unwrap()[0].prompt.clone();
    let earlier = prompt.find("A rate limiter.").expect("earlier turn");
    let question = prompt.find("Why not a fixed window?").expect("question");
    assert!(earlier < question);
}

#[tokio::test]
async fn text_only_mode_gives_no_tools_and_rejects_file_citations() {
    let stub = Arc::new(Stub::always(
        Vec::new(),
        answer("It is a limiter [1].", vec![source(1, 1)]),
    ));
    let result = explainer(&stub, false)
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap();
    assert_eq!(result.grounding, Grounding::TextOnly);
    assert!(stub.inputs.lock().unwrap()[0].toolbox.is_none());
    assert!(stub.inputs.lock().unwrap()[0].prompt.contains("text_only"));
    assert!(result.tool_payloads.is_empty());
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].tool, "submit");

    let stub = Arc::new(Stub::always(
        Vec::new(),
        answer("See [1].", vec![file("src/limit.rs", 1, 2)]),
    ));
    let error = explainer(&stub, false)
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap_err();
    assert!(matches!(error, ParseError::InvalidOutput { .. }), "{error}");
}

async fn rejected(
    source_text: &str,
    needle: &str,
    window: usize,
    calls: Vec<(&'static str, serde_json::Value)>,
    extraction: ExplainerExtraction,
) -> (ParseError, usize) {
    let stub = Arc::new(Stub::always(calls, extraction));
    let config = ExplainerConfig::default().with_window_segments(window);
    let explainer = Explainer::new(config, Handle(stub.clone())).with_project(project());
    let error = explainer
        .clarify(request(source_text, needle))
        .await
        .unwrap_err();
    (error, stub.calls())
}

#[tokio::test]
async fn invalid_explanations_consume_the_attempt_budget_and_never_return_partial_results() {
    let long = text(30);
    let cases: Vec<Case> = vec![
        ("empty explanation", read_limiter(), answer("  \n", vec![])),
        (
            "marker without a citation",
            read_limiter(),
            answer("See [1].", vec![]),
        ),
        (
            "marker past the list",
            read_limiter(),
            answer(
                "See [1] and [3].",
                vec![file("src/limit.rs", 1, 2), source(1, 1)],
            ),
        ),
        (
            "marker zero",
            read_limiter(),
            answer("See [0].", vec![source(1, 1)]),
        ),
        (
            "file never read",
            Vec::new(),
            answer("See [1].", vec![file("src/limit.rs", 1, 2)]),
        ),
        (
            "lines beyond what read_file returned",
            read_limiter(),
            answer("See [1].", vec![file("src/limit.rs", 3, 6)]),
        ),
        (
            "a different file",
            read_limiter(),
            answer("See [1].", vec![file("src/other.rs", 1, 1)]),
        ),
        (
            "inverted lines",
            read_limiter(),
            answer("See [1].", vec![file("src/limit.rs", 3, 2)]),
        ),
        (
            "segment zero",
            read_limiter(),
            answer("x", vec![source(0, 1)]),
        ),
        (
            "reversed segments",
            read_limiter(),
            answer("x", vec![source(5, 4)]),
        ),
    ];
    for (name, calls, extraction) in cases {
        let (error, model_calls) = rejected(&long, "line 15", 20, calls, extraction).await;
        assert!(
            matches!(error, ParseError::InvalidOutput { attempts: 3, .. }),
            "{name}: {error}"
        );
        assert_eq!(model_calls, 3, "{name}");
    }

    // Window is 2 segments each side of line 15: segments 13..=17.
    let (error, _) = rejected(
        &long,
        "line 15",
        2,
        Vec::new(),
        answer("x", vec![source(1, 2)]),
    )
    .await;
    assert!(error.to_string().contains("outside"), "{error}");
    let (error, _) = rejected(
        &long,
        "line 15",
        2,
        Vec::new(),
        answer("x", vec![source(15, 18)]),
    )
    .await;
    assert!(matches!(error, ParseError::InvalidOutput { .. }));
}

#[tokio::test]
async fn a_second_attempt_starts_fresh_and_counts_attempts() {
    // Attempt 1 reads the file but cites unread lines; attempt 2 reads nothing
    // and cites the lines attempt 1 read. A recorder shared across attempts
    // would accept it; a fresh one must not.
    let stub = Arc::new(Stub::new(|attempt, _| match attempt {
        1 => Step::Submit(
            read_limiter(),
            answer("Bad [1].", vec![file("src/limit.rs", 5, 6)]),
        ),
        2 => Step::Submit(
            Vec::new(),
            answer("Stale [1].", vec![file("src/limit.rs", 1, 4)]),
        ),
        _ => Step::Submit(
            read_limiter(),
            answer("Good [1].", vec![file("src/limit.rs", 1, 4)]),
        ),
    }));
    let result = explainer(&stub, true)
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap();
    assert_eq!(result.run.attempts, 3);
    // Only the successful attempt's calls are recorded.
    assert_eq!(result.tool_calls.len(), 3);

    let stub = Arc::new(Stub::new(|attempt, _| {
        if attempt == 1 {
            Step::Fail(ModelError::Invalid("no submit".to_owned()))
        } else {
            Step::Submit(Vec::new(), answer("ok", vec![]))
        }
    }));
    let result = explainer(&stub, true)
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap();
    assert_eq!(result.run.attempts, 2);
}

#[tokio::test]
async fn invalid_model_output_exhausts_a_configured_budget() {
    let stub = Arc::new(Stub::new(|_, _| {
        Step::Fail(ModelError::Invalid("turn limit".to_owned()))
    }));
    let config = ExplainerConfig::default().with_max_attempts(2);
    let error = Explainer::new(config, Handle(stub.clone()))
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, ParseError::InvalidOutput { attempts: 2, detail } if detail == "turn limit"),
        "{error}"
    );
    assert_eq!(stub.calls(), 2);
}

#[tokio::test]
async fn provider_failures_and_timeouts_fail_fast() {
    let stub = Arc::new(Stub::new(|_, _| {
        Step::Fail(ModelError::Provider("quota".to_owned()))
    }));
    let error = explainer(&stub, false)
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap_err();
    assert!(matches!(error, ParseError::Provider { .. }));
    assert_eq!(stub.calls(), 1);

    let stub = Arc::new(Stub::new(|_, _| Step::Hang));
    let config = ExplainerConfig::default().with_timeout(Duration::from_millis(20));
    let error = Explainer::new(config, Handle(stub.clone()))
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap_err();
    assert!(matches!(error, ParseError::Timeout { .. }), "{error}");
    assert_eq!(stub.calls(), 1);
}

async fn rejects(request: ClarifyRequest, config: ExplainerConfig) -> (ParseError, usize) {
    let stub = Arc::new(Stub::always(Vec::new(), answer("never used", vec![])));
    let error = Explainer::new(config, Handle(stub.clone()))
        .clarify(request)
        .await
        .unwrap_err();
    (error, stub.calls())
}

#[tokio::test]
async fn request_validation_happens_before_any_model_call() {
    let config = ExplainerConfig::default;

    // Hash mismatch.
    let mut bad = request(SOURCE, "token bucket");
    bad.source.payload_hash = sha256("something else");
    let (error, calls) = rejects(bad, config()).await;
    assert!(
        matches!(error, ParseError::PayloadMismatch { .. }),
        "{error}"
    );
    assert_eq!(calls, 0);

    // Empty text with a matching hash of the changed text still mismatches
    // when the text was edited after hashing.
    let mut edited = request(SOURCE, "token bucket");
    edited.source.text.push('!');
    let (error, _) = rejects(edited, config()).await;
    assert!(matches!(error, ParseError::PayloadMismatch { .. }));

    // Invalid targets.
    let base = request(SOURCE, "token bucket");
    for (name, start, end) in [
        ("empty", 3, 3),
        ("reversed", 9, 3),
        ("past the end", 5, SOURCE.len() + 1),
    ] {
        let mut bad = base.clone();
        bad.target.start_byte = start;
        bad.target.end_byte = end;
        let (error, calls) = rejects(bad, config()).await;
        assert!(
            matches!(error, ParseError::InvalidTarget { .. }),
            "{name}: {error}"
        );
        assert_eq!(calls, 0);
    }
    let accented = "caf\u{e9} au lait";
    let mut bad = request(accented, "au");
    bad.target.start_byte = 4; // inside the two-byte "é"
    let (error, _) = rejects(bad, config()).await;
    assert!(matches!(error, ParseError::InvalidTarget { .. }), "{error}");

    // Empty question.
    let mut bad = base.clone();
    bad.question = " \n".to_owned();
    let (error, _) = rejects(bad, config()).await;
    assert!(matches!(error, ParseError::InvalidRequest { .. }));

    // Oversized question, history, and source.
    let mut bad = base.clone();
    bad.question = "q".repeat(2049);
    let (error, calls) = rejects(bad, config()).await;
    assert!(
        matches!(error, ParseError::OversizedInput { .. }),
        "{error}"
    );
    assert_eq!(calls, 0);

    let turn = |n: usize| ClarifyTurn {
        question_node_id: format!("q{n}"),
        question: "q".to_owned(),
        explanation_node_id: format!("e{n}"),
        explanation: "e".repeat(10),
    };
    let mut bad = base.clone();
    bad.turns = (0..3).map(turn).collect();
    let (error, _) = rejects(bad.clone(), config().with_history_limits(2, 1000)).await;
    assert!(
        matches!(error, ParseError::OversizedInput { .. }),
        "{error}"
    );
    let (error, _) = rejects(bad, config().with_history_limits(5, 20)).await;
    assert!(
        matches!(error, ParseError::OversizedInput { .. }),
        "{error}"
    );

    let (error, _) = rejects(base.clone(), config().with_input_limits(10, 100)).await;
    assert!(matches!(error, ParseError::OversizedInput { .. }));
    let (error, _) = rejects(base, config().with_input_limits(1000, 2)).await;
    assert!(matches!(error, ParseError::OversizedInput { .. }));
}

#[tokio::test]
async fn context_window_is_numbered_clamped_and_marks_the_subject() {
    let long = text(60);
    let stub = Arc::new(Stub::always(Vec::new(), answer("ok", vec![source(25, 25)])));
    let result = explainer(&stub, false)
        .clarify(request(&long, "line 25"))
        .await
        .unwrap();
    let prompt = stub.inputs.lock().unwrap()[0].prompt.clone();
    assert!(prompt.contains("[5] line 5\n"));
    assert!(prompt.contains("[45] line 45\n"));
    assert!(!prompt.contains("[4] line 4\n"));
    assert!(!prompt.contains("[46] line 46\n"));
    assert!(prompt.contains("<subject>\nline 25\n</subject>"));
    let window = result.source_window.unwrap();
    assert_eq!((window.start_segment, window.end_segment), (5, 45));

    // Clamped at both ends of the payload.
    let stub = Arc::new(Stub::always(Vec::new(), answer("ok", vec![])));
    let result = explainer(&stub, false)
        .clarify(request(&long, "line 1\n"))
        .await
        .unwrap();
    assert_eq!(result.source_window.unwrap().start_segment, 1);
    let result = explainer(&stub, false)
        .clarify(request(&long, "line 60"))
        .await
        .unwrap();
    assert_eq!(result.source_window.unwrap().end_segment, 60);

    // A multi-segment subject extends the window from both its ends.
    let config = ExplainerConfig::default().with_window_segments(1);
    let result = Explainer::new(config, Handle(stub.clone()))
        .clarify(request(&long, "line 10\nline 11\nline 12"))
        .await
        .unwrap();
    let window = result.source_window.unwrap();
    assert_eq!((window.start_segment, window.end_segment), (9, 13));
}

#[tokio::test]
async fn only_a_clarification_label_is_ever_produced() {
    let stub = Arc::new(Stub::always(
        read_limiter(),
        answer("x [1]", vec![file("src/limit.rs", 1, 4)]),
    ));
    let result = explainer(&stub, true)
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap();
    assert!(
        result
            .labels
            .iter()
            .all(|l| l.kind == LabelKind::Clarification && l.status.is_none())
    );
    assert!(result.nodes.iter().all(|n| n.parent_id.is_some()));
}

#[tokio::test]
async fn clarifier_trait_object_works() {
    let stub = Arc::new(Stub::always(Vec::new(), answer("ok", vec![])));
    let clarifier: Arc<dyn Clarifier> = Arc::new(explainer(&stub, false));
    let result = clarifier
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap();
    assert_eq!(result.run.agent, "explainer");
}

// ---------------------------------------------------------------------------
// Per-call projects
// ---------------------------------------------------------------------------

fn other_project() -> Arc<dyn ProjectFiles> {
    Arc::new(InMemoryProject::new().with_file("docs/other.md", "# Other\nA different folder.\n"))
}

fn read_other() -> Calls {
    vec![(
        "read_file",
        json!({"path": "docs/other.md", "start_line": 1, "end_line": 2}),
    )]
}

#[tokio::test]
async fn a_per_call_project_gives_a_text_only_explainer_project_grounding() {
    let stub = Arc::new(Stub::always(
        read_other(),
        answer("See [1].", vec![file("docs/other.md", 1, 2)]),
    ));
    let explainer = explainer(&stub, false);
    assert_eq!(explainer.grounding(), Grounding::TextOnly);

    let result = explainer
        .clarify_with_project(request(SOURCE, "token bucket"), other_project())
        .await
        .unwrap();
    assert_eq!(result.grounding, Grounding::Project);
    assert!(stub.inputs.lock().unwrap()[0].toolbox.is_some());
    assert_eq!(result.run.project_id, None);

    // The next plain call is text-only again: the project was per call.
    let stub = Arc::new(Stub::always(
        Vec::new(),
        answer("It is a limiter [1].", vec![source(1, 1)]),
    ));
    let result = self::explainer(&stub, false)
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap();
    assert_eq!(result.grounding, Grounding::TextOnly);
}

#[tokio::test]
async fn a_per_call_project_overrides_the_default_one() {
    let stub = Arc::new(Stub::always(
        read_other(),
        answer("See [1].", vec![file("docs/other.md", 1, 2)]),
    ));
    let explainer = explainer(&stub, true);

    // The default project has no docs/other.md, so the call fails on its own.
    let error = explainer
        .clarify(request(SOURCE, "token bucket"))
        .await
        .unwrap_err();
    assert!(matches!(error, ParseError::InvalidOutput { .. }), "{error}");

    let result = explainer
        .clarify_with_project(request(SOURCE, "token bucket"), other_project())
        .await
        .unwrap();
    assert_eq!(result.grounding, Grounding::Project);
    assert_eq!(result.citations.len(), 1);
}

#[tokio::test]
async fn a_granted_scope_reads_its_folder_and_records_the_project_id() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("src")).unwrap();
    std::fs::write(folder.path().join("src/limit.rs"), LIMITER).unwrap();
    let scope = ProjectScope {
        project_id: "project-1".to_owned(),
        project_path_id: "path-1".to_owned(),
        root: folder.path().to_path_buf(),
    };

    let stub = Arc::new(Stub::always(
        read_limiter(),
        answer(
            "A bucket holds capacity [1].",
            vec![file("src/limit.rs", 2, 4)],
        ),
    ));
    let explainer = explainer(&stub, false);
    let result = explainer
        .clarify_in(request(SOURCE, "token bucket"), Some(&scope))
        .await
        .unwrap();
    assert_eq!(result.grounding, Grounding::Project);
    assert_eq!(result.run.project_id.as_deref(), Some("project-1"));

    // Through the trait object too.
    let clarifier: Arc<dyn Clarifier> = Arc::new(self::explainer(&stub, false));
    let result = clarifier
        .clarify_in(request(SOURCE, "token bucket"), Some(&scope))
        .await
        .unwrap();
    assert_eq!(result.run.project_id.as_deref(), Some("project-1"));

    // No scope keeps the default (here: none).
    let stub = Arc::new(Stub::always(
        Vec::new(),
        answer("It is a limiter [1].", vec![source(1, 1)]),
    ));
    let result = self::explainer(&stub, false)
        .clarify_in(request(SOURCE, "token bucket"), None)
        .await
        .unwrap();
    assert_eq!(result.grounding, Grounding::TextOnly);
    assert_eq!(result.run.project_id, None);
}

#[tokio::test]
async fn a_scope_over_a_missing_folder_is_rejected_before_any_model_call() {
    let scope = ProjectScope {
        project_id: "project-1".to_owned(),
        project_path_id: "path-1".to_owned(),
        root: "/definitely/not/a/folder".into(),
    };
    let stub = Arc::new(Stub::always(Vec::new(), answer("ok", vec![])));
    let error = explainer(&stub, false)
        .clarify_in(request(SOURCE, "token bucket"), Some(&scope))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ParseError::InvalidRequest { .. }),
        "{error}"
    );
    assert_eq!(stub.calls(), 0);
}
