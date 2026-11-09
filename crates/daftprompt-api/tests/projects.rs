//! `/projects` contract tests over a temporary database. No credentials,
//! network, or provider are involved.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use daftprompt_api::{ApiConfig, AppState, router};
use daftprompt_core::{BoxFuture, Parser, Store};
use daftprompt_types::{ParseError, ParseRequest, ParseResult};
use serde_json::{Value, json};
use tower::ServiceExt;

struct NoParser;

impl Parser for NoParser {
    fn parse(&self, _request: ParseRequest) -> BoxFuture<'_, Result<ParseResult, ParseError>> {
        Box::pin(async {
            Err(ParseError::Internal {
                detail: "unused".to_owned(),
            })
        })
    }
}

struct Fixture {
    app: Router,
    _db: tempfile::TempDir,
    folders: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let db = tempfile::tempdir().unwrap();
        let store = Store::open(db.path().join("data/daftprompt.db")).unwrap();
        let state =
            AppState::new(Arc::new(NoParser), ApiConfig::default()).with_store(Arc::new(store));
        Self {
            app: router(state),
            _db: db,
            folders: tempfile::tempdir().unwrap(),
        }
    }

    fn folder(&self, name: &str) -> PathBuf {
        let path = self.folders.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let request = Request::builder().method(method).uri(uri);
        let request = match body {
            Some(body) => request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }
}

