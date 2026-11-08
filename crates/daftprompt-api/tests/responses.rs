//! Stored agent responses through the API (Epic 007, Task B): a successful
//! briefing is persisted for a registered project and the response body keeps
//! its shape; a failed write is an error. Stub models; no credentials.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use daftprompt_api::{ApiConfig, AppState, router};
use daftprompt_core::chronicler::{ChroniclerInput, ChroniclerModel};
use daftprompt_core::git_tools::{GitHistory, InMemoryGit, MemoryCommit};
use daftprompt_core::operator::{OperatorInput, OperatorModel};
use daftprompt_core::store::{ResponseOutcome, ResponseQuery, StoredAgent, StoredResult};
use daftprompt_core::surveyor::{SurveyorInput, SurveyorModel};
use daftprompt_core::{
    BoxFuture, Briefer, Chronicler, ChroniclerConfig, CommandsBriefer, ExtractionModel, ModelError,
    ModelInput, Operator, OperatorConfig, Scribe, ScribeConfig, Store, Surveyor, SurveyorConfig,
};
use daftprompt_types::{
    AgentTaskRequest, BriefingCitation, BriefingExtraction, CommandKind, CommandsExtraction,
    ExtractedCommand, Extraction, Project,
};
use serde_json::{Value, json};
use tower::ServiceExt;

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

struct ScribeStub;

impl ExtractionModel for ScribeStub {
    fn extract(&self, _input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        Box::pin(async { Ok(Extraction::default()) })
    }
}

/// Summarizes, first removing the project when a store is armed: the path is
/// gone by the time the finished run is written.
type Sabotage = Arc<Mutex<Option<(Arc<Store>, String)>>>;

struct ChroniclerStub {
    sabotage: Sabotage,
}

impl ChroniclerModel for ChroniclerStub {
    fn summarize(
        &self,
        _prompt: String,
    ) -> daftprompt_core::chronicler::BoxFuture<'_, Result<String, ModelError>> {
        if let Some((store, project_id)) = self.sabotage.lock().unwrap().take() {
            store.remove_project(&project_id).unwrap();
        }
        Box::pin(async { Ok("Recent work: **importer**.".to_owned()) })
    }

    fn brief(
        &self,
        _input: ChroniclerInput,
    ) -> daftprompt_core::chronicler::BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        Box::pin(async { Err(ModelError::Provider("unused".to_owned())) })
    }
}

struct SurveyorStub;

impl SurveyorModel for SurveyorStub {
    fn brief(
        &self,
        input: SurveyorInput,
    ) -> daftprompt_core::surveyor::BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        Box::pin(async move {
            input
                .toolset
                .files()
                .call("read_file", json!({"path": "README.md"}));
            Ok(BriefingExtraction {
                summary: "A demo [1].".to_owned(),
                citations: vec![BriefingCitation::File {
                    path: "README.md".to_owned(),
                    start_line: 1,
                    end_line: 1,
                }],
            })
        })
    }
}

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
                summary: "A demo [1].".to_owned(),
                citations: vec![BriefingCitation::File {
                    path: "README.md".to_owned(),
                    start_line: 1,
                    end_line: 1,
                }],
                commands: vec![ExtractedCommand {
                    label: "build".to_owned(),
                    command: "cargo build".to_owned(),
                    working_dir: None,
                    kind: CommandKind::Build,
                    candidate: Some(1),
                    citations: vec![],
                }],
            })
        })
    }
}

struct Fixture {
    app: Router,
    store: Arc<Store>,
    project: Project,
    sabotage: Sabotage,
    chronicler: Arc<Chronicler<ChroniclerStub>>,
    surveyor: Arc<Surveyor<SurveyorStub>>,
    operator: Arc<Operator<OperatorStub>>,
    _folder: tempfile::TempDir,
    _db: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let db = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(db.path().join("daftprompt.db")).unwrap());
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("README.md"), "# Demo\n").unwrap();
    std::fs::write(
        folder.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    let (project, _) = store.add_project(folder.path(), Some("demo")).unwrap();

    let history: Arc<dyn GitHistory> = Arc::new(InMemoryGit::new(vec![MemoryCommit::new(
        HASH,
        "Add the importer",
    )]));
    let sabotage = Sabotage::default();
    let chronicler = Arc::new(
        Chronicler::new(
            ChroniclerConfig::default(),
            ChroniclerStub {
                sabotage: sabotage.clone(),
            },
        )
        .with_history(move |_| history.clone()),
    );
    let surveyor = Arc::new(Surveyor::new(SurveyorConfig::default(), SurveyorStub));
    let operator = Arc::new(Operator::new(OperatorConfig::default(), OperatorStub));
    let state = AppState::new(
        Arc::new(Scribe::new(ScribeConfig::new(), ScribeStub)),
        ApiConfig::default(),
    )
    .with_store(store.clone())
    .with_chronicler(chronicler.clone())
    .with_surveyor(surveyor.clone())
    .with_operator(operator.clone());
    Fixture {
        app: router(state),
        store,
        project,
        sabotage,
        chronicler,
        surveyor,
        operator,
        _folder: folder,
        _db: db,
    }
}

