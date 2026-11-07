//! HTTP layer for daftprompt.
//!
//! The service is stateless: one submission in, one complete validated graph
//! fragment out. The shared [`ParseRequest`], [`ParseResult`], and
//! [`ParseError`] types cross the wire unchanged; this crate only handles the
//! HTTP envelope and maps explicit parse failures to status codes.
//!
//! `POST /parse` accepts a [`ParseRequest`] as JSON and answers with a
//! [`ParseResult`]. A successful result contains the payload text, the graph
//! records, and each span node's half-open byte range, so a client can render
//! blocks, labels, assignees, and answer targets without recomputing anything.
//!
//! Failures are returned as the serialized [`ParseError`], which carries its
//! own machine-readable `kind` discriminant:
//!
//! | error kind         | status |
//! |--------------------|--------|
//! | `invalid_request`  | 400    |
//! | `oversized_input`  | 413    |
//! | `invalid_output`   | 422    |
//! | `provider_error`   | 502    |
//! | `timeout`          | 504    |
//! | `internal_error`   | 500    |
//!
//! `POST /clarify` accepts a [`ClarifyRequest`] and answers with a
//! [`ClarifyResult`]: one explanation of one extracted span, with citations.
//! It uses the same error mapping; a payload hash mismatch or an invalid target
//! is `400`. It never changes the parse result.
//!
//! Saved projects (named local folders) live in the SQLite store and are
//! served by `GET/POST /projects`, `GET/PATCH/DELETE /projects/{id}`, and
//! `POST /projects/{id}/open`. Their failures use the same `{ "kind", "detail" }`
//! body with the kinds `invalid_request` (400), `not_found` (404), and
//! `internal_error` (500). See `docs/api.md`.
//!
//! `POST /parse` and `POST /clarify` may name a saved project with
//! `project_id`. The service resolves it against the registry and its
//! [`RootPolicy`] for the calling [`Principal`] (always [`Principal::Local`]
//! for now), so a client never sends a filesystem path. Failures add
//! `not_found` (404, unknown id), `forbidden` (403, the policy refused) and
//! `project_unavailable` (422, the registered folder is gone or unreadable).
//!
//! `GET /agents` lists the configured agents. `POST /projects/{id}/history`
//! takes an [`AgentTaskRequest`] and answers with a [`BriefingResult`] from the
//! Chronicler (git history), resolved for [`AgentKind::Chronicler`] like the
//! other project routes. `POST /projects/{id}/overview` does the same with the
//! Surveyor (files and documentation, [`AgentKind::Surveyor`]), and
//! `GET /projects/{id}/profile` answers the deterministic `ProjectProfile`
//! (languages and key files) with no model call. `POST /projects/{id}/commands`
//! answers a [`CommandsResult`] from the Operator (the commands the project
//! supports, discovered and never run, [`AgentKind::Operator`]).
//!
//! A request whose JSON envelope is too large is rejected with 413 before it
//! can reach the parser. A partly valid graph is never returned.

use std::convert::Infallible;
use std::error::Error as _;
use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::Path;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use daftprompt_core::{
    AccessError, AgentKind, Briefer, Clarifier, CommandsBriefer, DEFAULT_MAX_INPUT_BYTES,
    LocalPolicy, Parser, Principal, ProjectScope, RootPolicy, Store, StoreError, resolve_project,
};
use http_body_util::LengthLimitError;
use serde::de::DeserializeOwned;
use serde_json::json;

pub use daftprompt_types::{
    AgentInfo, AgentTaskRequest, BriefingResult, ClarifyRequest, ClarifyResult, CommandsResult,
    CreateProjectRequest, ParseError, ParseRequest, ParseResult, Project, RenameProjectRequest,
};

/// Room for JSON escaping and source metadata beyond the text itself.
///
/// The input text limit is enforced by the parser in UTF-8 bytes. The JSON
/// envelope can be larger: a control character becomes a six-byte `\u00xx`
/// escape, and optional source metadata travels in the same object. The
/// envelope is therefore six times the text limit plus 16 KiB, so ordinary
/// oversized text reaches the parser and gets the documented 413 instead of
/// being rejected by the transport.
const ENVELOPE_HEADROOM: usize = 16 * 1024;

/// HTTP envelope limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiConfig {
    /// Maximum accepted size of the JSON request body.
    pub max_body_bytes: usize,
}

