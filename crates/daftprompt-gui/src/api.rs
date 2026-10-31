//! The GUI's only link to parsing: a small blocking HTTP client.
//!
//! Every call runs on a background thread created by the app; this module
//! never touches akar or the rendering loop. It speaks the documented
//! `POST /parse` contract from `docs/api.md` and turns both transport failures
//! and serialized [`ParseError`](daftprompt_types::ParseError) bodies into an
//! [`ApiError`] the view can show.

use std::fmt;
use std::time::Duration;

use daftprompt_types::{ParseError, ParseRequest, ParseResult};

/// The endpoint the API documents as its default.
pub const DEFAULT_API_URL: &str = "http://127.0.0.1:8080/parse";

/// The API allows three 60-second model attempts; leave room for transport and
/// request setup before the client gives up.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

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

/// Interpret a non-2xx response. The API always serializes a `ParseError`; a
/// proxy or a stopped process may return HTML or an empty body instead.
pub fn error_from_response(status: u16, body: &str) -> ApiError {
    match serde_json::from_str::<ParseError>(body) {
        Ok(error) => error.into(),
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
        Ok(Self::new(url, Duration::from_secs(timeout_secs.max(1))))
    }

    /// A client for an explicit endpoint. Used by tests and by `from_env`.
    pub fn new(url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            url: url.into(),
            timeout,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Parse one independent submission. There is no session or history.
    pub fn parse(&self, text: &str) -> Result<ParseResult, ApiError> {
        let request = ParseRequest::new(text);
        let body = serde_json::to_vec(&request)
            .map_err(|error| ApiError::decode(format!("could not encode the request: {error}")))?;

        let response = ureq::post(&self.url)
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(self.timeout))
            .build()
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .send(&body[..]);

        let mut response = match response {
            Ok(response) => response,
            Err(error) => return Err(ApiError::connection(error.to_string())),
        };

        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().map_err(|error| {
            ApiError::connection(format!("could not read the response: {error}"))
        })?;

        if !(200..300).contains(&status) {
            return Err(error_from_response(status, &text));
        }

        serde_json::from_str::<ParseResult>(&text).map_err(|error| {
            ApiError::decode(format!("the response is not a ParseResult: {error}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn canned_result_json() -> String {
        serde_json::json!({
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
            .parse("Plan\nQ?\nA\n")
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
    fn the_documented_default_is_a_local_parse_endpoint() {
        assert_eq!(DEFAULT_API_URL, "http://127.0.0.1:8080/parse");
    }

    #[test]
    fn explicit_clients_keep_their_endpoint() {
        let client = ParseClient::new("http://127.0.0.1:9999/parse", Duration::from_secs(1));
        assert_eq!(client.url(), "http://127.0.0.1:9999/parse");
    }
}
