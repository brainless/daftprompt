//! Deterministic Operator tests: a stub model drives the per-attempt file
//! toolbox over an in-memory project. No credentials, network or filesystem.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use daftprompt_core::ProjectScope;
use daftprompt_core::operator::{
    ModelError, Operator, OperatorConfig, OperatorInput, OperatorModel,
};
use daftprompt_core::project_tools::{InMemoryProject, ProjectFiles, READ_FILE};
use daftprompt_types::{
    AgentTask, AgentTaskRequest, BriefingCitation, CitationLocation, CommandKind, CommandOrigin,
    CommandsExtraction, CommandsResult, ContextTarget, ExtractedCommand, LanguageId, ParseError,
    RunStatus,
};
use serde_json::{Value, json};

fn scope() -> ProjectScope {
    ProjectScope {
        project_id: "project-1".to_owned(),
        project_path_id: "path-1".to_owned(),
        root: PathBuf::from("/does/not/matter"),
    }
}

/// Candidates (cargo workspace, no scripts):
/// 1 build, 2 test, 3 clippy, 4 fmt, 5 `cargo run -p app`.
fn project() -> Arc<dyn ProjectFiles> {
    Arc::new(
        InMemoryProject::new()
            .with_file(
                "README.md",
                "# Demo\n\n## Run\n\nStart the site with `make site`.\n",
            )
            .with_file("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n")
            .with_file("crates/app/Cargo.toml", "[package]\nname = \"app\"\n")
            .with_file("crates/app/src/main.rs", "fn main() {}\n")
            .with_file("web/package.json", "{\"scripts\":{\"dev\":\"vite\"}}")
            .with_file("web/pnpm-lock.yaml", ""),
    )
}

type Calls = Vec<(&'static str, Value)>;

enum Step {
    Submit(Calls, CommandsExtraction),
    Fail(ModelError),
}

type Script = dyn Fn(usize, &OperatorInput) -> Step + Send + Sync;

struct Stub {
    script: Box<Script>,
    prompts: Mutex<Vec<String>>,
}

impl Stub {
    fn new(script: impl Fn(usize, &OperatorInput) -> Step + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            script: Box::new(script),
            prompts: Mutex::new(Vec::new()),
        })
    }

    fn always(calls: Calls, extraction: CommandsExtraction) -> Arc<Self> {
        Self::new(move |_, _| Step::Submit(calls.clone(), extraction.clone()))
    }

    fn attempts(&self) -> usize {
        self.prompts.lock().unwrap().len()
    }
}

struct Handle(Arc<Stub>);