async fn post(app: &Router, uri: &str, body: Body) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(body)
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn rows(fx: &Fixture, agent: StoredAgent, config_key: &str) -> Vec<ResponseOutcome> {
    let request_key = daftprompt_core::project_agent::request_key(&AgentTaskRequest::default());
    fx.store
        .latest_agent_responses(
            ResponseQuery {
                project_id: &fx.project.id,
                project_path_id: &fx.project.default_path_id,
                agent,
                request_key: &request_key,
                config_key,
            },
            10,
        )
        .unwrap()
}

fn stored(outcome: &ResponseOutcome) -> StoredResult {
    match outcome {
        ResponseOutcome::Usable(stored) => stored.response.clone().into_result(),
        ResponseOutcome::Unusable(row) => panic!("unusable: {}", row.reason),
    }
}

/// Split a response body into the stored result's fields and the delivery
/// metadata (`project_path_id`, `cache`) checked against the stored row.
fn split_delivery(fx: &Fixture, mut body: Value, response_id: &str) -> Value {
    let object = body.as_object_mut().unwrap();
    assert_eq!(
        object.remove("project_path_id").unwrap(),
        json!(fx.project.default_path_id)
    );
    let cache = object.remove("cache").unwrap();
    assert_eq!(cache["kind"], "fresh");
    assert_eq!(cache["response_id"], response_id);
    assert_eq!(cache["requested_path_id"], cache["source_path_id"]);
    assert_eq!(cache["source_path_id"], json!(fx.project.default_path_id));
    body
}

#[tokio::test]
async fn each_briefing_route_stores_its_result_and_keeps_its_body_shape() {
    let fx = fixture();
    let id = &fx.project.id;

    let (status, history) = post(&fx.app, &format!("/projects/{id}/history"), Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let stored_rows = rows(
        &fx,
        StoredAgent::Chronicler,
        &Briefer::cache_identity(&*fx.chronicler),
    );
    assert_eq!(stored_rows.len(), 1);
    let StoredResult::Briefing(result) = stored(&stored_rows[0]) else {
        panic!("expected a briefing");
    };
    // Every stored field stays top level; delivery adds only two more.
    let history = split_delivery(&fx, history, &stored_rows[0].meta().id);
    assert_eq!(serde_json::to_value(&result).unwrap(), history);

    let (status, overview) =
        post(&fx.app, &format!("/projects/{id}/overview"), Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    let stored_rows = rows(
        &fx,
        StoredAgent::Surveyor,
        &Briefer::cache_identity(&*fx.surveyor),
    );
    assert_eq!(stored_rows.len(), 1);
    let StoredResult::Briefing(result) = stored(&stored_rows[0]) else {
        panic!("expected a briefing");
    };
    let overview = split_delivery(&fx, overview, &stored_rows[0].meta().id);
    assert_eq!(serde_json::to_value(&result).unwrap(), overview);

    let (status, commands) =
        post(&fx.app, &format!("/projects/{id}/commands"), Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{commands}");
    let stored_rows = rows(
        &fx,
        StoredAgent::Operator,
        &CommandsBriefer::cache_identity(&*fx.operator),
    );
    assert_eq!(stored_rows.len(), 1);
    let StoredResult::Commands(result) = stored(&stored_rows[0]) else {
        panic!("expected commands");
    };
    let commands = split_delivery(&fx, commands, &stored_rows[0].meta().id);
    assert_eq!(serde_json::to_value(&result).unwrap(), commands);
    assert_eq!(commands["commands"][0]["command"], "cargo build");
}

#[tokio::test]
async fn a_failed_request_stores_nothing_and_a_prompt_is_stored_separately() {
    let fx = fixture();
    let uri = format!("/projects/{}/history", fx.project.id);
    let (status, _) = post(&fx.app, &uri, Body::from("{not json")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let config_key = Briefer::cache_identity(&*fx.chronicler);
    assert!(rows(&fx, StoredAgent::Chronicler, &config_key).is_empty());

    let (status, _) = post(&fx.app, &uri, Body::from(r#"{"prompt":"What changed?"}"#)).await;
    assert_eq!(status, StatusCode::OK);
    // The default-task key finds nothing; the custom prompt has its own key.
    assert!(rows(&fx, StoredAgent::Chronicler, &config_key).is_empty());
    let custom = AgentTaskRequest {
        prompt: Some("What changed?".to_owned()),
        ..Default::default()
    };
    let key = daftprompt_core::project_agent::request_key(&custom);
    let found = fx
        .store
        .latest_agent_responses(
            ResponseQuery {
                project_id: &fx.project.id,
                project_path_id: &fx.project.default_path_id,
                agent: StoredAgent::Chronicler,
                request_key: &key,
                config_key: &config_key,
            },
            10,
        )
        .unwrap();
    assert_eq!(found.len(), 1);
}

#[tokio::test]
async fn a_failed_write_is_an_internal_error_not_a_success() {
    let fx = fixture();
    *fx.sabotage.lock().unwrap() = Some((fx.store.clone(), fx.project.id.clone()));
    let (status, body) = post(
        &fx.app,
        &format!("/projects/{}/history", fx.project.id),
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["kind"], "internal_error");
    assert!(body["detail"].as_str().unwrap().contains("run-"));
    assert!(body.get("summary_payload").is_none());
}
