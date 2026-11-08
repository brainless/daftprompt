//! Stored agent responses (Epic 007, Task B): stub models run through the
//! briefing service into a temporary SQLite database. No credentials, network,
//! or git binary.

use std::path::Path;
use std::sync::Arc;

use daftprompt_core::chronicler::{ChroniclerInput, ChroniclerModel};
use daftprompt_core::git_tools::{GitHistory, InMemoryGit, MemoryCommit};
use daftprompt_core::operator::{OperatorInput, OperatorModel};
use daftprompt_core::project_tools::{InMemoryProject, ProjectFiles};
use daftprompt_core::store::{
    NewResponse, ResponseOutcome, ResponseQuery, StoredAgent, StoredAgentResponseV1, StoredResult,
};
use daftprompt_core::surveyor::{SurveyorInput, SurveyorModel};
use daftprompt_core::{
    Briefer, BriefingAgent, BriefingError, BriefingService, Chronicler, ChroniclerConfig,
    CommandsBriefer, ModelError, Operator, OperatorConfig, ProjectScope, ScopeOrigin, Store,
    Surveyor, SurveyorConfig,
};
use daftprompt_types::{
    AgentTaskRequest, BriefingCitation, BriefingExtraction, CommandKind, CommandsExtraction,
    ExtractedCommand, Project,
};
use serde_json::json;

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
const MARKDOWN: &str = "# Recent work\n\n- **Added** the `importer` [1]\n- 日本語 and emoji ✓\n";

struct ChroniclerStub;

impl ChroniclerModel for ChroniclerStub {
    fn summarize(
        &self,
        _prompt: String,
    ) -> daftprompt_core::chronicler::BoxFuture<'_, Result<String, ModelError>> {
        Box::pin(async { Ok(MARKDOWN.to_owned()) })
    }

    fn brief(
        &self,
        _input: ChroniclerInput,
    ) -> daftprompt_core::chronicler::BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        Box::pin(async { Err(ModelError::Provider("unused".to_owned())) })
    }
}

/// Reads the README and cites its first line, or fails when `fail` is set.
struct SurveyorStub {
    fail: bool,
}

impl SurveyorModel for SurveyorStub {
    fn brief(
        &self,
        input: SurveyorInput,
    ) -> daftprompt_core::surveyor::BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        let fail = self.fail;
        Box::pin(async move {
            if fail {
                return Err(ModelError::Provider("down".to_owned()));
            }
            input
                .toolset
                .files()
                .call("read_file", json!({"path": "README.md"}));
            Ok(BriefingExtraction {
                summary: MARKDOWN.to_owned(),
                citations: vec![BriefingCitation::File {
                    path: "README.md".to_owned(),
                    start_line: 1,
                    end_line: 1,
                }],
            })
        })
    }
}

/// Offers the first candidate and a documented command in `web/`.
struct OperatorStub;

impl OperatorModel for OperatorStub {
    fn brief(
        &self,
        input: OperatorInput,
    ) -> daftprompt_core::operator::BoxFuture<'_, Result<CommandsExtraction, ModelError>> {
        Box::pin(async move {
            input
                .toolset
                .files()
                .call("read_file", json!({"path": "README.md"}));
            Ok(CommandsExtraction {
                summary: MARKDOWN.to_owned(),
                citations: vec![BriefingCitation::File {
                    path: "README.md".to_owned(),
                    start_line: 1,
                    end_line: 1,
                }],
                commands: vec![
                    ExtractedCommand {
                        label: "build".to_owned(),
                        command: "cargo build".to_owned(),
                        working_dir: None,
                        kind: CommandKind::Build,
                        candidate: Some(1),
                        citations: vec![],
                    },
                    ExtractedCommand {
                        label: "site".to_owned(),
                        command: "make site".to_owned(),
                        working_dir: Some("web".to_owned()),
                        kind: CommandKind::Run,
                        candidate: None,
                        citations: vec![1],
                    },
                ],
            })
        })
    }
}

fn project_files() -> Arc<dyn ProjectFiles> {
    Arc::new(
        InMemoryProject::new()
            .with_file("README.md", "# Demo\n\nStart with `make site`.\n")
            .with_file("Cargo.toml", "[package]\nname = \"demo\"\n")
            .with_file("web/package.json", "{\"name\":\"web\"}"),
    )
}