impl OperatorModel for Handle {
    fn brief(
        &self,
        input: OperatorInput,
    ) -> daftprompt_core::operator::BoxFuture<'_, Result<CommandsExtraction, ModelError>> {
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

fn operator(stub: &Arc<Stub>) -> Operator<Handle> {
    operator_with(stub, OperatorConfig::default())
}

fn operator_with(stub: &Arc<Stub>, config: OperatorConfig) -> Operator<Handle> {
    Operator::new(config, Handle(stub.clone())).with_project(|_| project())
}

async fn run(stub: &Arc<Stub>) -> Result<CommandsResult, ParseError> {
    operator(stub)
        .brief(AgentTaskRequest::default(), &scope())
        .await
}

fn file(path: &str, start_line: u32, end_line: u32) -> BriefingCitation {
    BriefingCitation::File {
        path: path.to_owned(),
        start_line,
        end_line,
    }
}

fn detected(label: &str, command: &str, kind: CommandKind, candidate: u32) -> ExtractedCommand {
    ExtractedCommand {
        label: label.to_owned(),
        command: command.to_owned(),
        working_dir: None,
        kind,
        candidate: Some(candidate),
        citations: Vec::new(),
    }
}

fn documented(label: &str, command: &str, citations: Vec<u32>) -> ExtractedCommand {
    ExtractedCommand {
        label: label.to_owned(),
        command: command.to_owned(),
        working_dir: None,
        kind: CommandKind::Run,
        candidate: None,
        citations,
    }
}

fn extraction(
    citations: Vec<BriefingCitation>,
    commands: Vec<ExtractedCommand>,
) -> CommandsExtraction {
    CommandsExtraction {
        summary: "Run it with cargo.".to_owned(),
        citations,
        commands,
    }
}

fn read_readme() -> Calls {
    vec![(READ_FILE, json!({"path": "README.md"}))]
}

/// Run and expect the attempts to be spent on invalid output.
async fn rejected(commands: Vec<ExtractedCommand>, citations: Vec<BriefingCitation>) -> String {
    let stub = Stub::always(read_readme(), extraction(citations, commands));
    let error = run(&stub).await.unwrap_err();
    let ParseError::InvalidOutput { detail, attempts } = error else {
        panic!("expected invalid output, got {error:?}");
    };
    assert_eq!(attempts, 3);
    assert_eq!(stub.attempts(), 3);
    detail
}

#[tokio::test]
async fn candidate_backed_and_documented_commands_become_records() {
    let stub = Stub::always(
        read_readme(),
        extraction(
            vec![file("README.md", 5, 5)],
            vec![
                detected("build", "cargo build --workspace", CommandKind::Build, 1),
                detected("run app", "cargo run -p app", CommandKind::Run, 5),
                ExtractedCommand {
                    working_dir: Some("web".to_owned()),
                    ..detected("web dev", "pnpm dev", CommandKind::Dev, 7)
                },
                ExtractedCommand {
                    working_dir: Some("./web/".to_owned()),
                    command: "make site".to_owned(),
                    ..documented("site", "make site", vec![1])
                },
            ],
        ),
    );
    // Check the numbering the model sees.
    let probe = Stub::always(read_readme(), extraction(vec![], vec![]));
    run(&probe).await.unwrap();
    let prompt = probe.prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains("## Command candidates"));
    assert!(prompt.contains("5. `cargo run -p app` in `.` (run, rust)"));
    let web = prompt
        .lines()
        .find(|l| l.contains("`pnpm dev`"))
        .expect("pnpm dev candidate");
    assert!(web.contains("in `web` (dev, typescript)"), "{web}");
    let number: u32 = web.split('.').next().unwrap().parse().unwrap();
    assert_eq!(number, 7, "{prompt}");

    let result = run(&stub).await.unwrap();
    assert_eq!(result.briefing.task, AgentTask::Default);
    assert_eq!(result.briefing.run.agent, "operator");
    assert_eq!(
        result.briefing.run.prompt_version.as_deref(),
        Some("operator-v3")
    );
    assert_eq!(result.briefing.run.status, RunStatus::Done);
    assert!(result.briefing.profile.is_some());
    assert_eq!(result.commands.len(), 4);

    let build = &result.commands[0];
    assert_eq!(build.origin, CommandOrigin::Detected);
    assert_eq!(build.language, Some(LanguageId::Rust));
    assert_eq!(build.working_dir, "");
    assert_eq!(build.kind, CommandKind::Build);
    assert!(build.id.starts_with("cmd-"));

    // The model cited README line 5 as [1]; detected evidence follows it.
    let citations = &result.briefing.citations;
    assert_eq!(
        citations[0].location,
        CitationLocation::File {
            path: "README.md".to_owned(),
            start_line: 5,
            end_line: 5
        }
    );
    // Cargo.toml [workspace] (shared by build), crates/app/Cargo.toml name,
    // web/package.json "dev" line.
    assert_eq!(citations.len(), 4, "{citations:#?}");
    assert_eq!(build.citation_indexes, [2]);
    let run_app = &result.commands[1];
    assert_eq!(run_app.citation_indexes, [3]);
    let web_dev = &result.commands[2];
    assert_eq!(web_dev.working_dir, "web");
    assert_eq!(web_dev.language, Some(LanguageId::TypeScript));
    assert_eq!(web_dev.citation_indexes, [4]);

    let site = &result.commands[3];
    assert_eq!(site.origin, CommandOrigin::Documented);
    assert_eq!(site.language, None);
    assert_eq!(site.working_dir, "web");
    assert_eq!(site.citation_indexes, [1]);

