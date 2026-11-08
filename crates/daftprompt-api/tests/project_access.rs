//! `project_id` on `POST /parse` and `POST /clarify`: resolution against the
//! registry and the access policy. Stub models only; no credentials.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use daftprompt_api::{ApiConfig, AppState, router};
use daftprompt_core::explainer::{ExplainerInput, ExplainerModel};
use daftprompt_core::{
    AccessError, AgentKind, BoxFuture, Explainer, ExplainerConfig, ExtractionModel, ModelError,
    ModelInput, Principal, ProjectScope, RootPolicy, Scribe, ScribeConfig, Store,
};
use daftprompt_types::{ExplainerCitation, ExplainerExtraction, Extraction, Project, ProjectPath};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const SOURCE: &str = "We use a token bucket.\nIt refills slowly.";
const LIMITER: &str = "// rate limiter\nstruct Bucket {\n    capacity: u32,\n}\n";

struct ScribeStub;

impl ExtractionModel for ScribeStub {
    fn extract(&self, _input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        Box::pin(async { Ok(Extraction::default()) })
    }
}

/// Reads `src/limit.rs` through the toolbox, cites it, and records whether it
/// had project tools.
struct ExplainerStub {
    prompts: Arc<Mutex<Vec<String>>>,
}

impl ExplainerModel for ExplainerStub {
    fn explain(
        &self,
        input: ExplainerInput,
    ) -> daftprompt_core::explainer::BoxFuture<'_, Result<ExplainerExtraction, ModelError>> {
        self.prompts.lock().unwrap().push(input.prompt.clone());
        Box::pin(async move {
            let citations = match &input.toolbox {
                Some(toolbox) => {
                    toolbox.call(
                        "read_file",
                        json!({"path": "src/limit.rs", "start_line": 1, "end_line": 3}),
                    );
                    vec![ExplainerCitation::File {
                        path: "src/limit.rs".to_owned(),
                        start_line: 1,
                        end_line: 3,
                    }]
                }
                None => vec![ExplainerCitation::Source {
                    start_segment: 1,
                    end_segment: 1,
                }],
            };
            Ok(ExplainerExtraction {
                explanation: "It is a limiter [1].".to_owned(),
                citations,
            })
        })
    }
}

/// A policy that refuses every project.
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

/// Records the path id of every grant.
struct Recording(Mutex<Vec<String>>);

impl RootPolicy for Recording {
    fn grant(
        &self,
        principal: &Principal,
        agent: AgentKind,
        project: &Project,
        path: &ProjectPath,
    ) -> Result<ProjectScope, AccessError> {
        self.0.lock().unwrap().push(path.id.clone());
        daftprompt_core::LocalPolicy.grant(principal, agent, project, path)
    }
}

struct Fixture {
    app: Router,
    project: Project,
    folder: tempfile::TempDir,
    prompts: Arc<Mutex<Vec<String>>>,
    _db: tempfile::TempDir,
}

fn fixture(policy: Option<Arc<dyn RootPolicy>>) -> Fixture {
    let db = tempfile::tempdir().unwrap();
    let store = Store::open(db.path().join("daftprompt.db")).unwrap();
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("src")).unwrap();
    std::fs::write(folder.path().join("src/limit.rs"), LIMITER).unwrap();
    let (project, _) = store.add_project(folder.path(), Some("limiter")).unwrap();

    let prompts = Arc::new(Mutex::new(Vec::new()));
    let scribe = Scribe::new(ScribeConfig::new(), ScribeStub);
    let explainer = Explainer::new(
        ExplainerConfig::default(),
        ExplainerStub {
            prompts: prompts.clone(),
        },
    );
    let mut state = AppState::new(Arc::new(scribe), ApiConfig::default())
        .with_clarifier(Arc::new(explainer))
        .with_store(Arc::new(store));
    if let Some(policy) = policy {
        state = state.with_policy(policy);
    }
    Fixture {
        app: router(state),
        project,
        folder,
        prompts,
        _db: db,
    }
}

async fn post(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
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

fn parse_body(project_id: Option<&str>) -> Value {
    let mut body = json!({ "text": SOURCE });
    if let Some(id) = project_id {
        body["project_id"] = json!(id);
    }
    body
}

fn clarify_body(project_id: Option<&str>) -> Value {
    let start = SOURCE.find("token bucket").unwrap();
    let mut body = json!({
        "source": {
            "text": SOURCE,
            "payload_hash": format!("{:x}", Sha256::digest(SOURCE.as_bytes())),
        },
        "target": {
            "node_id": "node-target",
            "display_id": 1,
            "start_byte": start,
            "end_byte": start + "token bucket".len(),
        },
        "question": "What does the bucket mean here?",
    });
    if let Some(id) = project_id {
        body["project_id"] = json!(id);
    }
    body
}

// ---------------------------------------------------------------------------
// POST /parse
// ---------------------------------------------------------------------------

#[tokio::test]
async fn parse_with_a_project_id_records_it_on_the_run() {
    let fx = fixture(None);
    let (status, json) = post(&fx.app, "/parse", parse_body(Some(&fx.project.id))).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["run"]["project_id"], fx.project.id.as_str());
}

