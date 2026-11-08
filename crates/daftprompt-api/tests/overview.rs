//! `GET /projects/{id}/profile`, `POST /projects/{id}/overview` and the
//! Surveyor in `GET /agents`. A stub Surveyor model over a real temporary
//! folder; no credentials or network.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use daftprompt_api::{ApiConfig, AppState, router};
use daftprompt_core::surveyor::{SurveyorInput, SurveyorModel};
use daftprompt_core::{
    AccessError, AgentKind, BoxFuture, ExtractionModel, ModelError, ModelInput, Principal,
    ProjectScope, RootPolicy, Scribe, ScribeConfig, Store, Surveyor, SurveyorConfig,
};
use daftprompt_types::{BriefingCitation, BriefingExtraction, Extraction, Project};
use serde_json::{Value, json};
use tower::ServiceExt;

struct ScribeStub;

impl ExtractionModel for ScribeStub {
    fn extract(&self, _input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        Box::pin(async { Ok(Extraction::default()) })
    }
}

/// Reads the README, then cites its first line.
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
                summary: format!("A demo project [1] ({}).", input.prompt.len()),
                citations: vec![BriefingCitation::File {
                    path: "README.md".to_owned(),
                    start_line: 1,
                    end_line: 1,
                }],
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
    ) -> Result<ProjectScope, AccessError> {
        Err(AccessError::Denied(format!(
            "{} may not use project {}",
            agent.as_str(),
            project.id
        )))
    }
}

/// Denies only the Surveyor: the policy sees the agent kind.
struct NoSurveyor;

impl RootPolicy for NoSurveyor {
    fn grant(
        &self,
        principal: &Principal,
        agent: AgentKind,
        project: &Project,
    ) -> Result<ProjectScope, AccessError> {
        if agent == AgentKind::Surveyor {
            return Err(AccessError::Denied("no survey for you".to_owned()));
        }
        daftprompt_core::LocalPolicy.grant(principal, agent, project)
    }
}

struct Fixture {
    app: Router,
    project: Project,
    folder: tempfile::TempDir,
    _db: tempfile::TempDir,
}