    // Every citation index resolves to a real file context on the summary
    // node, in order, with a content hash and a whole-line range.
    for command in &result.commands {
        assert!(!command.citation_indexes.is_empty());
        for index in &command.citation_indexes {
            let info = &citations[*index as usize - 1];
            assert_eq!(info.index, *index);
            let context = result
                .briefing
                .node_contexts
                .iter()
                .find(|c| c.id == info.context_id)
                .expect("context exists");
            assert_eq!(context.node_id, result.briefing.summary_node_id);
            assert_eq!(context.seq, *index);
            let ContextTarget::File { content_hash, .. } = &context.target else {
                panic!("expected a file context");
            };
            assert_eq!(content_hash.as_ref().map(String::len), Some(64));
            assert!(context.range.is_some());
        }
    }
    let ids: std::collections::BTreeSet<&str> =
        result.commands.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), 4);
    let tools: Vec<&str> = result
        .briefing
        .tool_calls
        .iter()
        .map(|c| c.tool.as_str())
        .collect();
    assert_eq!(tools.last(), Some(&"submit"));
    assert!(tools.contains(&"list_files"));
    assert!(tools.contains(&"read_file"));
}

#[tokio::test]
async fn a_candidate_cited_by_the_model_shares_one_citation() {
    // The model cites the very lines the candidate came from.
    let stub = Stub::always(
        vec![(READ_FILE, json!({"path": "Cargo.toml"}))],
        extraction(
            vec![file("Cargo.toml", 1, 1)],
            vec![ExtractedCommand {
                citations: vec![1],
                ..detected("build", "cargo build --workspace", CommandKind::Build, 1)
            }],
        ),
    );
    let result = run(&stub).await.unwrap();
    assert_eq!(result.briefing.citations.len(), 1);
    assert_eq!(result.commands[0].citation_indexes, [1]);
}

#[tokio::test]
async fn a_user_prompt_becomes_a_root_prompt_node() {
    let stub = Stub::always(
        vec![],
        extraction(
            vec![],
            vec![detected(
                "test",
                "cargo test --workspace",
                CommandKind::Test,
                2,
            )],
        ),
    );
    let request = AgentTaskRequest {
        prompt: Some("how do I run the tests?".to_owned()),
        author: Some("ada".to_owned()),
        ..Default::default()
    };
    let result = operator(&stub).brief(request, &scope()).await.unwrap();
    assert_eq!(result.briefing.task, AgentTask::Prompt);
    assert_eq!(result.briefing.nodes[0].author, "ada");
    assert_eq!(
        result.briefing.nodes[1].parent_id.as_deref(),
        Some(result.briefing.nodes[0].id.as_str())
    );
    let prompt = stub.prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains("<user_prompt>\nhow do I run the tests?\n</user_prompt>"));
}

#[tokio::test]
async fn an_unbacked_command_is_rejected() {
    // No candidate and no citation.
    let detail = rejected(vec![documented("site", "make site", vec![])], vec![]).await;
    assert!(
        detail.contains("not a candidate and cites no file"),
        "{detail}"
    );
    // A citation index that does not exist.
    let detail = rejected(
        vec![documented("site", "make site", vec![2])],
        vec![file("README.md", 5, 5)],
    )
    .await;
    assert!(detail.contains("refers to citation 2"), "{detail}");
    // Citing lines that were never read.
    let stub = Stub::always(
        vec![],
        extraction(
            vec![file("unread.txt", 1, 1)],
            vec![documented("site", "make site", vec![1])],
        ),
    );
    assert!(matches!(
        run(&stub).await.unwrap_err(),
        ParseError::InvalidOutput { .. }
    ));
}

#[tokio::test]
async fn a_candidate_must_exist_and_be_used_as_detected() {
    let detail = rejected(
        vec![detected("x", "cargo build", CommandKind::Build, 99)],
        vec![],
    )
    .await;
    assert!(detail.contains("names candidate 99"), "{detail}");
    let detail = rejected(
        vec![detected("x", "cargo build", CommandKind::Build, 0)],
        vec![],
    )
    .await;
    assert!(detail.contains("names candidate 0"), "{detail}");
    // Candidate 1 is `cargo build --workspace`: editing it is not allowed.
    let detail = rejected(
        vec![detected(
            "x",
            "cargo build --release",
            CommandKind::Build,
            1,
        )],
        vec![],
    )
    .await;
    assert!(detail.contains("changes its command"), "{detail}");
    let detail = rejected(
        vec![ExtractedCommand {
            working_dir: Some("web".to_owned()),
            ..detected("x", "cargo build --workspace", CommandKind::Build, 1)
        }],
        vec![],
    )
    .await;
    assert!(detail.contains("changes its command"), "{detail}");
}

