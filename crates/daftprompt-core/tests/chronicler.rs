//! Deterministic Chronicler tests: a stub model drives the per-attempt git
//! toolbox over an in-memory history. No credentials, network, or git binary.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use daftprompt_core::chronicler::{
    Chronicler, ChroniclerConfig, ChroniclerInput, ChroniclerModel, ModelError,
};
use daftprompt_core::git_tools::{GIT_LOG, GIT_SHOW, GitHistory, InMemoryGit, MemoryCommit};
use daftprompt_core::{Briefer, ProjectScope};
use daftprompt_types::{
    AgentTask, AgentTaskRequest, BriefingCitation, BriefingExtraction, BriefingNoticeKind,
    BriefingResult, CitationLocation, ContextTarget, ParseError, RunStatus,
};
use serde_json::{Value, json};

fn hash(prefix: &str) -> String {
    format!("{prefix:0<40}")
}

fn scope() -> ProjectScope {
    ProjectScope {
        project_id: "project-1".to_owned(),
        root: PathBuf::from("/does/not/matter"),
    }
}

fn history() -> Arc<dyn GitHistory> {
    Arc::new(InMemoryGit::new(vec![
        MemoryCommit::new(&hash("a1b2c3d4"), "Add the importer"),
        MemoryCommit::new(&hash("a1b2c3d9"), "Test the importer"),
        MemoryCommit::new(&hash("f00dfeed"), "Fix a typo"),
    ]))
}

type Calls = Vec<(&'static str, Value)>;

enum Step {
    /// Run these `(tool, args)` calls on the toolbox, then submit.
    Submit(Calls, BriefingExtraction),
    Fail(ModelError),
}

type Script = dyn Fn(usize, &ChroniclerInput) -> Step + Send + Sync;

/// A stub model that records every prompt it receives.
struct Stub {
    script: Box<Script>,
    prompts: Mutex<Vec<String>>,
}

impl Stub {
    fn new(script: impl Fn(usize, &ChroniclerInput) -> Step + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            script: Box::new(script),
            prompts: Mutex::new(Vec::new()),
        })
    }

    fn always(calls: Calls, extraction: BriefingExtraction) -> Arc<Self> {
        Self::new(move |_, _| Step::Submit(calls.clone(), extraction.clone()))
    }

    fn attempts(&self) -> usize {
        self.prompts.lock().unwrap().len()
    }
}

struct Handle(Arc<Stub>);

impl ChroniclerModel for Handle {
    fn brief(
        &self,
        input: ChroniclerInput,
    ) -> daftprompt_core::chronicler::BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        Box::pin(async move {
            let attempt = {
                let mut prompts = self.0.prompts.lock().unwrap();
                prompts.push(input.prompt.clone());
                prompts.len()
            };
            match (self.0.script)(attempt, &input) {
                Step::Submit(calls, extraction) => {
                    for (name, args) in calls {
                        input.toolset.git().call(name, args);
                    }
                    Ok(extraction)
                }
                Step::Fail(error) => Err(error),
            }
        })
    }
}

fn chronicler(stub: &Arc<Stub>) -> Chronicler<Handle> {
    chronicler_over(stub, history())
}

fn chronicler_over(stub: &Arc<Stub>, history: Arc<dyn GitHistory>) -> Chronicler<Handle> {
    Chronicler::new(ChroniclerConfig::default(), Handle(stub.clone()))
        .with_history(move |_| history.clone())
}

fn summary(text: &str, citations: Vec<BriefingCitation>) -> BriefingExtraction {
    BriefingExtraction {
        summary: text.to_owned(),
        citations,
    }
}

fn commit(prefix: &str) -> BriefingCitation {
    BriefingCitation::Commit {
        hash: prefix.to_owned(),
    }
}

fn log_all() -> Calls {
    vec![(GIT_LOG, json!({}))]
}

async fn run(stub: &Arc<Stub>, request: AgentTaskRequest) -> Result<BriefingResult, ParseError> {
    chronicler(stub).brief(request, &scope()).await
}