fn fixture(policy: Option<Arc<dyn RootPolicy>>, with_surveyor: bool) -> Fixture {
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
    std::fs::write(folder.path().join("web/pnpm-lock.yaml"), "").unwrap();
    let (project, _) = store.add_project(folder.path(), Some("demo")).unwrap();

    let mut state = AppState::new(
        Arc::new(Scribe::new(ScribeConfig::new(), ScribeStub)),
        ApiConfig::default(),
    )
    .with_store(Arc::new(store));
    if with_surveyor {
        state = state.with_surveyor(Arc::new(Surveyor::new(
            SurveyorConfig::default(),
            SurveyorStub,
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
async fn profile_lists_languages_and_key_files_without_a_surveyor() {
    // No Surveyor is configured: the profile needs no model.
    let fx = fixture(None, false);
    let uri = format!("/projects/{}/profile", fx.project.id);
    let (status, json) = call(&fx.app, "GET", &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let languages = json["languages"].as_array().unwrap();
    assert_eq!(languages.len(), 2);
    assert_eq!(languages[0]["language"], "rust");
    assert_eq!(languages[0]["projects"][0]["name"], "demo");
    assert_eq!(languages[0]["projects"][0]["package_manager"], "cargo");
    assert_eq!(languages[1]["language"], "typescript");
    assert_eq!(languages[1]["projects"][0]["dir"], "web");
    assert_eq!(languages[1]["projects"][0]["package_manager"], "pnpm");
    assert_eq!(
        json["key_files"],
        json!(["README.md", "Cargo.toml", "web/package.json"])
    );
}

#[tokio::test]
async fn profile_for_an_unknown_project_is_not_found() {
    let fx = fixture(None, true);
    let (status, json) = call(
        &fx.app,
        "GET",
        "/projects/project-nope/profile",
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["kind"], "not_found");
}

#[tokio::test]
async fn profile_with_a_denying_policy_is_forbidden() {
    for policy in [
        Arc::new(DenyAll) as Arc<dyn RootPolicy>,
        Arc::new(NoSurveyor),
    ] {
        let fx = fixture(Some(policy), true);
        let uri = format!("/projects/{}/profile", fx.project.id);
        let (status, json) = call(&fx.app, "GET", &uri, Body::empty()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        assert_eq!(json["kind"], "forbidden");
    }
}

#[tokio::test]
async fn profile_for_a_removed_folder_is_unprocessable() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/profile", fx.project.id);
    drop(fx.folder);
    let (status, json) = call(&fx.app, "GET", &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["kind"], "project_unavailable");
}

#[tokio::test]
async fn overview_with_an_empty_body_runs_the_default_task_and_includes_the_profile() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/overview", fx.project.id);
    let (status, json) = call(&fx.app, "POST", &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["project_id"], fx.project.id.as_str());
    assert_eq!(json["task"]["kind"], "default");
    assert_eq!(json["run"]["agent"], "surveyor");
    assert_eq!(json["run"]["prompt_version"], "surveyor-v3");
    assert_eq!(json["citations"][0]["location"]["kind"], "file");
    assert_eq!(json["citations"][0]["location"]["path"], "README.md");
    assert_eq!(json["node_contexts"][1]["target"]["kind"], "file");
    assert_eq!(json["profile"]["languages"][0]["language"], "rust");
    assert_eq!(json["profile"]["key_files"][0], "README.md");
}

#[tokio::test]
async fn overview_with_a_prompt_records_the_prompt_node() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/overview", fx.project.id);
    let body = json!({"prompt": "how is it laid out?", "author": "ada"});
    let (status, json) = call(&fx.app, "POST", &uri, Body::from(body.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["task"]["kind"], "prompt");
    assert_eq!(json["prompt_payload"]["content"], "how is it laid out?");
    assert_eq!(json["nodes"][0]["author"], "ada");
}

#[tokio::test]
async fn overview_rejects_a_malformed_or_oversized_body() {
    let fx = fixture(None, true);
    let uri = format!("/projects/{}/overview", fx.project.id);
    let (status, json) = call(&fx.app, "POST", &uri, Body::from("{not json")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["kind"], "invalid_request");
    let body = json!({"prompt": "x".repeat(5000)});
    let (status, json) = call(&fx.app, "POST", &uri, Body::from(body.to_string())).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json["kind"], "oversized_input");
}

#[tokio::test]
async fn overview_for_an_unknown_project_is_not_found() {
    let fx = fixture(None, true);
    let (status, json) = call(
        &fx.app,
        "POST",
        "/projects/project-nope/overview",
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["kind"], "not_found");
}

#[tokio::test]
async fn overview_with_a_denying_policy_is_forbidden() {
    for policy in [
        Arc::new(DenyAll) as Arc<dyn RootPolicy>,
        Arc::new(NoSurveyor),
    ] {
        let fx = fixture(Some(policy), true);
        let uri = format!("/projects/{}/overview", fx.project.id);
        let (status, json) = call(&fx.app, "POST", &uri, Body::empty()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        assert_eq!(json["kind"], "forbidden");
    }
}

#[tokio::test]
async fn overview_without_a_configured_surveyor_is_an_internal_error() {
    let fx = fixture(None, false);
    let uri = format!("/projects/{}/overview", fx.project.id);
    let (status, json) = call(&fx.app, "POST", &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json["kind"], "internal_error");
}

#[tokio::test]
async fn agents_lists_the_surveyor_when_configured() {
    let fx = fixture(None, true);
    let (status, json) = call(&fx.app, "GET", "/agents", Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let agents = json.as_array().unwrap();
    let kinds: Vec<&str> = agents.iter().map(|a| a["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["scribe", "surveyor"]);
    assert_eq!(agents[1]["prompt_version"], "surveyor-v3");
    assert_eq!(agents[1]["provider"], "neuralwatt");
}