#[tokio::test]
async fn a_bad_working_directory_is_rejected() {
    for dir in [
        "nope",
        "../outside",
        "/etc",
        "web/../../x",
        "README.md",
        ".hidden",
    ] {
        let detail = rejected(
            vec![ExtractedCommand {
                working_dir: Some(dir.to_owned()),
                ..documented("site", "make site", vec![1])
            }],
            vec![file("README.md", 5, 5)],
        )
        .await;
        assert!(detail.contains("working_dir"), "{dir}: {detail}");
    }
}

#[tokio::test]
async fn duplicates_long_labels_and_the_cap_are_rejected() {
    // Same (working_dir, command), even through ./ spelling.
    let detail = rejected(
        vec![
            detected("a", "cargo build --workspace", CommandKind::Build, 1),
            ExtractedCommand {
                working_dir: Some("./".to_owned()),
                label: "b".to_owned(),
                ..detected("b", "cargo build --workspace", CommandKind::Build, 1)
            },
        ],
        vec![],
    )
    .await;
    assert!(detail.contains("repeats"), "{detail}");

    // Labels: 25 characters, empty, and a repeat (case-insensitive).
    for label in ["x".repeat(25), "  ".to_owned()] {
        let detail = rejected(
            vec![detected(
                &label,
                "cargo build --workspace",
                CommandKind::Build,
                1,
            )],
            vec![],
        )
        .await;
        assert!(detail.contains("label"), "{detail}");
    }
    let detail = rejected(
        vec![
            detected("Test", "cargo build --workspace", CommandKind::Build, 1),
            detected("test", "cargo test --workspace", CommandKind::Test, 2),
        ],
        vec![],
    )
    .await;
    assert!(detail.contains("repeats the label"), "{detail}");

    // A multi-line command.
    let detail = rejected(
        vec![ExtractedCommand {
            command: "cargo build\nrm -rf /".to_owned(),
            ..documented("x", "", vec![1])
        }],
        vec![file("README.md", 5, 5)],
    )
    .await;
    assert!(detail.contains("single line"), "{detail}");

    // Over the cap (default 12): 13 commands.
    let many: Vec<ExtractedCommand> = (0..13)
        .map(|i| ExtractedCommand {
            label: format!("c{i}"),
            command: format!("make t{i}"),
            ..documented("", "", vec![1])
        })
        .collect();
    let detail = rejected(many, vec![file("README.md", 5, 5)]).await;
    assert!(detail.contains("exceed the limit of 12"), "{detail}");
}

#[tokio::test]
async fn the_cap_is_configurable() {
    let two = vec![
        detected("build", "cargo build --workspace", CommandKind::Build, 1),
        detected("test", "cargo test --workspace", CommandKind::Test, 2),
    ];
    let stub = Stub::always(vec![], extraction(vec![], two));
    let config = OperatorConfig::default().with_max_commands(1);
    let error = operator_with(&stub, config)
        .brief(AgentTaskRequest::default(), &scope())
        .await
        .unwrap_err();
    assert!(matches!(error, ParseError::InvalidOutput { .. }));
    let result = operator_with(&stub, OperatorConfig::default().with_max_commands(2))
        .brief(AgentTaskRequest::default(), &scope())
        .await
        .unwrap();
    assert_eq!(result.commands.len(), 2);
}

#[tokio::test]
async fn markers_and_commit_citations_follow_the_briefing_rules() {
    for text in ["See [2].", "See [0].", "  "] {
        let stub = Stub::always(
            read_readme(),
            CommandsExtraction {
                summary: text.to_owned(),
                citations: vec![file("README.md", 5, 5)],
                commands: vec![],
            },
        );
        assert!(
            matches!(
                run(&stub).await.unwrap_err(),
                ParseError::InvalidOutput { .. }
            ),
            "{text}"
        );
    }
    let stub = Stub::always(
        read_readme(),
        extraction(
            vec![BriefingCitation::Commit {
                hash: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            }],
            vec![],
        ),
    );
    let ParseError::InvalidOutput { detail, .. } = run(&stub).await.unwrap_err() else {
        panic!("expected invalid output");
    };
    assert!(detail.contains("may cite files only"), "{detail}");
}