fn chronicler() -> Chronicler<ChroniclerStub> {
    let history: Arc<dyn GitHistory> = Arc::new(InMemoryGit::new(vec![MemoryCommit::new(
        HASH,
        "Add the importer",
    )]));
    Chronicler::new(ChroniclerConfig::default(), ChroniclerStub)
        .with_history(move |_| history.clone())
}

fn surveyor(fail: bool) -> Surveyor<SurveyorStub> {
    Surveyor::new(SurveyorConfig::default(), SurveyorStub { fail })
        .with_project(|_| project_files())
}

fn operator() -> Operator<OperatorStub> {
    Operator::new(OperatorConfig::default(), OperatorStub).with_project(|_| project_files())
}

struct Fixture {
    store: Arc<Store>,
    service: BriefingService,
    project: Project,
    scope: ProjectScope,
    _folder: tempfile::TempDir,
}

fn fixture(db: &Path) -> Fixture {
    let store = Arc::new(Store::open(db).unwrap());
    let folder = tempfile::tempdir().unwrap();
    let (project, _) = store.add_project(folder.path(), Some("demo")).unwrap();
    let scope = ProjectScope {
        project_id: project.id.clone(),
        project_path_id: project.default_path_id.clone(),
        root: folder.path().to_path_buf(),
    };
    Fixture {
        service: BriefingService::new(Some(store.clone())),
        store,
        project,
        scope,
        _folder: folder,
    }
}

fn query<'a>(
    fx: &'a Fixture,
    agent: StoredAgent,
    request_key: &'a str,
    config_key: &'a str,
) -> ResponseQuery<'a> {
    ResponseQuery {
        project_id: &fx.project.id,
        project_path_id: &fx.project.default_path_id,
        agent,
        request_key,
        config_key,
    }
}

fn custom(prompt: &str) -> AgentTaskRequest {
    AgentTaskRequest {
        prompt: Some(prompt.to_owned()),
        author: Some("ada".to_owned()),
        project_path_id: None,
        refresh: false,
    }
}

fn usable(outcome: &ResponseOutcome) -> &daftprompt_core::store::StoredResponse {
    match outcome {
        ResponseOutcome::Usable(stored) => stored,
        ResponseOutcome::Unusable(row) => panic!("unusable row: {}", row.reason),
    }
}

