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
//! A request whose JSON envelope is too large is rejected with 413 before it
//! can reach the parser. A partly valid graph is never returned.

use std::error::Error as _;
use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use daftprompt_core::{DEFAULT_MAX_INPUT_BYTES, Parser};
use http_body_util::LengthLimitError;

pub use daftprompt_types::{ParseError, ParseRequest, ParseResult};

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
    config: ApiConfig,
}

impl AppState {
    /// State over `parser` with `config`.
    pub fn new(parser: Arc<dyn Parser>, config: ApiConfig) -> Self {
        Self { parser, config }
    }

    /// The active envelope configuration.
    pub fn config(&self) -> ApiConfig {
        self.config
    }
}

/// The HTTP router.
///
/// The only route is stateless `POST /parse`; there are no session, history,
/// or storage endpoints.
pub fn router(state: AppState) -> Router {
    Router::new().route("/parse", post(parse)).with_state(state)
}

async fn parse(State(state): State<AppState>, request: Request) -> Response {
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

    match state.parser.parse(request).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => error_response(error),
    }
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