#[tokio::test]
async fn valid_commit_citations_resolve_to_full_hashes() {
    let stub = Stub::always(
        log_all(),
        summary(
            "The importer landed [1] with tests [2].",
            vec![commit("a1b2c3d4"), commit(&hash("a1b2c3d9").to_uppercase())],
        ),
    );
    let result = run(&stub, AgentTaskRequest::default()).await.unwrap();

    assert_eq!(result.project_id, "project-1");
    assert_eq!(result.task, AgentTask::Default);
    assert_eq!(result.prompt_payload, None);
    assert_eq!(
        result.summary_payload.content,
        "The importer landed [1] with tests [2]."
    );
    assert!(result.notices.is_empty());

    // One root summary node; contexts: the summary text, then each citation.
    assert_eq!(result.nodes.len(), 1);
    let node = &result.nodes[0];
    assert_eq!(node.id, result.summary_node_id);
    assert_eq!(node.parent_id, None);
    assert_eq!(node.author, "agent");
    assert_eq!(node.run_id.as_deref(), Some(result.run.id.as_str()));
    assert_eq!(result.node_contexts.len(), 3);
    assert_eq!(result.node_contexts[0].seq, 0);
    assert_eq!(
        result.node_contexts[0].target,
        ContextTarget::Payload {
            payload_hash: result.summary_payload.hash.clone()
        }
    );
    assert_eq!(
        result.node_contexts[1].target,
        ContextTarget::Commit {
            hash: hash("a1b2c3d4")
        }
    );
    assert_eq!(result.node_contexts[1].seq, 1);
    assert_eq!(result.node_contexts[1].range, None);
    assert_eq!(
        result.node_contexts[2].target,
        ContextTarget::Commit {
            hash: hash("a1b2c3d9")
        }
    );

    assert_eq!(result.citations.len(), 2);
    assert_eq!(result.citations[0].index, 1);
    assert_eq!(result.citations[0].context_id, result.node_contexts[1].id);
    assert_eq!(
        result.citations[0].location,
        CitationLocation::Commit {
            hash: hash("a1b2c3d4"),
            subject: "Add the importer".to_owned()
        }
    );

    // Run metadata, tool calls (with submit), and tool output payloads.
    assert_eq!(result.run.agent, "chronicler");
    assert_eq!(result.run.status, RunStatus::Done);
    assert_eq!(result.run.attempts, 1);
    assert_eq!(result.run.prompt_version.as_deref(), Some("chronicler-v1"));
    assert_eq!(result.run.project_id.as_deref(), Some("project-1"));
    assert_eq!(result.run.provider.as_deref(), Some("neuralwatt"));
    assert!(result.run_inputs.is_empty(), "no prompt node, no run input");
    let tools: Vec<&str> = result.tool_calls.iter().map(|c| c.tool.as_str()).collect();
    assert_eq!(tools, ["git_log", "submit"]);
    let output = result.tool_calls[0].output_hash.as_ref().unwrap();
    let payload = result
        .tool_payloads
        .iter()
        .find(|p| &p.hash == output)
        .expect("tool output payload");
    assert!(payload.content.contains(&hash("f00dfeed")));
    assert_eq!(stub.attempts(), 1);
    assert!(stub.prompts.lock().unwrap()[0].contains("Summarize recent work"));
}

#[tokio::test]
async fn show_also_makes_a_commit_citable() {
    let stub = Stub::always(
        vec![(GIT_SHOW, json!({"hash": "f00dfee"}))],
        summary("A typo was fixed [1].", vec![commit("f00dfeed")]),
    );
    let result = run(&stub, AgentTaskRequest::default()).await.unwrap();
    assert_eq!(result.citations.len(), 1);
}

#[tokio::test]
async fn an_unseen_hash_is_invalid_even_if_it_exists() {
    // The log is cut to one commit; the second commit exists but was never shown.
    let stub = Stub::always(
        vec![(GIT_LOG, json!({"count": 1}))],
        summary("Tests exist [1].", vec![commit("a1b2c3d9")]),
    );
    let error = run(&stub, AgentTaskRequest::default()).await.unwrap_err();
    let ParseError::InvalidOutput { detail, attempts } = error else {
        panic!("expected invalid output");
    };
    assert_eq!(attempts, 3);
    assert!(detail.contains("was not returned"), "{detail}");
    assert_eq!(stub.attempts(), 3);
}

#[tokio::test]
async fn an_ambiguous_prefix_is_invalid_but_a_longer_one_resolves() {
    // `a1b2c3d` starts both importer commits.
    let ambiguous = Stub::always(
        log_all(),
        summary("The importer [1].", vec![commit("a1b2c3d")]),
    );
    let error = run(&ambiguous, AgentTaskRequest::default())
        .await
        .unwrap_err();
    let ParseError::InvalidOutput { detail, .. } = error else {
        panic!("expected invalid output");
    };
    assert!(detail.contains("matches 2 commits"), "{detail}");

    let resolved = Stub::always(
        log_all(),
        summary("The importer [1].", vec![commit("a1b2c3d4")]),
    );
    assert!(run(&resolved, AgentTaskRequest::default()).await.is_ok());
}

