//! Deterministic Surveyor tests: a stub model drives the per-attempt file
//! toolbox over an in-memory project. No credentials, network or filesystem.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use daftprompt_core::project_tools::{InMemoryProject, ProjectFiles, READ_FILE};
use daftprompt_core::surveyor::{
    ModelError, Surveyor, SurveyorConfig, SurveyorInput, SurveyorModel,
};
use daftprompt_core::{Briefer, ProjectScope};
use daftprompt_types::{
    AgentTask, AgentTaskRequest, BriefingCitation, BriefingExtraction, BriefingNoticeKind,
    BriefingResult, CitationLocation, ContextTarget, LanguageId, ParseError, RunStatus,
};
use serde_json::{Value, json};

fn scope() -> ProjectScope {
    ProjectScope {
        project_id: "project-1".to_owned(),
        project_path_id: "path-1".to_owned(),
        root: PathBuf::from("/does/not/matter"),
    }
}

fn project() -> Arc<dyn ProjectFiles> {
    Arc::new(
        InMemoryProject::new()
            .with_file(
                "README.md",
                "# Demo\n\nA demo importer.\n\n## Layout\n\n- crates/core\n",
            )
            .with_file("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n")
            .with_file(
                "crates/core/Cargo.toml",
                "[package]\nname = \"demo-core\"\n",
            )
            .with_file("docs/guide.md", "Guide\n")
            .with_file("custom.txt", "first\nsecond\n"),
    )
}