impl ApiConfig {
    /// A configuration with an explicit envelope limit.
    pub fn new(max_body_bytes: usize) -> Self {
        Self { max_body_bytes }
    }

    /// An envelope sized for a parser whose text limit is `max_input_bytes`.
    pub fn for_input_limit(max_input_bytes: usize) -> Self {
        Self {
            max_body_bytes: max_input_bytes
                .saturating_mul(6)
                .saturating_add(ENVELOPE_HEADROOM),
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self::for_input_limit(DEFAULT_MAX_INPUT_BYTES)
    }
}

/// Shared state for the parse route.
///
/// The parser is behind a trait object so tests can supply a deterministic
/// stub and production can hold Scribe without leaking its model type into
/// the HTTP layer.
#[derive(Clone)]
pub struct AppState {
    parser: Arc<dyn Parser>,
    clarifier: Option<Arc<dyn Clarifier>>,
    chronicler: Option<Arc<dyn Briefer>>,
    surveyor: Option<Arc<dyn Briefer>>,
    operator: Option<Arc<dyn CommandsBriefer>>,
    store: Option<Arc<Store>>,
    policy: Arc<dyn RootPolicy>,
    config: ApiConfig,
}

impl AppState {
    /// State over `parser` with `config`.
    pub fn new(parser: Arc<dyn Parser>, config: ApiConfig) -> Self {
        Self {
            parser,
            clarifier: None,
            chronicler: None,
            surveyor: None,
            operator: None,
            store: None,
            policy: Arc::new(LocalPolicy),
            config,
        }
    }

    /// Serve `POST /clarify` with `clarifier`. Without one the route answers
    /// `500 internal_error`.
    pub fn with_clarifier(mut self, clarifier: Arc<dyn Clarifier>) -> Self {
        self.clarifier = Some(clarifier);
        self
    }

    /// Serve `POST /projects/{id}/history` with `chronicler`. Without one the
    /// route answers `500 internal_error`.
    pub fn with_chronicler(mut self, chronicler: Arc<dyn Briefer>) -> Self {
        self.chronicler = Some(chronicler);
        self
    }

    /// Serve `POST /projects/{id}/overview` with `surveyor`. Without one the
    /// route answers `500 internal_error`.
    pub fn with_surveyor(mut self, surveyor: Arc<dyn Briefer>) -> Self {
        self.surveyor = Some(surveyor);
        self
    }

    /// Serve `POST /projects/{id}/commands` with `operator`. Without one the
    /// route answers `500 internal_error`.
    pub fn with_operator(mut self, operator: Arc<dyn CommandsBriefer>) -> Self {
        self.operator = Some(operator);
        self
    }

    /// Serve the `/projects` routes from `store`. Without one they answer
    /// `500 internal_error`.
    pub fn with_store(mut self, store: Arc<Store>) -> Self {
        self.store = Some(store);
        self
    }

    /// Decide which saved projects a caller may use, replacing the default
    /// [`LocalPolicy`] (any project, any agent).
    pub fn with_policy(mut self, policy: Arc<dyn RootPolicy>) -> Self {
        self.policy = policy;
        self
    }

    /// The active envelope configuration.
    pub fn config(&self) -> ApiConfig {
        self.config
    }
}

/// Who is calling. Always [`Principal::Local`] today; authentication
/// middleware would replace this extractor and nothing else.
pub struct Caller(pub Principal);

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = Infallible;

    async fn from_request_parts(_parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(Principal::Local))
    }
}

/// The HTTP router.
///
/// `POST /parse` and `POST /clarify` are stateless; the only persisted
/// resource is the project registry under `/projects`.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/parse", post(parse))
        .route("/clarify", post(clarify))
        .route("/agents", get(list_agents))
        .route("/projects", get(list_projects).post(create_project))
        .route(
            "/projects/{id}",
            get(get_project)
                .patch(rename_project)
                .delete(delete_project),
        )
        .route("/projects/{id}/open", post(open_project))
        .route("/projects/{id}/history", post(project_history))
        .route("/projects/{id}/overview", post(project_overview))
        .route("/projects/{id}/commands", post(project_commands))
        .route("/projects/{id}/profile", get(project_profile))
        .with_state(state)
}

/// A clarify body carries the source text plus earlier turns and a question,
/// so its envelope is twice the parse envelope.
fn clarify_body_limit(config: ApiConfig) -> usize {
    config.max_body_bytes.saturating_mul(2)
}