#[tokio::test]
async fn every_agent_round_trips_through_the_store_and_a_reopened_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("daftprompt.db");
    let fx = fixture(&db);
    let request = AgentTaskRequest::default();
    let request_key = daftprompt_core::project_agent::request_key(&request);

    let (chronicler, surveyor, operator) = (chronicler(), surveyor(false), operator());
    let history = fx
        .service
        .brief(
            &chronicler,
            BriefingAgent::Chronicler,
            request.clone(),
            &fx.scope,
            ScopeOrigin::Registered,
        )
        .await
        .unwrap()
        .result;
    let overview = fx
        .service
        .brief(
            &surveyor,
            BriefingAgent::Surveyor,
            request.clone(),
            &fx.scope,
            ScopeOrigin::Registered,
        )
        .await
        .unwrap()
        .result;
    let commands = fx
        .service
        .commands(
            &operator,
            request.clone(),
            &fx.scope,
            ScopeOrigin::Registered,
        )
        .await
        .unwrap()
        .result;

    // Sanity: the fixtures exercise the interesting fields.
    assert_eq!(history.summary_payload.content, MARKDOWN);
    assert!(!history.tool_calls.is_empty() && !history.tool_payloads.is_empty());
    assert!(overview.profile.is_some() && !overview.citations.is_empty());
    assert_eq!(commands.commands.len(), 2);
    assert_eq!(commands.commands[1].working_dir, "web");
    assert!(!commands.commands[0].citation_indexes.is_empty());

    let config_keys = [
        Briefer::cache_identity(&chronicler),
        Briefer::cache_identity(&surveyor),
        CommandsBriefer::cache_identity(&operator),
    ];
    assert_eq!(
        config_keys
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );

    drop(fx.service);
    drop(fx.store);
    let reopened = Store::open(&db).unwrap();
    let load = |agent: StoredAgent, config_key: &str| {
        let outcomes = reopened
            .latest_agent_responses(
                ResponseQuery {
                    project_id: &fx.project.id,
                    project_path_id: &fx.project.default_path_id,
                    agent,
                    request_key: &request_key,
                    config_key,
                },
                5,
            )
            .unwrap();
        assert_eq!(outcomes.len(), 1);
        let stored = usable(&outcomes[0]).clone();
        // Loading by id returns the same row.
        assert_eq!(
            usable(&reopened.get_agent_response(&stored.meta.id).unwrap()),
            &stored
        );
        assert!(stored.meta.id.starts_with("response-"));
        // Task B leaves repository identity and eligibility for later.
        assert_eq!(stored.meta.commit_hash, None);
        assert_eq!(stored.meta.repository_subpath, None);
        assert!(!stored.meta.reusable);
        assert_eq!(stored.meta.response_version, 1);
        stored
    };

    let stored = load(StoredAgent::Chronicler, &config_keys[0]);
    assert_eq!(stored.response.summary_text, MARKDOWN);
    let StoredResult::Briefing(back) = stored.response.into_result() else {
        panic!("expected a briefing");
    };
    assert_eq!(back, history);

    let stored = load(StoredAgent::Surveyor, &config_keys[1]);
    let StoredResult::Briefing(back) = stored.response.into_result() else {
        panic!("expected a briefing");
    };
    assert_eq!(back, overview);

    let stored = load(StoredAgent::Operator, &config_keys[2]);
    let StoredResult::Commands(back) = stored.response.into_result() else {
        panic!("expected commands");
    };
    assert_eq!(back, commands);
}

#[tokio::test]
async fn failed_runs_insert_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture(&dir.path().join("db.sqlite"));
    let surveyor = surveyor(true);
    let error = fx
        .service
        .brief(
            &surveyor,
            BriefingAgent::Surveyor,
            AgentTaskRequest::default(),
            &fx.scope,
            ScopeOrigin::Registered,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, BriefingError::Agent(_)), "{error}");
    let request_key = daftprompt_core::project_agent::request_key(&AgentTaskRequest::default());
    let rows = fx
        .store
        .latest_agent_responses(
            query(
                &fx,
                StoredAgent::Surveyor,
                &request_key,
                &Briefer::cache_identity(&surveyor),
            ),
            10,
        )
        .unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn custom_prompts_are_stored_under_their_own_request_key() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture(&dir.path().join("db.sqlite"));
    let chronicler = chronicler();
    let config_key = Briefer::cache_identity(&chronicler);
    for request in [AgentTaskRequest::default(), custom("What changed?")] {
        fx.service
            .brief(
                &chronicler,
                BriefingAgent::Chronicler,
                request,
                &fx.scope,
                ScopeOrigin::Registered,
            )
            .await
            .unwrap();
    }
    let default_key = daftprompt_core::project_agent::request_key(&AgentTaskRequest::default());
    let custom_key = daftprompt_core::project_agent::request_key(&custom("What changed?"));
    assert_ne!(default_key, custom_key);
    for key in [&default_key, &custom_key] {
        let rows = fx
            .store
            .latest_agent_responses(query(&fx, StoredAgent::Chronicler, key, &config_key), 10)
            .unwrap();
        assert_eq!(rows.len(), 1);
    }
    let rows = fx
        .store
        .latest_agent_responses(
            query(&fx, StoredAgent::Chronicler, &custom_key, &config_key),
            10,
        )
        .unwrap();
    let StoredResult::Briefing(result) = usable(&rows[0]).response.clone().into_result() else {
        panic!("expected a briefing");
    };
    assert_eq!(result.prompt_payload.unwrap().content, "What changed?");
}

