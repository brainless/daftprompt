//! The GUI's only link to parsing: a small blocking HTTP client.
//!
//! Every call runs on a background thread created by the app; this module
//! never touches akar or the rendering loop. It speaks the documented
//! `POST /parse` contract from `docs/api.md` and turns both transport failures
//! and serialized [`ParseError`](daftprompt_types::ParseError) bodies into an
//! [`ApiError`] the view can show.

use std::fmt;
use std::time::Duration;

use daftprompt_types::{
    AgentTaskRequest, BriefingResult, ClarifyRequest, ClarifyResult, CommandsResult,
    CreateProjectRequest, Delivered, ParseError, ParseRequest, ParseResult, Project,
};
use serde::{Serialize, de::DeserializeOwned};

/// The endpoint the API documents as its default.
pub const DEFAULT_API_URL: &str = "http://127.0.0.1:8080/parse";

/// The API allows three 60-second model attempts; leave room for transport and
/// request setup before the client gives up.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// `/clarify` allows up to 120 seconds per attempt and three attempts.
const DEFAULT_CLARIFY_TIMEOUT: Duration = Duration::from_secs(420);

/// A briefing agent makes several model calls with tools; the API bounds each
/// attempt itself, so this only has to outlast three of them.
const BRIEFING_TIMEOUT: Duration = Duration::from_secs(420);

/// Project calls only touch the local registry and the file system.
const PROJECT_TIMEOUT: Duration = Duration::from_secs(30);

/// The `/clarify` endpoint that belongs with a `/parse` URL: same host, the
/// last path segment replaced. A URL that does not end in `/parse` gets
/// `/clarify` appended.
pub fn clarify_url(parse_url: &str) -> String {
    sibling_url(parse_url, "clarify")
}

/// The `/projects` endpoint that belongs with a `/parse` URL, derived the same
/// way as [`clarify_url`].
pub fn projects_url(parse_url: &str) -> String {
    sibling_url(parse_url, "projects")
}

/// A briefing route of one project, for example `.../projects/{id}/history`.
pub fn briefing_url(parse_url: &str, project_id: &str, route: &str) -> String {
    format!("{}/{project_id}/{route}", projects_url(parse_url))
}

fn sibling_url(parse_url: &str, route: &str) -> String {
    let trimmed = parse_url.trim_end_matches('/');
    match trimmed.strip_suffix("/parse") {
        Some(base) => format!("{base}/{route}"),
        None => format!("{trimmed}/{route}"),
    }
}

/// The request for `text`, carrying the selected project and path when there
/// are any.
pub fn build_parse_request(
    text: &str,
    project_id: Option<&str>,
    path_id: Option<&str>,
) -> ParseRequest {
    ParseRequest {
        project_id: project_id.map(str::to_owned),
        project_path_id: project_id.and(path_id).map(str::to_owned),
        ..ParseRequest::new(text)
    }
}

/// The body of a default-task agent request for one path. `refresh` is set
/// only by the explicit re-run controls; an ordinary open may reuse a stored
/// response.
pub fn build_agent_request(path_id: Option<&str>, refresh: bool) -> AgentTaskRequest {
    AgentTaskRequest {
        project_path_id: path_id.map(str::to_owned),
        refresh,
        ..AgentTaskRequest::default()
    }
}

/// One failed parse, shaped for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// Machine-readable kind: an API `ParseError` kind, or a client-side kind
    /// such as `connection` or `decode`.
    pub kind: String,
    pub detail: String,
    /// Attempts used, when the API reported an `invalid_output` failure.
    pub attempts: Option<u32>,
}

