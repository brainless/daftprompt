//! `POST /projects/{id}/commands` and the Operator in `GET /agents`. A stub
//! Operator model over a real temporary folder; no credentials or network.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use daftprompt_api::{ApiConfig, AppState, router};
use daftprompt_core::operator::{OperatorInput, OperatorModel};
use daftprompt_core::{
    AccessError, AgentKind, BoxFuture, ExtractionModel, ModelError, ModelInput, Operator,
    OperatorConfig, Principal, ProjectScope, RootPolicy, Scribe, ScribeConfig, Store,
};
use daftprompt_types::{
    CommandKind, CommandsExtraction, ExtractedCommand, Extraction, Project, ProjectPath,
};
use serde_json::{Value, json};
use tower::ServiceExt;

struct ScribeStub;

impl ExtractionModel for ScribeStub {
    fn extract(&self, _input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        Box::pin(async { Ok(Extraction::default()) })
    }
}

/// Offers the first candidate (`cargo build`) and a documented command.
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
                citations: vec![daftprompt_types::BriefingCitation::File {
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

struct DenyAll;

impl RootPolicy for DenyAll {
    fn grant(
        &self,
        _principal: &Principal,
        agent: AgentKind,
        project: &Project,
        _path: &ProjectPath,
    ) -> Result<ProjectScope, AccessError> {
        Err(AccessError::Denied(format!(
            "{} may not use project {}",
            agent.as_str(),
            project.id
        )))
    }
}

/// Denies only the Surveyor: the policy sees the agent kind.
struct NoOperator;

impl RootPolicy for NoOperator {
    fn grant(
        &self,
        principal: &Principal,
        agent: AgentKind,
        project: &Project,
        path: &ProjectPath,
    ) -> Result<ProjectScope, AccessError> {
        if agent == AgentKind::Operator {
            return Err(AccessError::Denied("no commands for you".to_owned()));
        }
        daftprompt_core::LocalPolicy.grant(principal, agent, project, path)
    }
}

struct Fixture {
    app: Router,
    project: Project,
    folder: tempfile::TempDir,
    _db: tempfile::TempDir,
}

fn fixture(policy: Option<Arc<dyn RootPolicy>>, with_operator: bool) -> Fixture {
    let db = tempfile::tempdir().unwrap();
    let store = Store::open(db.path().join("daftprompt.db")).unwrap();
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("README.md"), "# Demo\n").unwrap();
    std::fs::write(
        folder.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    std::fs::create_dir(folder.path().join("web")).unwrap();
    std::fs::write(folder.path().join("web/package.json"), "{\"name\":\"web\"}").unwrap();
    let (project, _) = store.add_project(folder.path(), Some("demo")).unwrap();

    let mut state = AppState::new(
        Arc::new(Scribe::new(ScribeConfig::new(), ScribeStub)),
        ApiConfig::default(),
    )
    .with_store(Arc::new(store));
    if with_operator {
        state = state.with_operator(Arc::new(Operator::new(
            OperatorConfig::default(),
            OperatorStub,
        )));
    }
    if let Some(policy) = policy {
        state = state.with_policy(policy);
    }
    Fixture {
        app: router(state),
        project,
        folder,
        _db: db,
    }
}

async fn call(app: &Router, method: &str, uri: &str, body: Body) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
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

#[tokio::test]
async fn commands_with_an_empty_body_returns_commands_and_briefing_records() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/commands", fx.project.id);
    let (status, json) = call(&fx.app, "POST", &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    // The briefing fields are flattened next to `commands`.
    assert_eq!(json["project_id"], fx.project.id.as_str());
    assert_eq!(json["task"]["kind"], "default");
    assert_eq!(json["run"]["agent"], "operator");
    assert_eq!(json["run"]["prompt_version"], "operator-v3");
    assert_eq!(json["profile"]["languages"][0]["language"], "rust");

    let commands = json["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0]["label"], "build");
    assert_eq!(commands[0]["command"], "cargo build");
    assert_eq!(commands[0]["working_dir"], "");
    assert_eq!(commands[0]["kind"], "build");
    assert_eq!(commands[0]["language"], "rust");
    assert_eq!(commands[0]["origin"], "detected");
    assert!(commands[0]["id"].as_str().unwrap().starts_with("cmd-"));
    assert_eq!(commands[0]["citation_indexes"], json!([2]));
    assert_eq!(commands[1]["origin"], "documented");
    assert_eq!(commands[1]["working_dir"], "web");
    assert_eq!(commands[1]["language"], Value::Null);
    assert_eq!(commands[1]["citation_indexes"], json!([1]));

    let citations = json["citations"].as_array().unwrap();
    assert_eq!(citations[0]["location"]["path"], "README.md");
    assert_eq!(citations[1]["location"]["path"], "Cargo.toml");
}

#[tokio::test]
async fn commands_with_a_prompt_records_the_prompt_node() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/commands", fx.project.id);
    let body = json!({"prompt": "how do I build?", "author": "ada"});
    let (status, json) = call(&fx.app, "POST", &uri, Body::from(body.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["task"]["kind"], "prompt");
    assert_eq!(json["nodes"][0]["author"], "ada");
}

#[tokio::test]
async fn commands_rejects_a_malformed_or_oversized_body() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/commands", fx.project.id);
    let (status, json) = call(&fx.app, "POST", &uri, Body::from("{not json")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["kind"], "invalid_request");
    let body = json!({"prompt": "x".repeat(5000)});
    let (status, json) = call(&fx.app, "POST", &uri, Body::from(body.to_string())).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json["kind"], "oversized_input");
}

#[tokio::test]
async fn commands_for_an_unknown_project_is_not_found() {
    let fx = fixture(None, true);
    let (status, json) = call(
        &fx.app,
        "POST",
        "/projects/project-nope/commands",
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["kind"], "not_found");
}

#[tokio::test]
async fn commands_with_a_denying_policy_is_forbidden() {
    for policy in [
        Arc::new(DenyAll) as Arc<dyn RootPolicy>,
        Arc::new(NoOperator),
    ] {
        let fx = fixture(Some(policy), true);
        let uri = format!("/projects/{}/commands", fx.project.id);
        let (status, json) = call(&fx.app, "POST", &uri, Body::empty()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        assert_eq!(json["kind"], "forbidden");
    }
}

#[tokio::test]
async fn commands_for_a_removed_folder_is_unprocessable() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/commands", fx.project.id);
    drop(fx.folder);
    let (status, json) = call(&fx.app, "POST", &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    assert_eq!(json["kind"], "project_unavailable");
}

#[tokio::test]
async fn commands_without_a_configured_operator_is_an_internal_error() {
    let fx = fixture(None, false);
    let uri = format!("/projects/{}/commands", fx.project.id);
    let (status, json) = call(&fx.app, "POST", &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json["kind"], "internal_error");
}

#[tokio::test]
async fn agents_lists_the_operator_when_configured() {
    let fx = fixture(None, true);
    let (status, json) = call(&fx.app, "GET", "/agents", Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let agents = json.as_array().unwrap();
    let kinds: Vec<&str> = agents.iter().map(|a| a["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["scribe", "operator"]);
    assert_eq!(agents[1]["prompt_version"], "operator-v3");
    assert_eq!(agents[1]["provider"], "neuralwatt");
}