#[tokio::test]
async fn an_empty_command_list_is_valid() {
    let stub = Stub::always(vec![], extraction(vec![], vec![]));
    let result = run(&stub).await.unwrap();
    assert!(result.commands.is_empty());
}

#[tokio::test]
async fn invalid_output_is_retried_then_succeeds() {
    let stub = Stub::new(|attempt, _| {
        let commands = if attempt == 1 {
            vec![documented("site", "make site", vec![])]
        } else {
            vec![detected(
                "test",
                "cargo test --workspace",
                CommandKind::Test,
                2,
            )]
        };
        Step::Submit(vec![], extraction(vec![], commands))
    });
    let result = run(&stub).await.unwrap();
    assert_eq!(result.briefing.run.attempts, 2);
    assert_eq!(result.commands.len(), 1);
    assert_eq!(stub.attempts(), 2);

    // A schema failure is retried too; a provider error is not.
    let stub = Stub::new(|attempt, _| {
        if attempt == 1 {
            Step::Fail(ModelError::Invalid("bad schema".to_owned()))
        } else {
            Step::Submit(vec![], extraction(vec![], vec![]))
        }
    });
    assert_eq!(run(&stub).await.unwrap().briefing.run.attempts, 2);
    let stub = Stub::new(|_, _| Step::Fail(ModelError::Provider("down".to_owned())));
    assert!(matches!(
        run(&stub).await.unwrap_err(),
        ParseError::Provider { .. }
    ));
    assert_eq!(stub.attempts(), 1);
}

#[tokio::test]
async fn oversized_prompts_are_rejected_before_any_model_call() {
    let stub = Stub::always(vec![], extraction(vec![], vec![]));
    let request = AgentTaskRequest {
        prompt: Some("x".repeat(5000)),
        author: None,
        ..Default::default()
    };
    let error = operator(&stub).brief(request, &scope()).await.unwrap_err();
    assert!(matches!(error, ParseError::OversizedInput { .. }));
    assert_eq!(stub.attempts(), 0);
}

#[tokio::test]
async fn info_reports_the_configuration() {
    let stub = Stub::always(vec![], extraction(vec![], vec![]));
    let info = operator(&stub).info();
    assert_eq!(info.kind, "operator");
    assert_eq!(info.prompt_version, "operator-v3");
    assert_eq!(info.provider.as_deref(), Some("neuralwatt"));
}

#[tokio::test]
async fn preloaded_readme_supports_structured_documented_commands_without_model_reads() {
    let stub = Stub::new(|_, input| {
        assert!(input.prompt.contains("Start the site with `make site`."));
        Step::Submit(
            vec![],
            extraction(
                vec![file("README.md", 5, 5)],
                vec![documented("site", "make site", vec![1])],
            ),
        )
    });
    let result = run(&stub).await.unwrap();
    assert_eq!(result.commands[0].command, "make site");
    assert_eq!(result.commands[0].origin, CommandOrigin::Documented);
    assert_eq!(result.commands[0].citation_indexes, [1]);
    assert_eq!(result.briefing.tool_calls.last().unwrap().tool, "submit");
}

#[tokio::test]
async fn preloaded_manifests_support_documented_commands_without_model_reads() {
    let stub = Stub::new(|_, input| {
        assert!(
            input
                .toolset
                .files()
                .recorder()
                .covers("web/package.json", 1, 1)
        );
        Step::Submit(
            vec![],
            extraction(
                vec![file("web/package.json", 1, 1)],
                vec![documented("web", "pnpm dev", vec![1])],
            ),
        )
    });
    let result = run(&stub).await.unwrap();
    assert_eq!(result.commands[0].command, "pnpm dev");
    assert_eq!(
        result
            .briefing
            .tool_calls
            .iter()
            .filter(|c| c.tool == READ_FILE && c.input.contains("web/package.json"))
            .count(),
        1
    );
}