impl ApiError {
    pub fn new(kind: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            detail: detail.into(),
            attempts: None,
        }
    }

    /// A request rejected before it reached the API, such as empty text.
    pub fn local(detail: impl Into<String>) -> Self {
        Self::new("invalid_request", detail)
    }

    /// A transport failure: the API was not reachable or the exchange broke.
    pub fn connection(detail: impl Into<String>) -> Self {
        Self::new("connection", detail)
    }

    /// A response body that did not match the shared result contract.
    pub fn decode(detail: impl Into<String>) -> Self {
        Self::new("decode", detail)
    }

    /// A short heading for the error state.
    pub fn headline(&self) -> &'static str {
        match self.kind.as_str() {
            "invalid_request" => "Nothing was sent",
            "oversized_input" => "The text is too large",
            "invalid_output" => "Scribe could not produce a valid result",
            "provider_error" => "The model provider failed",
            "timeout" => "The model call timed out",
            "internal_error" => "The API had an internal error",
            "connection" => "Could not reach the API",
            "decode" => "The API response could not be read",
            "payload_mismatch" => "The source text does not match the parse result",
            "invalid_target" => "The span cannot be explained",
            "not_found" => "The project is not registered",
            "forbidden" => "This project is not available to you",
            "project_unavailable" => "The project folder cannot be read",
            _ => "Parsing failed",
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.headline(), self.detail)?;
        if let Some(attempts) = self.attempts {
            write!(f, " ({attempts} attempt(s))")?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

impl From<ParseError> for ApiError {
    fn from(error: ParseError) -> Self {
        let attempts = match &error {
            ParseError::InvalidOutput { attempts, .. } => Some(*attempts),
            _ => None,
        };
        Self {
            kind: error.kind().to_owned(),
            detail: error.to_string(),
            attempts,
        }
    }
}

/// The `{kind, detail}` body every API route serializes on failure. It is wider
/// than `ParseError`: project routes add kinds such as `not_found`,
/// `forbidden`, and `project_unavailable`.
#[derive(serde::Deserialize)]
struct ErrorBody {
    kind: String,
    detail: String,
    #[serde(default)]
    attempts: Option<u32>,
}

/// Interpret a non-2xx response. The API serializes `{kind, detail}`; a proxy
/// or a stopped process may return HTML or an empty body instead.
pub fn error_from_response(status: u16, body: &str) -> ApiError {
    match serde_json::from_str::<ErrorBody>(body) {
        Ok(error) => ApiError {
            kind: error.kind,
            detail: error.detail,
            attempts: error.attempts,
        },
        Err(_) => {
            let detail = body.trim();
            let detail = if detail.is_empty() {
                format!("the API returned HTTP {status} with an empty body")
            } else {
                format!("the API returned HTTP {status}: {detail}")
            };
            ApiError::new(format!("http_{status}"), detail)
        }
    }
}

/// A blocking `POST /parse` client.
///
/// The app spawns a thread per submission, so `ParseClient` stays cheap to
/// clone and never runs on the rendering thread.
#[derive(Debug, Clone)]
pub struct ParseClient {
    url: String,
    timeout: Duration,
    clarify_timeout: Duration,
    project_timeout: Duration,
}

impl ParseClient {
    /// Read the endpoint from `DAFTPROMPT_API_URL`, defaulting to the local API.
    pub fn from_env() -> Result<Self, String> {
        let url = std::env::var("DAFTPROMPT_API_URL")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_API_URL.to_owned());
        let timeout_secs = match std::env::var("DAFTPROMPT_GUI_TIMEOUT_SECS") {
            Ok(value) => value.trim().parse::<u64>().map_err(|_| {
                format!("DAFTPROMPT_GUI_TIMEOUT_SECS expects a positive integer, got {value:?}")
            })?,
            Err(_) => DEFAULT_TIMEOUT.as_secs(),
        };
        let clarify_secs = match std::env::var("DAFTPROMPT_GUI_CLARIFY_TIMEOUT_SECS") {
            Ok(value) => value.trim().parse::<u64>().map_err(|_| {
                format!(
                    "DAFTPROMPT_GUI_CLARIFY_TIMEOUT_SECS expects a positive integer, got {value:?}"
                )
            })?,
            Err(_) => DEFAULT_CLARIFY_TIMEOUT.as_secs(),
        };
        let mut client = Self::new(url, Duration::from_secs(timeout_secs.max(1)));
        client.clarify_timeout = Duration::from_secs(clarify_secs.max(1));
        Ok(client)
    }

    /// A client for an explicit endpoint. Used by tests and by `from_env`.
    pub fn new(url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            url: url.into(),
            timeout,
            clarify_timeout: DEFAULT_CLARIFY_TIMEOUT,
            project_timeout: PROJECT_TIMEOUT,
        }
    }

    /// The endpoint `clarify` posts to.
    pub fn clarify_url(&self) -> String {
        clarify_url(&self.url)
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Parse one independent submission. There is no session or history.
    pub fn parse(&self, request: &ParseRequest) -> Result<ParseResult, ApiError> {
        post_json(&self.url, self.timeout, request, "ParseResult")
    }

    /// `GET /projects`: registered projects, most recently opened first.
    pub fn list_projects(&self) -> Result<Vec<Project>, ApiError> {
        exchange(
            &projects_url(&self.url),
            None,
            self.project_timeout,
            "project list",
        )
    }

    /// `POST /projects`: register a folder (idempotent) and return its project.
    pub fn create_project(&self, path: &str) -> Result<Project, ApiError> {
        let request = CreateProjectRequest {
            path: path.to_owned(),
            name: None,
        };
        post_json(
            &projects_url(&self.url),
            self.project_timeout,
            &request,
            "Project",
        )
    }

    /// `POST /projects/{id}/open`: mark a project as just opened.
    pub fn open_project(&self, id: &str) -> Result<Project, ApiError> {
        let url = format!("{}/{id}/open", projects_url(&self.url));
        exchange(&url, Some(&[]), self.project_timeout, "Project")
    }

    /// `POST /projects/{id}/history`: Chronicler's default task.
    pub fn history(
        &self,
        project_id: &str,
        path_id: Option<&str>,
        refresh: bool,
    ) -> Result<Delivered<BriefingResult>, ApiError> {
        self.briefing(project_id, "history", path_id, refresh, "BriefingResult")
    }

    /// `POST /projects/{id}/overview`: Surveyor's default task.
    pub fn overview(
        &self,
        project_id: &str,
        path_id: Option<&str>,
        refresh: bool,
    ) -> Result<Delivered<BriefingResult>, ApiError> {
        self.briefing(project_id, "overview", path_id, refresh, "BriefingResult")
    }

    /// `POST /projects/{id}/commands`: Operator's default task.
    pub fn commands(
        &self,
        project_id: &str,
        path_id: Option<&str>,
        refresh: bool,
    ) -> Result<Delivered<CommandsResult>, ApiError> {
        self.briefing(project_id, "commands", path_id, refresh, "CommandsResult")
    }

    fn briefing<T: DeserializeOwned>(
        &self,
        project_id: &str,
        route: &str,
        path_id: Option<&str>,
        refresh: bool,
        what: &str,
    ) -> Result<Delivered<T>, ApiError> {
        post_json(
            &briefing_url(&self.url, project_id, route),
            BRIEFING_TIMEOUT,
            &build_agent_request(path_id, refresh),
            what,
        )
    }

    /// Explain one span. Stateless: the request carries every earlier turn.
    pub fn clarify(&self, request: &ClarifyRequest) -> Result<ClarifyResult, ApiError> {
        post_json(
            &self.clarify_url(),
            self.clarify_timeout,
            request,
            "ClarifyResult",
        )
    }
}