type Calls = Vec<(&'static str, Value)>;

enum Step {
    /// Run these `(tool, args)` calls on the toolbox, then submit.
    Submit(Calls, BriefingExtraction),
    Fail(ModelError),
}

type Script = dyn Fn(usize, &SurveyorInput) -> Step + Send + Sync;

/// A stub model that records every prompt it receives.
struct Stub {
    script: Box<Script>,
    prompts: Mutex<Vec<String>>,
}

impl Stub {
    fn new(script: impl Fn(usize, &SurveyorInput) -> Step + Send + Sync + 'static) -> Arc<Self> {
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

impl SurveyorModel for Handle {
    fn brief(
        &self,
        input: SurveyorInput,
    ) -> daftprompt_core::surveyor::BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        Box::pin(async move {
            let attempt = {
                let mut prompts = self.0.prompts.lock().unwrap();
                prompts.push(input.prompt.clone());
                prompts.len()
            };
            match (self.0.script)(attempt, &input) {
                Step::Submit(calls, extraction) => {
                    for (name, args) in calls {
                        input.toolset.files().call(name, args);
                    }
                    Ok(extraction)
                }
                Step::Fail(error) => Err(error),
            }
        })
    }
}

fn surveyor(stub: &Arc<Stub>) -> Surveyor<Handle> {
    Surveyor::new(SurveyorConfig::default(), Handle(stub.clone())).with_project(|_| project())
}

fn summary(text: &str, citations: Vec<BriefingCitation>) -> BriefingExtraction {
    BriefingExtraction {
        summary: text.to_owned(),
        citations,
    }
}

fn file(path: &str, start_line: u32, end_line: u32) -> BriefingCitation {
    BriefingCitation::File {
        path: path.to_owned(),
        start_line,
        end_line,
    }
}

fn read_readme() -> Calls {
    vec![(READ_FILE, json!({"path": "README.md"}))]
}

async fn run(stub: &Arc<Stub>, request: AgentTaskRequest) -> Result<BriefingResult, ParseError> {
    surveyor(stub).brief(request, &scope()).await
}

#[tokio::test]
async fn valid_file_citations_become_contexts_with_ranges_and_hashes() {
    let stub = Stub::always(
        vec![
            (READ_FILE, json!({"path": "README.md"})),
            (READ_FILE, json!({"path": "Cargo.toml"})),
        ],
        summary(
            "A demo importer [1] in a workspace [2].",
            vec![file("README.md", 3, 3), file("./Cargo.toml", 1, 2)],
        ),
    );
    let result = run(&stub, AgentTaskRequest::default()).await.unwrap();

    assert_eq!(result.project_id, "project-1");
    assert_eq!(result.task, AgentTask::Default);
    assert_eq!(result.prompt_payload, None);
    assert!(result.notices.is_empty());
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.nodes[0].parent_id, None);
    assert_eq!(result.nodes[0].author, "agent");

    assert_eq!(result.node_contexts.len(), 3);
    assert_eq!(result.node_contexts[0].seq, 0);
    let ContextTarget::File { path, content_hash } = &result.node_contexts[1].target else {
        panic!("expected a file target");
    };
    assert_eq!(path, "README.md");
    assert_eq!(content_hash.as_ref().map(String::len), Some(64));
    let range = result.node_contexts[1].range.as_ref().unwrap();
    assert_eq!((range.start_line, range.end_line), (3, 4));
    assert_eq!(
        result.citations[1].location,
        CitationLocation::File {
            path: "Cargo.toml".to_owned(),
            start_line: 1,
            end_line: 2
        }
    );

    assert_eq!(result.run.agent, "surveyor");
    assert_eq!(result.run.status, RunStatus::Done);
    assert_eq!(result.run.attempts, 1);
    assert_eq!(result.run.prompt_version.as_deref(), Some("surveyor-v3"));
    assert_eq!(result.run.project_id.as_deref(), Some("project-1"));
    let tools: Vec<&str> = result.tool_calls.iter().map(|c| c.tool.as_str()).collect();
    assert_eq!(tools.last(), Some(&"submit"));
    assert!(tools.contains(&"list_files"));
    assert!(tools.contains(&"read_file"));
    assert_eq!(stub.attempts(), 1);
}

#[tokio::test]
async fn the_result_and_the_prompt_include_the_computed_profile() {
    let stub = Stub::always(
        read_readme(),
        summary("Fine [1].", vec![file("README.md", 1, 1)]),
    );
    let result = run(&stub, AgentTaskRequest::default()).await.unwrap();

    let profile = result.profile.expect("a profile");
    assert_eq!(profile.languages.len(), 1);
    assert_eq!(profile.languages[0].language, LanguageId::Rust);
    let root = &profile.languages[0].projects[0];
    assert!(root.workspace);
    assert_eq!(root.packages, ["demo-core"]);
    assert_eq!(
        profile.key_files,
        ["README.md", "docs/guide.md", "Cargo.toml"]
    );

    let prompt = stub.prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains("## Project profile"));
    assert!(prompt.contains("- rust in `.`, cargo, workspace of 1 package(s): demo-core"));
    assert!(prompt.contains("- README.md\n- docs/guide.md\n- Cargo.toml\n"));
    assert!(prompt.contains("Summarize this project"));
}

#[tokio::test]
async fn lines_that_were_not_read_are_invalid() {
    // The custom source is not an entrypoint; only its first line is read.
    let stub = Stub::always(
        vec![(
            READ_FILE,
            json!({"path": "custom.txt", "start_line": 1, "end_line": 1}),
        )],
        summary("Layout [1].", vec![file("custom.txt", 2, 2)]),
    );
    let error = run(&stub, AgentTaskRequest::default()).await.unwrap_err();
    let ParseError::InvalidOutput { detail, attempts } = error else {
        panic!("expected invalid output");
    };
    assert_eq!(attempts, 3);
    assert!(detail.contains("no read_file call returned"), "{detail}");
    assert_eq!(stub.attempts(), 3);

    // A file that was never read at all is invalid too, as is a bad range.
    for citation in [
        file("custom.txt", 1, 1),
        file("README.md", 0, 1),
        file("README.md", 3, 2),
    ] {
        let stub = Stub::always(read_readme(), summary("X [1].", vec![citation]));
        let error = run(&stub, AgentTaskRequest::default()).await.unwrap_err();
        assert!(
            matches!(error, ParseError::InvalidOutput { .. }),
            "{error:?}"
        );
    }
}