async fn clarify(
    State(state): State<AppState>,
    Caller(principal): Caller,
    request: Request,
) -> Response {
    let limit = clarify_body_limit(state.config);
    let body = match to_bytes(request.into_body(), limit).await {
        Ok(body) => body,
        Err(error) => return error_response(body_error(&error, limit)),
    };

    let request: ClarifyRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => {
            return error_response(ParseError::InvalidRequest {
                detail: format!(
                    "the request body is not a valid ClarifyRequest JSON object: {error}"
                ),
            });
        }
    };

    let Some(clarifier) = &state.clarifier else {
        return error_response(ParseError::Internal {
            detail: "clarification is not configured on this service".to_owned(),
        });
    };

    let scope = match resolve_scope(
        &state,
        &principal,
        AgentKind::Explainer,
        request.project_id.as_deref(),
    ) {
        Ok(scope) => scope,
        Err(failure) => return failure.into_response(),
    };

    match clarifier.clarify_in(request, scope.as_ref()).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => error_response(error),
    }
}

async fn parse(
    State(state): State<AppState>,
    Caller(principal): Caller,
    request: Request,
) -> Response {
    let body = match to_bytes(request.into_body(), state.config.max_body_bytes).await {
        Ok(body) => body,
        Err(error) => return error_response(body_error(&error, state.config.max_body_bytes)),
    };

    let request: ParseRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => {
            return error_response(ParseError::InvalidRequest {
                detail: format!(
                    "the request body is not a valid ParseRequest JSON object: {error}"
                ),
            });
        }
    };

    let scope = match resolve_scope(
        &state,
        &principal,
        AgentKind::Scribe,
        request.project_id.as_deref(),
    ) {
        Ok(scope) => scope,
        Err(failure) => return failure.into_response(),
    };

    match state.parser.parse_in(request, scope.as_ref()).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => error_response(error),
    }
}

/// `GET /agents`: every configured agent with its provider, model, and prompt
/// version. Providers and models are configuration, never credentials.
async fn list_agents(State(state): State<AppState>) -> Json<Vec<AgentInfo>> {
    let agents = [
        state.parser.info(),
        state.clarifier.as_ref().and_then(|c| c.info()),
        state.chronicler.as_ref().map(|c| c.info()),
        state.surveyor.as_ref().map(|c| c.info()),
        state.operator.as_ref().map(|c| c.info()),
    ];
    Json(agents.into_iter().flatten().collect())
}

/// `POST /projects/{id}/history`: the Chronicler's briefing of the project's
/// git history. An empty body means the default task.
async fn project_history(
    State(state): State<AppState>,
    Caller(principal): Caller,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let briefer = state.chronicler.clone();
    briefing(
        &state,
        &principal,
        &id,
        request,
        AgentKind::Chronicler,
        briefer,
        "the Chronicler",
    )
    .await
}

/// `POST /projects/{id}/overview`: the Surveyor's briefing of the project's
/// files and documentation. An empty body means the default task.
async fn project_overview(
    State(state): State<AppState>,
    Caller(principal): Caller,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let briefer = state.surveyor.clone();
    briefing(
        &state,
        &principal,
        &id,
        request,
        AgentKind::Surveyor,
        briefer,
        "the Surveyor",
    )
    .await
}

/// `POST /projects/{id}/commands`: the Operator's commands for the project.
/// The body and errors are those of the other briefing routes; the response is
/// a `CommandsResult`.
async fn project_commands(
    State(state): State<AppState>,
    Caller(principal): Caller,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let (task, scope) = match prepare_task(
        &state,
        &principal,
        &id,
        request,
        AgentKind::Operator,
        state.operator.is_some(),
        "the Operator",
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(response) => return *response,
    };
    let Some(operator) = &state.operator else {
        return error_response(ParseError::Internal {
            detail: "the Operator is not configured on this service".to_owned(),
        });
    };
    match operator.brief(task, &scope).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => error_response(error),
    }
}

