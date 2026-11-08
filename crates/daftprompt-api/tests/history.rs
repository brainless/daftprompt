//! `GET /agents` and `POST /projects/{id}/history`. A stub Chronicler model
//! over an in-memory git history; no credentials, network, or git binary.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use daftprompt_api::{ApiConfig, AppState, router};
use daftprompt_core::chronicler::{ChroniclerInput, ChroniclerModel};
use daftprompt_core::explainer::{ExplainerInput, ExplainerModel};
use daftprompt_core::git_tools::{GitHistory, InMemoryGit, MemoryCommit};
use daftprompt_core::{
    AccessError, AgentKind, BoxFuture, Chronicler, ChroniclerConfig, Explainer, ExplainerConfig,
    ExtractionModel, ModelError, ModelInput, Principal, ProjectScope, RootPolicy, Scribe,
    ScribeConfig, Store,
};
use daftprompt_types::{
    BriefingCitation, BriefingExtraction, ExplainerExtraction, Extraction, Project, ProjectPath,
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

/// Reads the log, then cites its first commit.
struct ChroniclerStub;

impl ChroniclerModel for ChroniclerStub {
    fn summarize(
        &self,
        prompt: String,
    ) -> daftprompt_core::chronicler::BoxFuture<'_, Result<String, ModelError>> {
        Box::pin(async move {
            assert!(prompt.contains("Add the importer"));
            Ok("Recent work: added the importer.".to_owned())
        })
    }

    fn brief(
        &self,
        input: ChroniclerInput,
    ) -> daftprompt_core::chronicler::BoxFuture<'_, Result<BriefingExtraction, ModelError>> {
        Box::pin(async move {
            input.toolset.git().call("git_log", json!({}));
            Ok(BriefingExtraction {
                summary: format!("Recent work: {} [1]", input.prompt.len()),
                citations: vec![BriefingCitation::Commit {
                    hash: HASH[..10].to_owned(),
                }],
            })
        })
    }
}

/// Never called here; `GET /agents` only needs it to exist.
struct ExplainerStub;

