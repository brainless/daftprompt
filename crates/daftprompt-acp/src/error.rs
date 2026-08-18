//! Typed errors for the ACP runtime boundary.
//!
//! Task 1's acceptance criteria require that "Timeout, malformed message,
//! broken pipe, early exit, and stderr-tail errors are distinguishable."
//! `agent-client-protocol` reports most connection-level failures as one
//! `ErrorCode::InternalError` with a human-readable `.data` string (see
//! `acp_agent.rs`'s `finish_child_exit`/`write_line_with_shutdown_timeout` in
//! the vendored crate source); [`AcpError::from_connection_error`]
//! classifies those strings into the distinct variants below so callers
//! never need to pattern-match on SDK error text themselves.

use std::time::Duration;

use crate::diagnostic::RawDiagnostic;

/// Errors produced by the daftprompt-acp runtime boundary.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AcpError {
    /// A request did not receive a response within the configured timeout.
    /// The ACP wire protocol has no native "this request timed out" signal
    /// (Task 0 findings §10); this is a client-enforced budget.
    #[error("ACP request {method:?} timed out after {elapsed:?}")]
    Timeout {
        /// The method that timed out, if known.
        method: Option<String>,
        /// The configured timeout duration.
        elapsed: Duration,
    },

    /// A line received from the adapter's stdout was not valid JSON-RPC and
    /// could not be parsed at all (distinct from a validly-shaped but
    /// unrecognized message, which becomes a generic diagnostic event
    /// instead of an error).
    #[error("malformed message on the adapter's stdout")]
    MalformedMessage {
        /// A bounded capture of the offending line.
        raw: RawDiagnostic,
    },

    /// Writing to the adapter's stdin failed because the pipe was already
    /// closed (the adapter exited or closed stdin).
    #[error("broken pipe writing to the adapter: {detail}")]
    BrokenPipe {
        /// Underlying OS-level detail string.
        detail: String,
    },

    /// The adapter process exited (cleanly with a nonzero code, or via
    /// signal) before or during a pending exchange.
    #[error("adapter process exited early: {detail}")]
    EarlyExit {
        /// Process exit detail, including any captured stderr tail.
        detail: String,
    },

    /// The adapter negotiated a protocol version this pinned SDK build does
    /// not implement. `initialize()` validates this rather than silently
    /// proceeding as if the negotiated capabilities were understood.
    #[error("adapter negotiated unsupported ACP protocol version {negotiated}")]
    UnsupportedProtocolVersion {
        /// The protocol version number the adapter returned.
        negotiated: u16,
    },

    /// A well-formed JSON-RPC error response was returned by the adapter.
    #[error("adapter protocol error {code}: {message}")]
    Protocol {
        /// The JSON-RPC error code.
        code: i64,
        /// The error message.
        message: String,
        /// Optional structured error data.
        data: Option<serde_json::Value>,
    },

    /// A permission response referenced an option ID that was not one of
    /// the options the adapter actually offered for that request. Epic 014
    /// Design Decision #7 requires the client to only return an offered
    /// `optionId`; this error enforces that boundary in the runtime rather
    /// than trusting callers.
    #[error("permission option {option_id:?} was not offered for this request")]
    InvalidPermissionOption {
        /// The option ID the caller attempted to send.
        option_id: String,
    },

    /// The referenced permission request is not (or is no longer) pending
    /// (already answered, or the session/turn it belonged to ended).
    #[error("no pending permission request with this id")]
    UnknownPermissionRequest,

    /// The client has already been shut down; no further commands can be
    /// issued.
    #[error("the ACP client has shut down")]
    ShuttingDown,

    /// An error that does not fit a more specific category above. The
    /// original message is preserved for diagnostics.
    #[error("ACP runtime error: {0}")]
    Internal(String),
}

impl AcpError {
    /// Classify a connection-level error surfaced by
    /// `agent_client_protocol::Client::builder()...connect_with(...)`'s
    /// returned future into a distinguishable [`AcpError`] variant.
    #[must_use]
    pub fn from_connection_error(error: agent_client_protocol::Error) -> Self {
        let detail = error
            .data
            .as_ref()
            .and_then(|d| d.as_str().map(str::to_string))
            .unwrap_or_else(|| error.message.clone());

        if detail.contains("Process exited with") {
            return AcpError::EarlyExit { detail };
        }
        if detail.to_ascii_lowercase().contains("broken pipe") {
            return AcpError::BrokenPipe { detail };
        }
        if detail.contains("pending protocol output did not drain") {
            return AcpError::BrokenPipe { detail };
        }

        AcpError::Protocol {
            code: i64::from(<i32 as From<agent_client_protocol::ErrorCode>>::from(
                error.code,
            )),
            message: error.message,
            data: error.data,
        }
    }

    /// Classify a JSON-RPC error response returned by the adapter for one
    /// of our own outgoing requests.
    #[must_use]
    pub fn from_response_error(error: agent_client_protocol::Error) -> Self {
        Self::from_connection_error(error)
    }
}