/// `GET /projects/{id}/profile`: the deterministic `ProjectProfile` of the
/// project folder. No model is called, so no agent needs to be configured; the
/// project is still resolved for [`AgentKind::Surveyor`].
async fn project_profile(
    State(state): State<AppState>,
    Caller(principal): Caller,
    Path(id): Path<String>,
) -> Response {
    let scope = match resolve_scope(&state, &principal, AgentKind::Surveyor, Some(&id)) {
        Ok(Some(scope)) => scope,
        Ok(None) => {
            return error_response(ParseError::Internal {
                detail: "the project was not resolved".to_owned(),
            });
        }
        Err(failure) => return failure.into_response(),
    };
    // The folder walk is blocking file I/O.
    let root = scope.root.clone();
    match tokio::task::spawn_blocking(move || daftprompt_core::languages::profile_root(root)).await
    {
        Ok(Ok(profile)) => Json(profile).into_response(),
        Ok(Err(error)) => ProjectFailure {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            kind: "project_unavailable",
            detail: format!("the project folder cannot be read: {error}"),
        }
        .into_response(),
        Err(error) => error_response(ParseError::Internal {
            detail: format!("the profile task failed: {error}"),
        }),
    }
}

/// Shared handler of the `BriefingResult` routes. An empty body means the
/// default task; 404, 403 and 422 are answered before any model call.
async fn briefing(
    state: &AppState,
    principal: &Principal,
    id: &str,
    request: Request,
    agent: AgentKind,
    briefer: Option<Arc<dyn Briefer>>,
    name: &str,
) -> Response {
    let (task, scope) = match prepare_task(
        state,
        principal,
        id,
        request,
        agent,
        briefer.is_some(),
        name,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(response) => return *response,
    };
    let Some(briefer) = briefer else {
        return error_response(ParseError::Internal {
            detail: format!("{name} is not configured on this service"),
        });
    };
    match briefer.brief(task, &scope).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => error_response(error),
    }
}

/// The request body and the resolved project of a briefing route, or the
/// error response to send. An empty body is the default task.
async fn prepare_task(
    state: &AppState,
    principal: &Principal,
    id: &str,
    request: Request,
    agent: AgentKind,
    configured: bool,
    name: &str,
) -> Result<(AgentTaskRequest, ProjectScope), Box<Response>> {
    let limit = state.config.max_body_bytes;
    let body = match to_bytes(request.into_body(), limit).await {
        Ok(body) => body,
        Err(error) => return Err(Box::new(error_response(body_error(&error, limit)))),
    };
    let task: AgentTaskRequest = if body.iter().all(u8::is_ascii_whitespace) {
        AgentTaskRequest::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(task) => task,
            Err(error) => {
                return Err(Box::new(error_response(ParseError::InvalidRequest {
                    detail: format!(
                        "the request body is not a valid AgentTaskRequest JSON object: {error}"
                    ),
                })));
            }
        }
    };

    if !configured {
        return Err(Box::new(error_response(ParseError::Internal {
            detail: format!("{name} is not configured on this service"),
        })));
    }

    match resolve_scope(state, principal, agent, Some(id)) {
        Ok(Some(scope)) => Ok((task, scope)),
        Ok(None) => Err(Box::new(error_response(ParseError::Internal {
            detail: "the project was not resolved".to_owned(),
        }))),
        Err(failure) => Err(Box::new(failure.into_response())),
    }
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

/// A project-route failure, serialized as `{ "kind", "detail" }` like
/// [`ParseError`].
struct ProjectFailure {
    status: StatusCode,
    kind: &'static str,
    detail: String,
}

impl IntoResponse for ProjectFailure {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "kind": self.kind, "detail": self.detail })),
        )
            .into_response()
    }
}