fn post_json<B: Serialize, T: DeserializeOwned>(
    url: &str,
    timeout: Duration,
    request: &B,
    what: &str,
) -> Result<T, ApiError> {
    let body = serde_json::to_vec(request)
        .map_err(|error| ApiError::decode(format!("could not encode the request: {error}")))?;
    exchange(url, Some(&body), timeout, what)
}

/// Send one request and decode a 2xx JSON body. `body` is `Some` for POST
/// (possibly empty) and `None` for GET.
fn exchange<T: DeserializeOwned>(
    url: &str,
    body: Option<&[u8]>,
    timeout: Duration,
    what: &str,
) -> Result<T, ApiError> {
    let response = match body {
        Some(body) => ureq::post(url)
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build()
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .send(body),
        None => ureq::get(url)
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build()
            .header("accept", "application/json")
            .call(),
    };

    let mut response = match response {
        Ok(response) => response,
        Err(error) => return Err(ApiError::connection(error.to_string())),
    };

    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| ApiError::connection(format!("could not read the response: {error}")))?;

    if !(200..300).contains(&status) {
        return Err(error_from_response(status, &text));
    }

    serde_json::from_str::<T>(&text)
        .map_err(|error| ApiError::decode(format!("the response is not a {what}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn briefing_routes_hang_off_the_project() {
        assert_eq!(
            briefing_url("http://127.0.0.1:8080/parse", "project-1", "history"),
            "http://127.0.0.1:8080/projects/project-1/history"
        );
    }
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn parse_requests_follow_the_selected_path() {
        let request = build_parse_request("text", Some("p1"), Some("w1"));
        assert_eq!(request.project_id.as_deref(), Some("p1"));
        assert_eq!(request.project_path_id.as_deref(), Some("w1"));
        // A path means nothing without a project.
        let request = build_parse_request("text", None, Some("w1"));
        assert_eq!(request.project_path_id, None);
        let request = build_parse_request("text", Some("p1"), None);
        assert_eq!(request.project_path_id, None);
    }

    #[test]
    fn agent_requests_carry_the_path_and_only_explicit_refresh() {
        let open = serde_json::to_value(build_agent_request(Some("w1"), false)).unwrap();
        assert_eq!(open, serde_json::json!({ "project_path_id": "w1" }));
        let rerun = serde_json::to_value(build_agent_request(Some("w1"), true)).unwrap();
        assert_eq!(
            rerun,
            serde_json::json!({ "project_path_id": "w1", "refresh": true })
        );
        let legacy = serde_json::to_value(build_agent_request(None, false)).unwrap();
        assert_eq!(legacy, serde_json::json!({}));
    }

    #[test]
    fn delivered_briefings_decode_with_and_without_metadata() {
        let mut body =
            serde_json::to_value(crate::briefing::tests::briefing_result("p1", "x")).unwrap();
        // An older API answers the bare result.
        let bare: Delivered<BriefingResult> = serde_json::from_value(body.clone()).unwrap();
        assert!(bare.cache.is_none() && bare.project_path_id.is_none());
        body["project_path_id"] = "w1".into();
        body["cache"] = serde_json::json!({
            "requested_path_id": "w1",
            "source_path_id": "d",
            "response_id": "r1",
            "captured_at": "2026-11-08T10:30:00Z",
            "kind": "other_path",
        });
        let reused: Delivered<BriefingResult> = serde_json::from_value(body).unwrap();
        assert_eq!(reused.project_path_id.as_deref(), Some("w1"));
        assert_eq!(
            reused.cache.map(|cache| cache.kind),
            Some(daftprompt_types::CacheKind::OtherPath)
        );
    }

    fn canned_result_json() -> String {
        serde_json::json!({
            "content_type": "plain",
            "content_type_source": "detected",
            "payload": {
                "hash": "hash",
                "content": "Plan\nQ?\nA\n",
                "created_at": "now"
            },
            "source": null,
            "nodes": [
                { "id": "root", "parent_id": null, "author": "unknown", "run_id": null, "created_at": "now" }
            ],
            "node_contexts": [
                {
                    "id": "ctx",
                    "node_id": "root",
                    "seq": 0,
                    "target": { "kind": "payload", "payload_hash": "hash" },
                    "range": null
                }
            ],
            "labels": [],
            "spans": [],
            "run": {
                "id": "run",
                "agent": "scribe",
                "provider": "openai",
                "model": "gpt-4o-mini",
                "prompt_version": "scribe-v1",
                "started_at": "now",
                "ended_at": "now",
                "status": "done",
                "attempts": 1
            },
            "run_inputs": [{ "run_id": "run", "node_id": "root" }],
            "tool_calls": []
        })
        .to_string()
    }

    #[test]
    fn posts_json_and_decodes_a_parse_result() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a stub API");
        let address = listener.local_addr().expect("stub address");
        let body = canned_result_json();
        let response_body = body.clone();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept a request");
            let mut received = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = socket.read(&mut buffer).expect("read the request");
                if read == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..read]);
                if let Some(head_end) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&received[..head_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if received.len() >= head_end + 4 + content_length {
                        break;
                    }
                }
            }

            let request = String::from_utf8_lossy(&received);
            assert!(request.starts_with("POST /parse HTTP/1.1"));
            assert!(request.contains("content-type: application/json"));
            assert!(request.contains("Plan"));
            assert!(request.contains("\"project_id\":\"project-1\""));

            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            socket
                .write_all(response.as_bytes())
                .expect("write the response");
        });

        let client = ParseClient::new(format!("http://{address}/parse"), Duration::from_secs(5));
        let result = client
            .parse(&build_parse_request(
                "Plan\nQ?\nA\n",
                Some("project-1"),
                None,
            ))
            .expect("parse the stub result");
        assert_eq!(result.payload.content, "Plan\nQ?\nA\n");
        assert_eq!(result.run.model.as_deref(), Some("gpt-4o-mini"));
        server.join().expect("stub server");
    }

    #[test]
    fn serialized_parse_errors_keep_their_kind_and_attempts() {
        let error = error_from_response(
            422,
            r#"{"kind":"invalid_output","detail":"spans 1 and 2 overlap","attempts":3}"#,
        );
        assert_eq!(error.kind, "invalid_output");
        assert_eq!(error.attempts, Some(3));
        assert!(error.detail.contains("spans 1 and 2 overlap"));
        assert_eq!(error.headline(), "Scribe could not produce a valid result");
    }

    #[test]
    fn project_error_kinds_survive_decoding() {
        for (status, kind) in [
            (404, "not_found"),
            (403, "forbidden"),
            (422, "project_unavailable"),
        ] {
            let body = format!(r#"{{"kind":"{kind}","detail":"the folder is gone"}}"#);
            let error = error_from_response(status, &body);
            assert_eq!(error.kind, kind);
            assert_eq!(error.detail, "the folder is gone");
            assert_ne!(error.headline(), "Parsing failed");
        }
    }

    #[test]
    fn parse_requests_carry_the_selected_project() {
        assert_eq!(build_parse_request("Q?", None, None).project_id, None);
        let request = build_parse_request("Q?", Some("p1"), None);
        assert_eq!(request.project_id.as_deref(), Some("p1"));
        assert_eq!(request.text, "Q?");
    }

    #[test]
    fn projects_endpoint_sits_beside_parse() {
        assert_eq!(
            projects_url("http://127.0.0.1:8080/parse"),
            "http://127.0.0.1:8080/projects"
        );
        assert_eq!(projects_url("http://h:1"), "http://h:1/projects");
    }

    #[test]
    fn non_json_error_bodies_keep_the_status_and_text() {
        let error = error_from_response(502, "<html>bad gateway</html>");
        assert_eq!(error.kind, "http_502");
        assert!(error.detail.contains("HTTP 502"));
        assert!(error.detail.contains("bad gateway"));
    }

    #[test]
    fn empty_error_bodies_say_so() {
        let error = error_from_response(500, "  ");
        assert_eq!(error.kind, "http_500");
        assert!(error.detail.contains("empty body"));
    }

    #[test]
    fn clarify_endpoint_sits_beside_parse() {
        assert_eq!(
            clarify_url("http://127.0.0.1:8080/parse"),
            "http://127.0.0.1:8080/clarify"
        );
        assert_eq!(
            clarify_url("http://h:1/api/parse/"),
            "http://h:1/api/clarify"
        );
        assert_eq!(clarify_url("http://h:1"), "http://h:1/clarify");
    }

    #[test]
    fn clarify_posts_to_the_clarify_path_and_reports_api_errors() {
        use daftprompt_types::{ClarifySource, ClarifyTarget};
        let request = ClarifyRequest {
            source: ClarifySource {
                text: "Plan".to_owned(),
                payload_hash: "bad".to_owned(),
                metadata: None,
            },
            target: ClarifyTarget {
                node_id: "n".to_owned(),
                display_id: 1,
                start_byte: 0,
                end_byte: 4,
            },
            turns: Vec::new(),
            question: "Why?".to_owned(),
            author: None,
            project_id: None,
            project_path_id: None,
        };
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a stub API");
        let address = listener.local_addr().expect("stub address");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept a request");
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout");
            let mut received = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !String::from_utf8_lossy(&received).contains("\"question\"") {
                let read = socket.read(&mut buffer).expect("read the request");
                if read == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..read]);
            }
            let head = String::from_utf8_lossy(&received).to_string();
            let body = r#"{"kind":"payload_mismatch","detail":"hash differs"}"#;
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).expect("respond");
            head
        });
        let client = ParseClient::new(format!("http://{address}/parse"), Duration::from_secs(5));
        let error = client.clarify(&request).expect_err("a 400 is an error");
        assert_eq!(error.kind, "payload_mismatch");
        assert!(error.detail.contains("hash differs"));
        let head = server.join().expect("stub server");
        assert!(head.starts_with("POST /clarify HTTP/1.1"));
        assert!(head.contains("\"question\":\"Why?\""));
    }

    #[test]
    fn the_documented_default_is_a_local_parse_endpoint() {
        assert_eq!(DEFAULT_API_URL, "http://127.0.0.1:8080/parse");
    }

    #[test]
    fn explicit_clients_keep_their_endpoint() {
        let client = ParseClient::new("http://127.0.0.1:9999/parse", Duration::from_secs(1));
        assert_eq!(client.url(), "http://127.0.0.1:9999/parse");
    }
}