#[tokio::test]
async fn malformed_and_file_citations_are_invalid() {
    for citation in [
        commit("abc"),
        commit("not-hex!"),
        BriefingCitation::File {
            path: "src/lib.rs".to_owned(),
            start_line: 1,
            end_line: 2,
        },
    ] {
        let stub = Stub::always(log_all(), summary("Something [1].", vec![citation]));
        let error = run(&stub, AgentTaskRequest::default()).await.unwrap_err();
        assert!(
            matches!(error, ParseError::InvalidOutput { .. }),
            "{error:?}"
        );
    }
}

#[tokio::test]
async fn a_marker_without_a_citation_is_invalid() {
    for text in ["See [2].", "See [0].", "See [1] and [3]."] {
        let stub = Stub::always(log_all(), summary(text, vec![commit("f00dfeed")]));
        let error = run(&stub, AgentTaskRequest::default()).await.unwrap_err();
        let ParseError::InvalidOutput { detail, .. } = error else {
            panic!("expected invalid output for {text}");
        };
        assert!(detail.contains("refers to citation"), "{detail}");
    }
    // Other bracket text is not a marker.
    let stub = Stub::always(log_all(), summary("An array a[i] and [x].", vec![]));
    assert!(run(&stub, AgentTaskRequest::default()).await.is_ok());
}

#[tokio::test]
async fn an_empty_summary_is_invalid() {
    let stub = Stub::always(log_all(), summary("  \n", vec![]));
    let error = run(&stub, AgentTaskRequest::default()).await.unwrap_err();
    assert!(matches!(error, ParseError::InvalidOutput { .. }));
}

#[tokio::test]
async fn invalid_output_is_retried_and_only_the_accepted_attempt_is_recorded() {
    let stub = Stub::new(|attempt, _| {
        if attempt == 1 {
            // Cites a commit it never looked up.
            Step::Submit(
                vec![(GIT_LOG, json!({"count": 1}))],
                summary("Wrong [1].", vec![commit("f00dfeed")]),
            )
        } else {
            Step::Submit(
                vec![
                    (GIT_LOG, json!({})),
                    (GIT_SHOW, json!({"hash": "f00dfeed"})),
                ],
                summary("Right [1].", vec![commit("f00dfeed")]),
            )
        }
    });
    let result = run(&stub, AgentTaskRequest::default()).await.unwrap();
    assert_eq!(stub.attempts(), 2);
    assert_eq!(result.run.attempts, 2);
    let tools: Vec<&str> = result.tool_calls.iter().map(|c| c.tool.as_str()).collect();
    assert_eq!(tools, ["git_log", "git_show", "submit"]);
    assert_eq!(result.summary_payload.content, "Right [1].");
}

#[tokio::test]
async fn provider_errors_are_not_retried() {
    let stub = Stub::new(|_, _| Step::Fail(ModelError::Provider("down".to_owned())));
    let error = run(&stub, AgentTaskRequest::default()).await.unwrap_err();
    assert!(matches!(error, ParseError::Provider { .. }));
    assert_eq!(stub.attempts(), 1);
}

#[tokio::test]
async fn a_custom_prompt_replaces_the_task_and_creates_the_prompt_node() {
    let stub = Stub::always(
        log_all(),
        summary("Only a typo fix [1].", vec![commit("f00dfeed")]),
    );
    let request = AgentTaskRequest {
        prompt: Some("  what changed in auth recently?  ".to_owned()),
        author: Some("ada".to_owned()),
    };
    let result = run(&stub, request).await.unwrap();

    assert_eq!(result.task, AgentTask::Prompt);
    let prompt = result.prompt_payload.as_ref().expect("prompt payload");
    assert_eq!(prompt.content, "what changed in auth recently?");

    assert_eq!(result.nodes.len(), 2);
    let (prompt_node, summary_node) = (&result.nodes[0], &result.nodes[1]);
    assert_eq!(prompt_node.parent_id, None);
    assert_eq!(prompt_node.author, "ada");
    assert_eq!(prompt_node.run_id, None);
    assert_eq!(summary_node.id, result.summary_node_id);
    assert_eq!(
        summary_node.parent_id.as_deref(),
        Some(prompt_node.id.as_str())
    );
    assert_eq!(summary_node.author, "agent");

    // Prompt context first (the whole prompt payload), then the summary's.
    assert_eq!(
        result.node_contexts[0].target,
        ContextTarget::Payload {
            payload_hash: prompt.hash.clone()
        }
    );
    assert_eq!(result.node_contexts[0].node_id, prompt_node.id);
    assert_eq!(result.node_contexts[1].node_id, summary_node.id);
    assert_eq!(result.run_inputs.len(), 1);
    assert_eq!(result.run_inputs[0].node_id, prompt_node.id);

    let seen = stub.prompts.lock().unwrap()[0].clone();
    assert!(seen.contains("<user_prompt>\nwhat changed in auth recently?\n</user_prompt>"));
    assert!(!seen.contains("Summarize recent work"));
}