impl From<StoreError> for ProjectFailure {
    fn from(error: StoreError) -> Self {
        let (status, kind) = match &error {
            StoreError::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_request"),
            StoreError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            StoreError::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        Self {
            status,
            kind,
            detail: error.to_string(),
        }
    }
}

impl From<AccessError> for ProjectFailure {
    fn from(error: AccessError) -> Self {
        let (status, kind) = match &error {
            AccessError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            AccessError::Denied(_) => (StatusCode::FORBIDDEN, "forbidden"),
            // The request is well formed but names a project whose folder
            // cannot be used right now: 422, not 400.
            AccessError::UnusableRoot(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "project_unavailable")
            }
            AccessError::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        Self {
            status,
            kind,
            detail: error.to_string(),
        }
    }
}

/// The project scope for one agent call. A request with no `project_id`
/// needs no store and gets `None`.
fn resolve_scope(
    state: &AppState,
    principal: &Principal,
    agent: AgentKind,
    project_id: Option<&str>,
) -> Result<Option<ProjectScope>, ProjectFailure> {
    if project_id.is_none() {
        return Ok(None);
    }
    let store = store(state)?;
    Ok(resolve_project(
        store,
        state.policy.as_ref(),
        principal,
        agent,
        project_id,
    )?)
}

fn store(state: &AppState) -> Result<&Store, ProjectFailure> {
    state.store.as_deref().ok_or_else(|| ProjectFailure {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        kind: "internal_error",
        detail: "project storage is not configured on this service".to_owned(),
    })
}

/// Read and decode a small JSON body, with the usual error shape.
async fn project_body<T: DeserializeOwned>(
    state: &AppState,
    request: Request,
) -> Result<T, ProjectFailure> {
    let limit = state.config.max_body_bytes;
    let body = to_bytes(request.into_body(), limit)
        .await
        .map_err(|error| ProjectFailure {
            status: StatusCode::BAD_REQUEST,
            kind: "invalid_request",
            detail: format!("the request body could not be read: {error}"),
        })?;
    serde_json::from_slice(&body).map_err(|error| ProjectFailure {
        status: StatusCode::BAD_REQUEST,
        kind: "invalid_request",
        detail: format!("the request body is not a valid JSON object for this route: {error}"),
    })
}

async fn list_projects(
    State(state): State<AppState>,
) -> Result<Json<Vec<Project>>, ProjectFailure> {
    Ok(Json(store(&state)?.list_projects()?))
}

/// `201` for a newly registered folder, `200` with the existing project when
/// the folder was already registered.
async fn create_project(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, ProjectFailure> {
    let body: CreateProjectRequest = project_body(&state, request).await?;
    let (project, created) = store(&state)?.add_project(&body.path, body.name.as_deref())?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(project)).into_response())
}

async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Project>, ProjectFailure> {
    Ok(Json(store(&state)?.get_project(&id)?))
}

async fn open_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Project>, ProjectFailure> {
    Ok(Json(store(&state)?.mark_project_opened(&id)?))
}

async fn rename_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Json<Project>, ProjectFailure> {
    let body: RenameProjectRequest = project_body(&state, request).await?;
    Ok(Json(store(&state)?.rename_project(&id, &body.name)?))
}

async fn delete_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ProjectFailure> {
    store(&state)?.remove_project(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

fn body_error(error: &axum::Error, limit: usize) -> ParseError {
    let oversized = error
        .source()
        .is_some_and(|source| source.is::<LengthLimitError>());

    if oversized {
        ParseError::OversizedInput {
            detail: format!("the JSON request body exceeds the {limit} byte envelope limit"),
        }
    } else {
        ParseError::InvalidRequest {
            detail: format!("the request body could not be read: {error}"),
        }
    }
}

fn error_response(error: ParseError) -> Response {
    (status_for(&error), Json(error)).into_response()
}

/// The status code for one explicit parse failure.
pub fn status_for(error: &ParseError) -> StatusCode {
    match error {
        ParseError::InvalidRequest { .. } => StatusCode::BAD_REQUEST,
        ParseError::PayloadMismatch { .. } | ParseError::InvalidTarget { .. } => {
            StatusCode::BAD_REQUEST
        }
        ParseError::OversizedInput { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        ParseError::InvalidOutput { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        ParseError::Provider { .. } => StatusCode::BAD_GATEWAY,
        ParseError::Timeout { .. } => StatusCode::GATEWAY_TIMEOUT,
        ParseError::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_kind_has_a_status() {
        let cases = [
            (
                ParseError::InvalidRequest {
                    detail: String::new(),
                },
                StatusCode::BAD_REQUEST,
            ),
            (
                ParseError::OversizedInput {
                    detail: String::new(),
                },
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
            (
                ParseError::PayloadMismatch {
                    detail: String::new(),
                },
                StatusCode::BAD_REQUEST,
            ),
            (
                ParseError::InvalidTarget {
                    detail: String::new(),
                },
                StatusCode::BAD_REQUEST,
            ),
            (
                ParseError::InvalidOutput {
                    detail: String::new(),
                    attempts: 3,
                },
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                ParseError::Provider {
                    detail: String::new(),
                },
                StatusCode::BAD_GATEWAY,
            ),
            (
                ParseError::Timeout {
                    detail: String::new(),
                },
                StatusCode::GATEWAY_TIMEOUT,
            ),
            (
                ParseError::Internal {
                    detail: String::new(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(status_for(&error), expected, "kind {}", error.kind());
        }
    }

    #[test]
    fn the_default_envelope_leaves_room_for_oversized_text() {
        let config = ApiConfig::default();
        assert!(config.max_body_bytes > DEFAULT_MAX_INPUT_BYTES);
    }
}