impl ExplainerModel for ExplainerStub {
    fn explain(
        &self,
        _input: ExplainerInput,
    ) -> daftprompt_core::explainer::BoxFuture<'_, Result<ExplainerExtraction, ModelError>> {
        Box::pin(async {
            Ok(ExplainerExtraction {
                explanation: "Unused.".to_owned(),
                citations: Vec::new(),
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

/// Denies only the Chronicler: the policy sees the agent kind.
struct NoChronicler;

impl RootPolicy for NoChronicler {
    fn grant(
        &self,
        principal: &Principal,
        agent: AgentKind,
        project: &Project,
        path: &ProjectPath,
    ) -> Result<ProjectScope, AccessError> {
        if agent == AgentKind::Chronicler {
            return Err(AccessError::Denied("no history for you".to_owned()));
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

fn fixture(policy: Option<Arc<dyn RootPolicy>>, with_chronicler: bool) -> Fixture {
    let db = tempfile::tempdir().unwrap();
    let store = Store::open(db.path().join("daftprompt.db")).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let (project, _) = store.add_project(folder.path(), Some("repo")).unwrap();

    let scribe = Scribe::new(ScribeConfig::new(), ScribeStub);
    let history: Arc<dyn GitHistory> = Arc::new(InMemoryGit::new(vec![MemoryCommit::new(
        HASH,
        "Add the importer",
    )]));
    let chronicler = Chronicler::new(ChroniclerConfig::default(), ChroniclerStub)
        .with_history(move |_| history.clone());
    let mut state = AppState::new(Arc::new(scribe), ApiConfig::default())
        .with_clarifier(Arc::new(Explainer::new(
            ExplainerConfig::default(),
            ExplainerStub,
        )))
        .with_store(Arc::new(store));
    if with_chronicler {
        state = state.with_chronicler(Arc::new(chronicler));
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

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn post(app: &Router, uri: &str, body: Body) -> (StatusCode, Value) {
    send(
        app,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(body)
            .unwrap(),
    )
    .await
}

async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    send(
        app,
        Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

#[tokio::test]
async fn history_with_an_empty_body_runs_the_default_task() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/history", fx.project.id);
    let (status, json) = post(&fx.app, &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["project_id"], fx.project.id.as_str());
    assert_eq!(json["task"]["kind"], "default");
    assert!(json["prompt_payload"].is_null());
    assert_eq!(json["run"]["agent"], "chronicler");
    assert_eq!(json["run"]["project_id"], fx.project.id.as_str());
    assert_eq!(json["citations"], json!([]));
    assert_eq!(json["tool_calls"].as_array().unwrap().len(), 1);
    assert_eq!(json["tool_calls"][0]["tool"], "git_log");
    assert_eq!(
        json["summary_payload"]["content"],
        "Recent work: added the importer."
    );
    assert_eq!(json["nodes"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn history_with_a_prompt_records_the_prompt_node() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/history", fx.project.id);
    let body = json!({"prompt": "what changed in auth?", "author": "ada"});
    let (status, json) = post(&fx.app, &uri, Body::from(body.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["task"]["kind"], "prompt");
    assert_eq!(json["prompt_payload"]["content"], "what changed in auth?");
    let nodes = json["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0]["author"], "ada");
    assert_eq!(nodes[1]["parent_id"], nodes[0]["id"]);
    assert_eq!(json["run_inputs"][0]["node_id"], nodes[0]["id"]);
}

#[tokio::test]
async fn history_rejects_a_malformed_or_oversized_body() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/history", fx.project.id);
    let (status, json) = post(&fx.app, &uri, Body::from("{not json")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["kind"], "invalid_request");

    let body = json!({"prompt": "x".repeat(5000)});
    let (status, json) = post(&fx.app, &uri, Body::from(body.to_string())).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json["kind"], "oversized_input");
}

#[tokio::test]
async fn history_for_an_unknown_project_is_not_found() {
    let fx = fixture(None, true);
    let (status, json) = post(&fx.app, "/projects/project-nope/history", Body::empty()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["kind"], "not_found");
}

#[tokio::test]
async fn history_with_a_denying_policy_is_forbidden() {
    for policy in [
        Arc::new(DenyAll) as Arc<dyn RootPolicy>,
        Arc::new(NoChronicler),
    ] {
        let fx = fixture(Some(policy), true);
        let uri = format!("/projects/{}/history", fx.project.id);
        let (status, json) = post(&fx.app, &uri, Body::empty()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        assert_eq!(json["kind"], "forbidden");
    }
}

#[tokio::test]
async fn history_for_a_removed_folder_is_unprocessable() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/history", fx.project.id);
    drop(fx.folder);
    let (status, json) = post(&fx.app, &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["kind"], "project_unavailable");
}

#[tokio::test]
async fn history_without_a_configured_chronicler_is_an_internal_error() {
    let fx = fixture(None, false);
    let uri = format!("/projects/{}/history", fx.project.id);
    let (status, json) = post(&fx.app, &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json["kind"], "internal_error");
}

#[tokio::test]
async fn agents_lists_every_configured_agent_without_credentials() {
    let fx = fixture(None, true);
    let (status, json) = get(&fx.app, "/agents").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let agents = json.as_array().unwrap();
    let kinds: Vec<&str> = agents.iter().map(|a| a["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["scribe", "explainer", "chronicler"]);
    let chronicler = &agents[2];
    assert_eq!(chronicler["provider"], "neuralwatt");
    assert_eq!(chronicler["model"], "glm-5.3-flash");
    assert_eq!(chronicler["prompt_version"], "chronicler-summary-v1");
    assert_eq!(agents[0]["prompt_version"], "scribe-v4");

    // Without a Chronicler it is simply not listed.
    let fx = fixture(None, false);
    let (_, json) = get(&fx.app, "/agents").await;
    assert_eq!(json.as_array().unwrap().len(), 2);
}