#[tokio::test]
async fn a_commit_citation_is_rejected() {
    let stub = Stub::always(
        read_readme(),
        summary(
            "A change [1].",
            vec![BriefingCitation::Commit {
                hash: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            }],
        ),
    );
    let error = run(&stub, AgentTaskRequest::default()).await.unwrap_err();
    let ParseError::InvalidOutput { detail, .. } = error else {
        panic!("expected invalid output");
    };
    assert!(detail.contains("may cite files only"), "{detail}");
}

#[tokio::test]
async fn a_marker_without_a_citation_and_an_empty_summary_are_invalid() {
    for text in ["See [2].", "See [0].", "  "] {
        let stub = Stub::always(read_readme(), summary(text, vec![file("README.md", 1, 1)]));
        let error = run(&stub, AgentTaskRequest::default()).await.unwrap_err();
        assert!(matches!(error, ParseError::InvalidOutput { .. }), "{text}");
    }
}

#[tokio::test]
async fn invalid_output_is_retried_and_only_the_accepted_attempt_is_recorded() {
    let stub = Stub::new(|attempt, _| {
        if attempt == 1 {
            Step::Submit(
                vec![],
                summary("Unread [1].", vec![file("custom.txt", 1, 1)]),
            )
        } else {
            Step::Submit(
                read_readme(),
                summary("Read [1].", vec![file("README.md", 1, 1)]),
            )
        }
    });
    let result = run(&stub, AgentTaskRequest::default()).await.unwrap();
    assert_eq!(result.run.attempts, 2);
    let tools: Vec<&str> = result.tool_calls.iter().map(|c| c.tool.as_str()).collect();
    assert_eq!(tools.last(), Some(&"submit"));
    assert_eq!(tools.iter().filter(|tool| **tool == "submit").count(), 1);
    assert!(
        result
            .tool_payloads
            .iter()
            .all(|p| !p.content.contains("Unread [1]."))
    );
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
        read_readme(),
        summary("It imports things [1].", vec![file("README.md", 3, 3)]),
    );
    let request = AgentTaskRequest {
        prompt: Some("  what does it import?  ".to_owned()),
        author: Some("ada".to_owned()),
        ..Default::default()
    };
    let result = run(&stub, request).await.unwrap();

    assert_eq!(result.task, AgentTask::Prompt);
    let prompt = result.prompt_payload.as_ref().expect("prompt payload");
    assert_eq!(prompt.content, "what does it import?");
    assert_eq!(result.nodes.len(), 2);
    assert_eq!(result.nodes[0].author, "ada");
    assert_eq!(
        result.nodes[1].parent_id.as_deref(),
        Some(result.nodes[0].id.as_str())
    );
    assert_eq!(result.run_inputs.len(), 1);
    assert!(result.profile.is_some());

    let seen = stub.prompts.lock().unwrap()[0].clone();
    assert!(seen.contains("<user_prompt>\nwhat does it import?\n</user_prompt>"));
    assert!(!seen.contains("Summarize this project"));
    assert!(seen.contains("## Project profile"));
}

#[tokio::test]
async fn an_oversized_prompt_is_rejected_before_any_model_call() {
    let stub = Stub::always(read_readme(), summary("Fine.", vec![]));
    let request = AgentTaskRequest {
        prompt: Some("x".repeat(3000)),
        author: None,
        ..Default::default()
    };
    let error = run(&stub, request).await.unwrap_err();
    assert!(
        matches!(error, ParseError::OversizedInput { .. }),
        "{error:?}"
    );
    assert_eq!(stub.attempts(), 0);
}