#[tokio::test]
async fn the_unregistered_fallback_scope_is_not_stored() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture(&dir.path().join("db.sqlite"));
    let chronicler = chronicler();
    fx.service
        .brief(
            &chronicler,
            BriefingAgent::Chronicler,
            AgentTaskRequest::default(),
            &fx.scope,
            ScopeOrigin::Fallback,
        )
        .await
        .unwrap();
    let request_key = daftprompt_core::project_agent::request_key(&AgentTaskRequest::default());
    let rows = fx
        .store
        .latest_agent_responses(
            query(
                &fx,
                StoredAgent::Chronicler,
                &request_key,
                &Briefer::cache_identity(&chronicler),
            ),
            10,
        )
        .unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn a_storage_failure_after_a_completed_run_is_an_error_with_the_run_id() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture(&dir.path().join("db.sqlite"));
    // The path disappears between authorization and the write.
    fx.store.remove_project(&fx.project.id).unwrap();
    let error = fx
        .service
        .brief(
            &chronicler(),
            BriefingAgent::Chronicler,
            AgentTaskRequest::default(),
            &fx.scope,
            ScopeOrigin::Registered,
        )
        .await
        .unwrap_err();
    let BriefingError::Storage { run_id, .. } = &error else {
        panic!("expected a storage error, got {error}");
    };
    assert!(run_id.starts_with("run-"));
    assert!(error.to_string().contains(run_id.as_str()));
}

#[test]
fn config_keys_follow_model_limits_and_commit_count() {
    let key = |config: SurveyorConfig| {
        Briefer::cache_identity(&Surveyor::new(config, SurveyorStub { fail: false }))
    };
    let base = key(SurveyorConfig::default());
    assert_eq!(base, key(SurveyorConfig::default()));
    assert_eq!(base.len(), 64);
    for changed in [
        SurveyorConfig::default().with_model("another-model"),
        SurveyorConfig::default().with_provider("groq"),
        SurveyorConfig::default().with_max_agent_turns(3),
        SurveyorConfig::default().with_max_attempts(1),
        SurveyorConfig::default().with_output_limits(5, 100),
        SurveyorConfig::default().with_max_prompt_bytes(10),
        SurveyorConfig::default().with_tool_config(
            daftprompt_core::project_tools::ProjectToolConfig {
                max_read_lines: 7,
                ..Default::default()
            },
        ),
    ] {
        assert_ne!(base, key(changed));
    }

    let chronicler_key = |config: ChroniclerConfig| {
        Briefer::cache_identity(&Chronicler::new(config, ChroniclerStub))
    };
    let mut config = ChroniclerConfig::default();
    let base = chronicler_key(config.clone());
    config.commit_count += 1;
    assert_ne!(base, chronicler_key(config));

    let operator_key = |config: OperatorConfig| {
        CommandsBriefer::cache_identity(&Operator::new(config, OperatorStub))
    };
    assert_ne!(
        operator_key(OperatorConfig::default()),
        operator_key(OperatorConfig::default().with_max_commands(3))
    );
    // The same settings under another agent are another configuration.
    assert_ne!(
        key(SurveyorConfig::default()),
        operator_key(OperatorConfig::default())
    );
}

#[tokio::test]
async fn removing_a_path_removes_its_responses() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixture(&dir.path().join("db.sqlite"));
    let second = tempfile::tempdir().unwrap();
    let (path, _) = fx.store.add_path(&fx.project.id, second.path()).unwrap();
    let chronicler = chronicler();
    let mut ids = Vec::new();
    for path_id in [&fx.project.default_path_id, &path.id] {
        let result = chronicler
            .brief(
                AgentTaskRequest::default(),
                &ProjectScope {
                    project_path_id: path_id.clone(),
                    ..fx.scope.clone()
                },
            )
            .await
            .unwrap();
        let meta = fx
            .store
            .insert_agent_response(NewResponse {
                project_id: &fx.project.id,
                project_path_id: path_id,
                request_key: "request",
                config_key: "config",
                commit_hash: None,
                repository_subpath: None,
                reusable: false,
                response: &StoredAgentResponseV1::chronicler(&result),
            })
            .unwrap();
        ids.push(meta.id);
    }
    fx.store
        .remove_path(&fx.project.id, &path.id, None)
        .unwrap();
    assert!(fx.store.get_agent_response(&ids[0]).is_ok());
    assert!(matches!(
        fx.store.get_agent_response(&ids[1]),
        Err(daftprompt_core::StoreError::NotFound(_))
    ));
}