#[tokio::test]
async fn parse_without_a_project_id_records_none() {
    let fx = fixture(None);
    let (status, json) = post(&fx.app, "/parse", parse_body(None)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["run"].get("project_id").is_none());
}

#[tokio::test]
async fn parse_with_an_unknown_project_is_not_found() {
    let fx = fixture(None);
    let (status, json) = post(&fx.app, "/parse", parse_body(Some("project-nope"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["kind"], "not_found");
}

#[tokio::test]
async fn parse_with_a_denying_policy_is_forbidden() {
    let fx = fixture(Some(Arc::new(DenyAll)));
    let (status, json) = post(&fx.app, "/parse", parse_body(Some(&fx.project.id))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["kind"], "forbidden");

    // No project_id means no policy question.
    let (status, _) = post(&fx.app, "/parse", parse_body(None)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_folder_removed_after_registration_is_unprocessable() {
    let fx = fixture(None);
    let id = fx.project.id.clone();
    drop(fx.folder);
    let (status, json) = post(&fx.app, "/parse", parse_body(Some(&id))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["kind"], "project_unavailable");
}

// ---------------------------------------------------------------------------
// POST /clarify
// ---------------------------------------------------------------------------

#[tokio::test]
async fn clarify_with_a_project_id_reads_that_folder() {
    let fx = fixture(None);
    let (status, json) = post(&fx.app, "/clarify", clarify_body(Some(&fx.project.id))).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["grounding"], "project");
    assert_eq!(json["run"]["project_id"], fx.project.id.as_str());
    assert!(fx.prompts.lock().unwrap()[0].contains("project"));
}

#[tokio::test]
async fn clarify_without_a_project_id_stays_text_only() {
    let fx = fixture(None);
    let (status, json) = post(&fx.app, "/clarify", clarify_body(None)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["grounding"], "text_only");
}

#[tokio::test]
async fn clarify_with_an_unknown_project_is_not_found() {
    let fx = fixture(None);
    let (status, json) = post(&fx.app, "/clarify", clarify_body(Some("project-nope"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["kind"], "not_found");
    assert!(fx.prompts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn clarify_with_a_denying_policy_is_forbidden_before_any_model_call() {
    let fx = fixture(Some(Arc::new(DenyAll)));
    let (status, json) = post(&fx.app, "/clarify", clarify_body(Some(&fx.project.id))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["kind"], "forbidden");
    assert!(fx.prompts.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// project_path_id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bodies_without_a_path_id_use_the_default_and_a_path_id_selects_another() {
    let recording = Arc::new(Recording(Mutex::default()));
    let fx = fixture(Some(recording.clone()));
    let second = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(second.path().join("src")).unwrap();
    std::fs::write(second.path().join("src/limit.rs"), LIMITER).unwrap();
    let (status, added) = post(
        &fx.app,
        &format!("/projects/{}/paths", fx.project.id),
        json!({ "path": second.path() }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let second_id = added["id"].as_str().unwrap().to_owned();

    // Old-style bodies: parse and clarify resolve the persisted default.
    let (status, _) = post(&fx.app, "/parse", parse_body(Some(&fx.project.id))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post(&fx.app, "/clarify", clarify_body(Some(&fx.project.id))).await;
    assert_eq!(status, StatusCode::OK);

    // An explicit path id reaches the policy.
    let mut body = parse_body(Some(&fx.project.id));
    body["project_path_id"] = json!(second_id);
    let (status, _) = post(&fx.app, "/parse", body).await;
    assert_eq!(status, StatusCode::OK);
    let mut body = clarify_body(Some(&fx.project.id));
    body["project_path_id"] = json!(second_id);
    let (status, _) = post(&fx.app, "/clarify", body).await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        *recording.0.lock().unwrap(),
        [
            fx.project.default_path_id.clone(),
            fx.project.default_path_id.clone(),
            second_id.clone(),
            second_id,
        ]
    );
}

#[tokio::test]
async fn a_wrong_or_orphan_path_id_is_rejected_before_the_policy() {
    let recording = Arc::new(Recording(Mutex::default()));
    let fx = fixture(Some(recording.clone()));

    let mut body = parse_body(Some(&fx.project.id));
    body["project_path_id"] = json!("path-nope");
    let (status, json) = post(&fx.app, "/parse", body).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["kind"], "not_found");

    let mut body = parse_body(None);
    body["project_path_id"] = json!(fx.project.default_path_id);
    let (status, json) = post(&fx.app, "/parse", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["kind"], "invalid_request");
    assert!(recording.0.lock().unwrap().is_empty());
}