#[tokio::test]
async fn an_empty_project_still_gets_an_overview_with_an_empty_profile() {
    let stub = Stub::always(vec![], summary("The folder is empty.", vec![]));
    let result = Surveyor::new(SurveyorConfig::default(), Handle(stub.clone()))
        .with_project(|_| Arc::new(InMemoryProject::new()))
        .brief(AgentTaskRequest::default(), &scope())
        .await
        .unwrap();
    let profile = result.profile.unwrap();
    assert!(profile.languages.is_empty() && profile.key_files.is_empty());
    assert!(stub.prompts.lock().unwrap()[0].contains("none of Rust"));
}

#[tokio::test]
async fn a_cut_listing_adds_a_truncated_notice() {
    let stub = Stub::always(vec![("list_files", json!({}))], summary("Big.", vec![]));
    let config = SurveyorConfig::default().with_tool_config(
        daftprompt_core::project_tools::ProjectToolConfig {
            max_list_files: 1,
            ..Default::default()
        },
    );
    let result = Surveyor::new(config, Handle(stub.clone()))
        .with_project(|_| project())
        .brief(AgentTaskRequest::default(), &scope())
        .await
        .unwrap();
    assert_eq!(result.notices.len(), 1);
    assert_eq!(result.notices[0].kind, BriefingNoticeKind::Truncated);
}

#[tokio::test]
async fn the_briefer_trait_reports_its_configuration() {
    let stub = Stub::always(read_readme(), summary("Fine.", vec![]));
    let briefer: Arc<dyn Briefer> = Arc::new(surveyor(&stub));
    let info = briefer.info();
    assert_eq!(info.kind, "surveyor");
    assert_eq!(info.prompt_version, "surveyor-v3");
    assert_eq!(info.model.as_deref(), Some("glm-5.3-flash"));
    let result = briefer
        .brief(AgentTaskRequest::default(), &scope())
        .await
        .unwrap();
    assert_eq!(result.run.agent, "surveyor");
}

#[tokio::test]
async fn preloaded_readme_can_be_cited_without_a_model_read_and_extra_context_uses_tools() {
    let stub = Stub::new(|_, input| {
        assert!(input.prompt.contains("A demo importer."));
        assert!(input.prompt.contains("docs/guide.md"));
        assert!(input.toolset.files().recorder().covers("README.md", 3, 3));
        Step::Submit(
            vec![],
            summary(
                "Importer in a workspace [1][2].",
                vec![file("README.md", 3, 3), file("Cargo.toml", 1, 1)],
            ),
        )
    });
    let result = run(&stub, AgentTaskRequest::default()).await.unwrap();
    assert_eq!(result.citations.len(), 2);
    assert_eq!(
        result
            .tool_calls
            .iter()
            .filter(|c| c.tool == READ_FILE && c.input.contains("README.md"))
            .count(),
        1
    );
}

#[tokio::test]
async fn configured_citation_limit_is_visible_and_rejection_can_be_corrected() {
    let stub = Stub::new(|attempt, input| {
        assert!(input.prompt.contains("at most 1 citations"));
        assert!(input.prompt.contains("at most 2048 UTF-8 bytes"));
        if attempt == 1 {
            assert!(!input.prompt.contains("## Previous attempt rejected"));
            Step::Submit(
                read_readme(),
                summary(
                    "Read [1].",
                    vec![file("README.md", 1, 1), file("README.md", 2, 2)],
                ),
            )
        } else {
            assert!(input.prompt.contains("2 citations exceed the limit of 1"));
            assert!(input.prompt.contains("new complete submission"));
            Step::Submit(
                read_readme(),
                summary("Read [1].", vec![file("README.md", 1, 1)]),
            )
        }
    });
    let agent = Surveyor::new(
        SurveyorConfig::default().with_output_limits(1, 2048),
        Handle(stub.clone()),
    )
    .with_project(|_| project());
    let result = agent
        .brief(AgentTaskRequest::default(), &scope())
        .await
        .unwrap();
    assert_eq!(result.run.attempts, 2);
    assert_eq!(result.citations.len(), 1);
}