#[tokio::test]
async fn project_lifecycle() {
    let fx = Fixture::new();
    let path = fx.folder("notes");

    let (status, list) = fx.call("GET", "/projects", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list, json!([]));

    let (status, created) = fx
        .call("POST", "/projects", Some(json!({ "path": path })))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["name"], "notes");
    let id = created["id"].as_str().unwrap().to_owned();

    // Adding the same folder again returns the existing project.
    let (status, again) = fx
        .call(
            "POST",
            "/projects",
            Some(json!({ "path": path, "name": "x" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, created);

    let (status, got) = fx.call("GET", &format!("/projects/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got, created);

    let (status, opened) = fx.call("POST", &format!("/projects/{id}/open"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(opened["id"], id.as_str());

    let (status, renamed) = fx
        .call(
            "PATCH",
            &format!("/projects/{id}"),
            Some(json!({ "name": "Renamed" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["name"], "Renamed");

    let (status, list) = fx.call("GET", "/projects", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);

    let (status, _) = fx.call("DELETE", &format!("/projects/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(path.is_dir());

    let (status, error) = fx.call("GET", &format!("/projects/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["kind"], "not_found");
}

#[tokio::test]
async fn invalid_requests_use_the_error_shape() {
    let fx = Fixture::new();

    let (status, error) = fx
        .call("POST", "/projects", Some(json!({ "path": "/no/such/dir" })))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["kind"], "invalid_request");
    assert!(error["detail"].is_string());

    let (status, error) = fx
        .call("POST", "/projects", Some(json!({ "nope": 1 })))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["kind"], "invalid_request");

    let path = fx.folder("a");
    let (_, created) = fx
        .call("POST", "/projects", Some(json!({ "path": path })))
        .await;
    let id = created["id"].as_str().unwrap();
    let (status, error) = fx
        .call(
            "PATCH",
            &format!("/projects/{id}"),
            Some(json!({ "name": " " })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["kind"], "invalid_request");

    for (method, uri) in [
        ("POST", "/projects/missing/open"),
        ("DELETE", "/projects/missing"),
    ] {
        let (status, error) = fx.call(method, uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        assert_eq!(error["kind"], "not_found");
    }
}

#[tokio::test]
async fn without_a_store_the_routes_answer_internal_error() {
    let app = router(AppState::new(Arc::new(NoParser), ApiConfig::default()));
    let response = app
        .oneshot(Request::get("/projects").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn project_paths_lifecycle() {
    let fx = Fixture::new();
    let first = fx.folder("first");
    let second = fx.folder("second");
    let (_, project) = fx
        .call("POST", "/projects", Some(json!({ "path": first })))
        .await;
    let id = project["id"].as_str().unwrap().to_owned();
    let first_path = project["default_path_id"].as_str().unwrap().to_owned();
    assert_eq!(project["paths"].as_array().unwrap().len(), 1);
    assert_eq!(project["paths"][0]["id"], first_path.as_str());
    assert_eq!(project["paths"][0]["is_default"], true);
    assert_eq!(project["root_path"], project["paths"][0]["root_path"]);

    // Attach: 201, then idempotent 200.
    let uri = format!("/projects/{id}/paths");
    let (status, added) = fx.call("POST", &uri, Some(json!({ "path": second }))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(added["is_default"], false);
    assert_eq!(added["project_id"], id.as_str());
    let second_path = added["id"].as_str().unwrap().to_owned();
    let (status, again) = fx.call("POST", &uri, Some(json!({ "path": second }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, added);

    let (status, list) = fx.call("GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (_, fetched) = fx.call("GET", &format!("/projects/{id}"), None).await;
    assert_eq!(fetched["paths"].as_array().unwrap().len(), 2);
    let (_, all) = fx.call("GET", "/projects", None).await;
    assert_eq!(all[0]["paths"].as_array().unwrap().len(), 2);

    // A path owned by another project is rejected; a bad folder too.
    let other = fx.folder("other");
    let (_, other_project) = fx
        .call("POST", "/projects", Some(json!({ "path": other })))
        .await;
    let other_id = other_project["id"].as_str().unwrap();
    let (status, error) = fx
        .call(
            "POST",
            &format!("/projects/{other_id}/paths"),
            Some(json!({ "path": second })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["kind"], "invalid_request");
    let (status, _) = fx
        .call(
            "POST",
            &uri,
            Some(json!({ "path": fx.folder("x").join("no") })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = fx.call("GET", "/projects/project-nope/paths", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // PATCH: wrong path ids are 404 and apply nothing; rename plus default works.
    let patch = format!("/projects/{id}");
    let foreign = other_project["default_path_id"].as_str().unwrap();
    let (status, _) = fx
        .call(
            "PATCH",
            &patch,
            Some(json!({ "name": "Renamed", "default_path_id": foreign })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, updated) = fx
        .call(
            "PATCH",
            &patch,
            Some(json!({ "name": "Renamed", "default_path_id": second_path })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "Renamed");
    assert_eq!(updated["default_path_id"], second_path.as_str());
    assert_eq!(updated["root_path"], added["root_path"]);
    let (status, _) = fx.call("PATCH", &patch, Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, renamed) = fx
        .call("PATCH", &patch, Some(json!({ "name": "Only name" })))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["default_path_id"], second_path.as_str());

    // DELETE: the default needs a replacement.
    let remove = |path: &str| format!("/projects/{id}/paths/{path}");
    let (status, error) = fx.call("DELETE", &remove(&second_path), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["kind"], "invalid_request");
    let (status, _) = fx.call("DELETE", &remove(foreign), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, after) = fx
        .call(
            "DELETE",
            &format!(
                "{}?replacement_default_path_id={first_path}",
                remove(&second_path)
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after["default_path_id"], first_path.as_str());
    assert_eq!(after["paths"].as_array().unwrap().len(), 1);
    assert!(second.is_dir());

    // The last path cannot go.
    let (status, _) = fx.call("DELETE", &remove(&first_path), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn profile_follows_the_selected_path() {
    let fx = Fixture::new();
    let first = fx.folder("first");
    let second = fx.folder("second");
    std::fs::write(second.join("Cargo.toml"), "[package]\nname = \"two\"\n").unwrap();
    let (_, project) = fx
        .call("POST", "/projects", Some(json!({ "path": first })))
        .await;
    let id = project["id"].as_str().unwrap();
    let (_, added) = fx
        .call(
            "POST",
            &format!("/projects/{id}/paths"),
            Some(json!({ "path": second })),
        )
        .await;
    let second_path = added["id"].as_str().unwrap();

    let (status, default) = fx
        .call("GET", &format!("/projects/{id}/profile"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, selected) = fx
        .call(
            "GET",
            &format!("/projects/{id}/profile?project_path_id={second_path}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(default, selected);

    let (status, error) = fx
        .call(
            "GET",
            &format!("/projects/{id}/profile?project_path_id=path-nope"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["kind"], "not_found");

    // A missing secondary folder is unavailable; the default still works.
    std::fs::remove_dir_all(&second).unwrap();
    let (status, error) = fx
        .call(
            "GET",
            &format!("/projects/{id}/profile?project_path_id={second_path}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["kind"], "project_unavailable");
    let (status, _) = fx
        .call("GET", &format!("/projects/{id}/profile"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
}