#[tokio::test]
async fn a_blank_prompt_is_the_default_task_and_the_author_defaults_to_unknown() {
    let stub = Stub::always(log_all(), summary("Fine.", vec![]));
    let blank = AgentTaskRequest {
        prompt: Some("   ".to_owned()),
        author: Some("ada".to_owned()),
    };
    let result = run(&stub, blank).await.unwrap();
    assert_eq!(result.task, AgentTask::Default);
    assert_eq!(result.nodes.len(), 1);

    let anonymous = AgentTaskRequest {
        prompt: Some("anything?".to_owned()),
        author: None,
    };
    let result = run(&stub, anonymous).await.unwrap();
    assert_eq!(result.nodes[0].author, "unknown");
}

#[tokio::test]
async fn an_oversized_prompt_is_rejected_before_any_model_call() {
    let stub = Stub::always(log_all(), summary("Fine.", vec![]));
    let request = AgentTaskRequest {
        prompt: Some("x".repeat(3000)),
        author: None,
    };
    let error = run(&stub, request).await.unwrap_err();
    assert!(
        matches!(error, ParseError::OversizedInput { .. }),
        "{error:?}"
    );
    let request = AgentTaskRequest {
        prompt: None,
        author: Some("a".repeat(500)),
    };
    let error = run(&stub, request).await.unwrap_err();
    assert!(
        matches!(error, ParseError::InvalidRequest { .. }),
        "{error:?}"
    );
    assert_eq!(stub.attempts(), 0);
}

#[tokio::test]
async fn a_folder_that_is_not_a_repository_is_a_briefing_with_a_notice() {
    let stub = Stub::always(
        log_all(),
        summary("The project is not under version control.", vec![]),
    );
    let chronicler = chronicler_over(&stub, Arc::new(InMemoryGit::not_a_repository()));
    let result = chronicler
        .brief(AgentTaskRequest::default(), &scope())
        .await
        .unwrap();
    assert_eq!(result.notices.len(), 1);
    assert_eq!(
        result.notices[0].kind,
        BriefingNoticeKind::NotAGitRepository
    );
    assert!(result.citations.is_empty());
    assert!(result.tool_calls[0].output_hash.is_some());

    let chronicler = chronicler_over(&stub, Arc::new(InMemoryGit::unavailable()));
    let result = chronicler
        .brief(AgentTaskRequest::default(), &scope())
        .await
        .unwrap();
    assert_eq!(result.notices[0].kind, BriefingNoticeKind::GitUnavailable);
}

#[tokio::test]
async fn a_cut_result_adds_a_truncated_notice() {
    let stub = Stub::always(
        vec![(GIT_LOG, json!({"count": 1}))],
        summary("One.", vec![]),
    );
    let result = run(&stub, AgentTaskRequest::default()).await.unwrap();
    assert_eq!(result.notices.len(), 1);
    assert_eq!(result.notices[0].kind, BriefingNoticeKind::Truncated);
}

#[tokio::test]
async fn the_briefer_trait_reports_its_configuration() {
    let stub = Stub::always(log_all(), summary("Fine.", vec![]));
    let briefer: Arc<dyn Briefer> = Arc::new(chronicler(&stub));
    let info = briefer.info();
    assert_eq!(info.kind, "chronicler");
    assert_eq!(info.prompt_version, "chronicler-v1");
    assert_eq!(info.model.as_deref(), Some("glm-5.3-flash"));
    let result = briefer
        .brief(AgentTaskRequest::default(), &scope())
        .await
        .unwrap();
    assert_eq!(result.run.agent, "chronicler");
}
